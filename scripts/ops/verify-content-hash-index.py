#!/usr/bin/env python3
"""Test the real storage SQL with TEXT bindings against a disposable temp table.

Usage: python3 scripts/ops/verify-content-hash-index.py [psql command...]
Example: ... docker exec -i omni-postgres psql -U omni -d omni
Only session-local temporary objects are written; the transaction rolls back.
"""
from pathlib import Path
import re
import subprocess
import sys

root = Path(__file__).resolve().parents[2]
queries = []
for relative in (
    "shared/src/content_storage.rs",
    "shared/src/storage/postgres.rs",
    "shared/src/storage/s3.rs",
):
    queries.extend(re.findall(
        r'"(SELECT id FROM content_blobs WHERE sha256_hash[^"\n]+)"',
        (root / relative).read_text(),
    ))
assert len(queries) == 6, f"Expected all six storage lookup paths, found {len(queries)}"

sql = ["""
BEGIN;
SET LOCAL statement_timeout = '30s';
SET LOCAL plan_cache_mode = force_generic_plan;
CREATE TEMP TABLE content_blobs (
  id text PRIMARY KEY, sha256_hash char(64) NOT NULL, storage_backend text NOT NULL
) ON COMMIT DROP;
INSERT INTO content_blobs
SELECT n::text, md5(n::text) || md5(n::text), 's3'
FROM generate_series(1, 20000) n;
CREATE INDEX ON content_blobs (sha256_hash);
ANALYZE content_blobs;
"""]
for number, query in enumerate(queries):
    sql.append(f"PREPARE hash_case_{number}(text) AS {query};")
    sql.append(f"""
DO $check$
DECLARE
  plan json;
  result text;
  hash text := md5('12345') || md5('12345');
BEGIN
  EXECUTE format('EXPLAIN (FORMAT JSON) EXECUTE hash_case_{number}(%L)', hash) INTO plan;
  IF plan::text NOT LIKE '%Index Scan%' OR plan::text LIKE '%Seq Scan%' THEN
    RAISE EXCEPTION 'lookup {number} does not use the hash index: %', plan;
  END IF;
  EXECUTE format('EXECUTE hash_case_{number}(%L)', hash) INTO result;
  IF result IS DISTINCT FROM '12345' THEN
    RAISE EXCEPTION 'lookup {number} failed to find the existing hash';
  END IF;
  EXECUTE format('EXECUTE hash_case_{number}(%L)', hash || 'suffix') INTO result;
  IF result IS NOT NULL THEN
    RAISE EXCEPTION 'lookup {number} truncated a longer input';
  END IF;
  EXECUTE format('EXECUTE hash_case_{number}(%L)', hash || ' ') INTO result;
  IF result IS NOT NULL THEN
    RAISE EXCEPTION 'lookup {number} changed exact TEXT comparison semantics';
  END IF;
END $check$;
DEALLOCATE hash_case_{number};
""")
sql.append("ROLLBACK;")
subprocess.run(
    (sys.argv[1:] or ["psql"]) + ["-X", "-v", "ON_ERROR_STOP=1"],
    input="\n".join(sql), text=True, check=True,
)
print("CONTENT_HASH_INDEX_OK: all six TEXT-bound lookups use the existing index")
