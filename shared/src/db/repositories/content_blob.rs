use crate::db::error::DatabaseError;
use serde::Serialize;
use sqlx::{FromRow, PgPool, Row};

/// Represents an orphan blob ready for deletion
#[derive(Debug, FromRow)]
pub struct OrphanBlob {
    pub id: String,
    pub size_bytes: i64,
}

/// Statistics about orphaned content blobs
#[derive(Debug, Serialize)]
pub struct OrphanStats {
    /// Orphans not yet marked (new this cycle)
    pub unmarked_orphans: i64,
    /// Orphans marked but not yet expired
    pub pending_orphans: i64,
    /// Orphans ready for deletion (past retention period)
    pub expired_orphans: i64,
    /// Total size of orphaned blobs in bytes
    pub orphan_size_bytes: i64,
}

pub struct ContentBlobRepository {
    pool: PgPool,
}

const MARK_ORPHANS_BATCH: i64 = 100_000;
const MARK_ORPHANS_QUERY: &str = r#"
    WITH candidates AS (
        SELECT cb.id
        FROM content_blobs cb
        WHERE cb.orphaned_at IS NULL
          AND NOT EXISTS (
              SELECT 1 FROM documents d WHERE d.content_id = cb.id::text
          )
          AND NOT EXISTS (
              SELECT 1 FROM connector_events_queue q
              WHERE q.status IN ('pending', 'processing')
                AND q.payload->>'content_id' = cb.id::text
          )
          AND NOT EXISTS (
              SELECT 1 FROM uploads u WHERE u.content_id = cb.id
          )
        LIMIT $1
    )
    UPDATE content_blobs cb
    SET orphaned_at = CURRENT_TIMESTAMP
    FROM candidates
    WHERE cb.id = candidates.id
    "#;

const UNMARK_NON_ORPHANS_SCAN_BATCH: i64 = 10_000;
const UNMARK_NON_ORPHANS_QUERY: &str = r#"
    WITH scan_batch AS MATERIALIZED (
        SELECT cb.id
        FROM content_blobs cb
        WHERE cb.orphaned_at IS NOT NULL
        ORDER BY cb.orphaned_at
        LIMIT $1
        FOR UPDATE OF cb
    )
    UPDATE content_blobs cb
    SET orphaned_at = NULL
    FROM scan_batch
    WHERE cb.id = scan_batch.id
      AND (
          EXISTS (
              SELECT 1 FROM documents d WHERE d.content_id = cb.id::text
          )
          OR EXISTS (
              SELECT 1 FROM connector_events_queue q
              WHERE q.status IN ('pending', 'processing')
                AND q.payload->>'content_id' = cb.id::text
          )
          OR EXISTS (
              SELECT 1 FROM uploads u WHERE u.content_id = cb.id
          )
      )
    "#;

impl ContentBlobRepository {
    pub fn new(pool: &PgPool) -> Self {
        Self { pool: pool.clone() }
    }

    /// Mark blobs as orphaned if they are not referenced by any document,
    /// upload, or any pending/processing queue event.
    /// Returns the number of blobs marked.
    ///
    /// Writes are capped at MARK_ORPHANS_BATCH rows per call: the previous unbounded
    /// `NOT IN` anti-joins against the full `content_blobs` table materialized
    /// hash tables over every row and took 30+ hours on production data (5M+
    /// blobs). Indexed NOT EXISTS predicates reduce normal runtime; transaction-
    /// local statement and lock timeouts provide the hard wall-clock bound when
    /// a sparse match still requires a large scan.
    pub async fn mark_orphans(&self) -> Result<i64, DatabaseError> {
        let mut transaction = self.pool.begin().await?;
        sqlx::query("SET LOCAL statement_timeout = '30s'")
            .execute(&mut *transaction)
            .await?;
        sqlx::query("SET LOCAL lock_timeout = '2s'")
            .execute(&mut *transaction)
            .await?;

        let result = sqlx::query(MARK_ORPHANS_QUERY)
            .bind(MARK_ORPHANS_BATCH)
            .execute(&mut *transaction)
            .await?;
        transaction.commit().await?;

        Ok(result.rows_affected() as i64)
    }

    /// Unmark blobs that are no longer orphaned (got re-referenced).
    /// Returns the number of blobs unmarked, bounded to one batch per call.
    ///
    /// The oldest orphan scan is capped before reference checks, so each call
    /// performs at most one bounded set of indexed probes. Transaction-local
    /// statement and lock timeouts prevent a GC pass from monopolizing the
    /// database; an error rolls the batch back before the delete phase can run.
    pub async fn unmark_non_orphans(&self) -> Result<i64, DatabaseError> {
        let mut transaction = self.pool.begin().await?;
        sqlx::query("SET LOCAL statement_timeout = '30s'")
            .execute(&mut *transaction)
            .await?;
        sqlx::query("SET LOCAL lock_timeout = '2s'")
            .execute(&mut *transaction)
            .await?;

        let result = sqlx::query(UNMARK_NON_ORPHANS_QUERY)
            .bind(UNMARK_NON_ORPHANS_SCAN_BATCH)
            .execute(&mut *transaction)
            .await?;
        transaction.commit().await?;

        Ok(result.rows_affected() as i64)
    }

    /// Fetch a batch of expired orphans ready for deletion, rechecking all
    /// reference sources visible to this statement. FOR UPDATE SKIP LOCKED
    /// prevents overlapping fetches from selecting the same rows, but its lock
    /// ends when this short fetch transaction commits; callers must not treat it
    /// as a lock across a later object-storage deletion.
    pub async fn fetch_expired_orphans(
        &self,
        retention_days: i32,
        batch_size: i32,
    ) -> Result<Vec<OrphanBlob>, DatabaseError> {
        let mut transaction = self.pool.begin().await?;
        sqlx::query("SET LOCAL statement_timeout = '30s'")
            .execute(&mut *transaction)
            .await?;
        sqlx::query("SET LOCAL lock_timeout = '2s'")
            .execute(&mut *transaction)
            .await?;

        let rows = sqlx::query_as::<_, OrphanBlob>(
            r#"
            SELECT cb.id, cb.size_bytes
            FROM content_blobs cb
            WHERE cb.orphaned_at IS NOT NULL
            AND cb.orphaned_at < CURRENT_TIMESTAMP - INTERVAL '1 day' * $1
            AND NOT EXISTS (
                SELECT 1 FROM documents d WHERE d.content_id = cb.id::text
            )
            AND NOT EXISTS (
                SELECT 1 FROM connector_events_queue q
                WHERE q.status IN ('pending', 'processing')
                  AND q.payload->>'content_id' = cb.id::text
            )
            AND NOT EXISTS (
                SELECT 1 FROM uploads u WHERE u.content_id = cb.id
            )
            ORDER BY cb.orphaned_at
            LIMIT $2
            FOR UPDATE OF cb SKIP LOCKED
            "#,
        )
        .bind(retention_days)
        .bind(batch_size)
        .fetch_all(&mut *transaction)
        .await?;
        transaction.commit().await?;

        Ok(rows)
    }

    /// Get statistics about orphaned content blobs.
    pub async fn get_orphan_stats(
        &self,
        retention_days: i32,
    ) -> Result<OrphanStats, DatabaseError> {
        let row = sqlx::query(
            r#"
            SELECT
                COUNT(*) FILTER (
                    WHERE orphaned_at IS NULL
                    AND id NOT IN (
                        SELECT DISTINCT content_id FROM documents WHERE content_id IS NOT NULL
                    )
                    AND id NOT IN (
                        SELECT DISTINCT payload->>'content_id'
                        FROM connector_events_queue
                        WHERE status IN ('pending', 'processing')
                        AND payload->>'content_id' IS NOT NULL
                    )
                    AND id NOT IN (
                        SELECT DISTINCT content_id FROM uploads
                    )
                ) as unmarked_orphans,
                COUNT(*) FILTER (
                    WHERE orphaned_at IS NOT NULL
                    AND orphaned_at >= CURRENT_TIMESTAMP - INTERVAL '1 day' * $1
                ) as pending_orphans,
                COUNT(*) FILTER (
                    WHERE orphaned_at IS NOT NULL
                    AND orphaned_at < CURRENT_TIMESTAMP - INTERVAL '1 day' * $1
                ) as expired_orphans,
                COALESCE(SUM(size_bytes) FILTER (WHERE orphaned_at IS NOT NULL), 0) as orphan_size_bytes
            FROM content_blobs
            "#,
        )
        .bind(retention_days)
        .fetch_one(&self.pool)
        .await?;

        Ok(OrphanStats {
            unmarked_orphans: row.get("unmarked_orphans"),
            pending_orphans: row.get("pending_orphans"),
            expired_orphans: row.get("expired_orphans"),
            orphan_size_bytes: row.get("orphan_size_bytes"),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use testcontainers::{ContainerAsync, ImageExt, runners::AsyncRunner};
    use testcontainers_modules::postgres::Postgres;

    struct TestDatabase {
        pool: PgPool,
        _container: ContainerAsync<Postgres>,
    }

    impl TestDatabase {
        async fn new() -> Self {
            let container = Postgres::default()
                .with_tag("17-alpine")
                .start()
                .await
                .expect("start postgres test container");
            let port = container
                .get_host_port_ipv4(5432)
                .await
                .expect("get postgres port");
            let pool = PgPool::connect(&format!(
                "postgres://postgres:postgres@127.0.0.1:{port}/postgres"
            ))
            .await
            .expect("connect to postgres");

            for statement in [
                "CREATE TABLE content_blobs (id CHAR(26) PRIMARY KEY, size_bytes BIGINT NOT NULL DEFAULT 0, orphaned_at TIMESTAMPTZ)",
                "CREATE INDEX idx_content_blobs_orphaned_at ON content_blobs(orphaned_at) WHERE orphaned_at IS NOT NULL",
                "CREATE TABLE documents (content_id VARCHAR(26))",
                "CREATE INDEX idx_documents_content_id ON documents(content_id)",
                "CREATE TABLE connector_events_queue (payload JSONB NOT NULL, status VARCHAR(20) NOT NULL)",
                "CREATE INDEX idx_queue_payload_content_id ON connector_events_queue ((payload->>'content_id')) WHERE status IN ('pending', 'processing') AND payload->>'content_id' IS NOT NULL",
                "CREATE TABLE uploads (content_id CHAR(26) NOT NULL)",
                "CREATE INDEX idx_uploads_content_id ON uploads(content_id)",
            ] {
                sqlx::query(statement)
                    .execute(&pool)
                    .await
                    .expect("create test schema");
            }

            Self {
                pool,
                _container: container,
            }
        }
    }

    #[tokio::test]
    async fn mark_orphans_can_use_document_index_with_production_id_types() {
        let db = TestDatabase::new().await;

        sqlx::query(
            r#"
            INSERT INTO content_blobs (id, orphaned_at)
            SELECT lpad(value::text, 26, '0'), NULL
            FROM generate_series(1, 100) AS value
            "#,
        )
        .execute(&db.pool)
        .await
        .unwrap();
        sqlx::query(
            r#"
            INSERT INTO documents (content_id)
            SELECT lpad(value::text, 26, '0')
            FROM generate_series(1, 100) AS value
            "#,
        )
        .execute(&db.pool)
        .await
        .unwrap();
        sqlx::query("ANALYZE content_blobs")
            .execute(&db.pool)
            .await
            .unwrap();
        sqlx::query("ANALYZE documents")
            .execute(&db.pool)
            .await
            .unwrap();

        let mut connection = db.pool.acquire().await.unwrap();
        for setting in [
            "SET enable_seqscan = off",
            "SET enable_hashjoin = off",
            "SET enable_mergejoin = off",
        ] {
            sqlx::query(setting)
                .execute(&mut *connection)
                .await
                .unwrap();
        }
        let plan =
            sqlx::query_scalar::<_, String>(&format!("EXPLAIN (COSTS OFF) {MARK_ORPHANS_QUERY}"))
                .bind(MARK_ORPHANS_BATCH)
                .fetch_all(&mut *connection)
                .await
                .unwrap()
                .join("\n");

        assert!(
            plan.contains("Index Cond: (content_id = (cb_1.id)::text)"),
            "document anti-join must support an index condition; plan was:\n{plan}"
        );
    }

    #[tokio::test]
    async fn mark_orphans_aborts_the_batch_when_a_candidate_is_locked() {
        let db = TestDatabase::new().await;
        let locked_id = "00000000000000000000000001";
        let available_id = "00000000000000000000000002";

        sqlx::query(
            r#"
            INSERT INTO content_blobs (id, orphaned_at)
            VALUES ($1, NULL), ($2, NULL)
            "#,
        )
        .bind(locked_id)
        .bind(available_id)
        .execute(&db.pool)
        .await
        .unwrap();

        let mut locking_transaction = db.pool.begin().await.unwrap();
        sqlx::query("SELECT id FROM content_blobs WHERE id = $1 FOR UPDATE")
            .bind(locked_id)
            .fetch_one(&mut *locking_transaction)
            .await
            .unwrap();

        let repo = ContentBlobRepository::new(&db.pool);
        let result = tokio::time::timeout(std::time::Duration::from_secs(5), repo.mark_orphans())
            .await
            .expect("lock timeout must bound the mark batch");
        assert!(result.is_err(), "a locked candidate must abort the batch");

        let still_unmarked: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM content_blobs WHERE orphaned_at IS NULL")
                .fetch_one(&db.pool)
                .await
                .unwrap();
        assert_eq!(
            still_unmarked, 2,
            "the failed batch must roll back atomically"
        );

        locking_transaction.rollback().await.unwrap();
        assert_eq!(repo.mark_orphans().await.unwrap(), 2);
    }

    #[tokio::test]
    async fn unmark_non_orphans_can_use_document_index_with_production_id_types() {
        let db = TestDatabase::new().await;

        sqlx::query(
            r#"
            INSERT INTO content_blobs (id, orphaned_at)
            SELECT
                lpad(value::text, 26, '0'),
                CASE WHEN value <= 2000 THEN CURRENT_TIMESTAMP ELSE NULL END
            FROM generate_series(1, 200000) AS value
            "#,
        )
        .execute(&db.pool)
        .await
        .unwrap();
        sqlx::query(
            r#"
            INSERT INTO documents (content_id)
            SELECT lpad(value::text, 26, '0')
            FROM generate_series(1, 200000) AS value
            "#,
        )
        .execute(&db.pool)
        .await
        .unwrap();
        sqlx::query("ANALYZE content_blobs")
            .execute(&db.pool)
            .await
            .unwrap();
        sqlx::query("ANALYZE documents")
            .execute(&db.pool)
            .await
            .unwrap();

        let mut connection = db.pool.acquire().await.unwrap();
        let plan = sqlx::query_scalar::<_, String>(&format!(
            "EXPLAIN (COSTS OFF) {UNMARK_NON_ORPHANS_QUERY}"
        ))
        .bind(UNMARK_NON_ORPHANS_SCAN_BATCH)
        .fetch_all(&mut *connection)
        .await
        .unwrap()
        .join("\n");
        let document_index_line = plan
            .lines()
            .position(|line| line.contains("idx_documents_content_id"))
            .unwrap_or_else(|| {
                panic!("planner must consider the document content-id index; plan was:\n{plan}")
            });
        let document_lookup = plan
            .lines()
            .skip(document_index_line)
            .take(3)
            .collect::<Vec<_>>()
            .join("\n");

        assert!(
            document_lookup.contains("Index Cond:"),
            "document reference lookup must use a correlated index condition; plan was:\n{plan}"
        );
    }

    #[tokio::test]
    async fn unmark_non_orphans_bounds_the_orphan_scan_before_reference_checks() {
        const SCAN_BATCH: i64 = 10_000;
        let db = TestDatabase::new().await;

        sqlx::query(
            r#"
            INSERT INTO content_blobs (id, orphaned_at)
            SELECT lpad(value::text, 26, '0'), CURRENT_TIMESTAMP - INTERVAL '2 days'
            FROM generate_series(1, $1) AS value
            "#,
        )
        .bind(SCAN_BATCH)
        .execute(&db.pool)
        .await
        .unwrap();
        sqlx::query(
            r#"
            INSERT INTO content_blobs (id, orphaned_at)
            VALUES (lpad($1::text, 26, '0'), CURRENT_TIMESTAMP)
            "#,
        )
        .bind(SCAN_BATCH + 1)
        .execute(&db.pool)
        .await
        .unwrap();
        sqlx::query("INSERT INTO documents (content_id) VALUES (lpad($1::text, 26, '0'))")
            .bind(SCAN_BATCH + 1)
            .execute(&db.pool)
            .await
            .unwrap();

        let repo = ContentBlobRepository::new(&db.pool);

        assert_eq!(
            repo.unmark_non_orphans().await.unwrap(),
            0,
            "reference checks must not scan past the bounded oldest-orphan batch"
        );
    }

    #[tokio::test]
    async fn unmark_non_orphans_updates_at_most_one_batch_and_eventually_converges() {
        const BATCH_SIZE: i64 = UNMARK_NON_ORPHANS_SCAN_BATCH;
        let db = TestDatabase::new().await;

        sqlx::query(
            r#"
            INSERT INTO content_blobs (id, orphaned_at)
            SELECT
                lpad(value::text, 26, '0'),
                CURRENT_TIMESTAMP + value * INTERVAL '1 millisecond'
            FROM generate_series(1, $1) AS value
            "#,
        )
        .bind(BATCH_SIZE + 4)
        .execute(&db.pool)
        .await
        .expect("seed marked content blobs");

        sqlx::query(
            r#"
            INSERT INTO documents (content_id)
            SELECT lpad(value::text, 26, '0')
            FROM generate_series(1, $1) AS value
            "#,
        )
        .bind(BATCH_SIZE + 1)
        .execute(&db.pool)
        .await
        .expect("seed document references");

        sqlx::query(
            r#"
            INSERT INTO connector_events_queue (payload, status)
            VALUES (jsonb_build_object('content_id', lpad($1::text, 26, '0')), 'processing')
            "#,
        )
        .bind(BATCH_SIZE + 2)
        .execute(&db.pool)
        .await
        .expect("seed queue reference");

        sqlx::query("INSERT INTO uploads (content_id) VALUES (lpad($1::text, 26, '0'))")
            .bind(BATCH_SIZE + 3)
            .execute(&db.pool)
            .await
            .expect("seed upload reference");

        let repo = ContentBlobRepository::new(&db.pool);

        assert_eq!(repo.unmark_non_orphans().await.unwrap(), BATCH_SIZE);
        assert_eq!(repo.unmark_non_orphans().await.unwrap(), 3);
        assert_eq!(repo.unmark_non_orphans().await.unwrap(), 0);

        let referenced_still_marked: i64 = sqlx::query_scalar(
            r#"
            SELECT COUNT(*)
            FROM content_blobs cb
            WHERE cb.orphaned_at IS NOT NULL
              AND (
                  EXISTS (SELECT 1 FROM documents d WHERE d.content_id = cb.id)
                  OR EXISTS (
                      SELECT 1 FROM connector_events_queue q
                      WHERE q.status IN ('pending', 'processing')
                        AND q.payload->>'content_id' = cb.id::text
                  )
                  OR EXISTS (SELECT 1 FROM uploads u WHERE u.content_id = cb.id)
              )
            "#,
        )
        .fetch_one(&db.pool)
        .await
        .unwrap();
        assert_eq!(referenced_still_marked, 0);

        let true_orphans_still_marked: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM content_blobs WHERE orphaned_at IS NOT NULL")
                .fetch_one(&db.pool)
                .await
                .unwrap();
        assert_eq!(true_orphans_still_marked, 1);
    }

    #[tokio::test]
    async fn fetch_expired_orphans_excludes_blobs_that_are_referenced() {
        let db = TestDatabase::new().await;
        let true_orphan_id = "00000000000000000000000001";
        let document_id = "00000000000000000000000002";
        let queue_id = "00000000000000000000000003";
        let upload_id = "00000000000000000000000004";

        sqlx::query(
            r#"
            INSERT INTO content_blobs (id, size_bytes, orphaned_at)
            VALUES
                ($1, 1, CURRENT_TIMESTAMP - INTERVAL '8 days'),
                ($2, 2, CURRENT_TIMESTAMP - INTERVAL '8 days'),
                ($3, 3, CURRENT_TIMESTAMP - INTERVAL '8 days'),
                ($4, 4, CURRENT_TIMESTAMP - INTERVAL '8 days')
            "#,
        )
        .bind(true_orphan_id)
        .bind(document_id)
        .bind(queue_id)
        .bind(upload_id)
        .execute(&db.pool)
        .await
        .unwrap();
        sqlx::query("INSERT INTO documents (content_id) VALUES ($1)")
            .bind(document_id)
            .execute(&db.pool)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO connector_events_queue (payload, status) VALUES (jsonb_build_object('content_id', $1::text), 'pending')",
        )
        .bind(queue_id)
        .execute(&db.pool)
        .await
        .unwrap();
        sqlx::query("INSERT INTO uploads (content_id) VALUES ($1)")
            .bind(upload_id)
            .execute(&db.pool)
            .await
            .unwrap();

        let repo = ContentBlobRepository::new(&db.pool);
        let expired = repo.fetch_expired_orphans(7, 10).await.unwrap();

        assert_eq!(expired.len(), 1);
        assert_eq!(expired[0].id.trim_end(), true_orphan_id);
    }

    #[tokio::test]
    async fn fetch_expired_orphans_aborts_when_the_table_is_locked() {
        let db = TestDatabase::new().await;
        let orphan_id = "00000000000000000000000001";

        sqlx::query(
            r#"
            INSERT INTO content_blobs (id, size_bytes, orphaned_at)
            VALUES ($1, 1, CURRENT_TIMESTAMP - INTERVAL '8 days')
            "#,
        )
        .bind(orphan_id)
        .execute(&db.pool)
        .await
        .unwrap();

        let mut locking_transaction = db.pool.begin().await.unwrap();
        sqlx::query("LOCK TABLE content_blobs IN ACCESS EXCLUSIVE MODE")
            .execute(&mut *locking_transaction)
            .await
            .unwrap();

        let repo = ContentBlobRepository::new(&db.pool);
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            repo.fetch_expired_orphans(7, 10),
        )
        .await
        .expect("lock timeout must bound the expired-orphan fetch");
        assert!(result.is_err(), "a locked table must abort the fetch");

        locking_transaction.rollback().await.unwrap();
        assert_eq!(repo.fetch_expired_orphans(7, 10).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn unmark_non_orphans_aborts_the_batch_when_a_candidate_is_locked() {
        let db = TestDatabase::new().await;
        let locked_id = "00000000000000000000000001";
        let available_id = "00000000000000000000000002";

        sqlx::query(
            r#"
            INSERT INTO content_blobs (id, orphaned_at)
            VALUES ($1, CURRENT_TIMESTAMP), ($2, CURRENT_TIMESTAMP)
            "#,
        )
        .bind(locked_id)
        .bind(available_id)
        .execute(&db.pool)
        .await
        .unwrap();
        sqlx::query("INSERT INTO documents (content_id) VALUES ($1), ($2)")
            .bind(locked_id)
            .bind(available_id)
            .execute(&db.pool)
            .await
            .unwrap();

        let mut locking_transaction = db.pool.begin().await.unwrap();
        sqlx::query("SELECT id FROM content_blobs WHERE id = $1 FOR UPDATE")
            .bind(locked_id)
            .fetch_one(&mut *locking_transaction)
            .await
            .unwrap();

        let repo = ContentBlobRepository::new(&db.pool);
        let result =
            tokio::time::timeout(std::time::Duration::from_secs(5), repo.unmark_non_orphans())
                .await
                .expect("lock timeout must bound the unmark batch");
        assert!(result.is_err(), "a locked candidate must abort the batch");

        let still_marked: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM content_blobs WHERE orphaned_at IS NOT NULL")
                .fetch_one(&db.pool)
                .await
                .unwrap();
        assert_eq!(
            still_marked, 2,
            "the failed batch must roll back atomically"
        );

        locking_transaction.rollback().await.unwrap();
        assert_eq!(repo.unmark_non_orphans().await.unwrap(), 2);
    }
}
