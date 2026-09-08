#!/usr/bin/env python3
"""Check real storage ID queries with TEXT parameters in a disposable test DB.

Pass a psql command, e.g. docker exec -i <test-container> psql -U omni -d fixture.
Only temporary fixture tables are written. DELETE statements are EXPLAINed,
never executed; equivalent SELECT predicates verify exact lookup semantics.
"""
from pathlib import Path
import re
import subprocess
import sys

def literal(value):
    return "'" + value.replace("'", "''") + "'"


root = Path(__file__).resolve().parents[2]
queries = []
for relative in ("shared/src/content_storage.rs", "shared/src/storage/postgres.rs", "shared/src/storage/s3.rs"):
    queries.extend(re.findall(r'"((?:SELECT|DELETE)[^"\n]*FROM content_blobs WHERE id[^"\n]*)"',
                              (root / relative).read_text()))
assert len(queries) == 16, f"Expected 13 scalar and 3 batch ID paths, found {len(queries)}"
sql = ["""
BEGIN;
SET LOCAL statement_timeout='30s';
SET LOCAL plan_cache_mode=force_generic_plan;
CREATE TEMP TABLE content_blobs (
 id char(26) PRIMARY KEY, content bytea, content_type text, size_bytes bigint,
 sha256_hash char(64), storage_key text, storage_backend text
) ON COMMIT DROP;
INSERT INTO content_blobs SELECT lpad(n::text,26,'0'),decode('7b7d','hex'),
 'text/plain',2,md5(n::text)||md5(n::text),'fixture','s3' FROM generate_series(1,20000) n;
ANALYZE content_blobs;
"""]
for number, query in enumerate(queries):
    batch = "{}" in query
    if batch:
        assert query.count("{}") == 2
        query = query.replace("{}", "$1::bpchar,$2::bpchar", 1).replace("{}", "$1,$2", 1)
    types = "text,text" if batch else "text"
    predicate = query.split(" WHERE ", 1)[1]
    sql.append(f"PREPARE actual_{number}({types}) AS {query};")
    sql.append(f"PREPARE semantics_{number}({types}) AS SELECT id FROM content_blobs WHERE {predicate};")
    args = "lpad('12345',26,'0'),lpad('12346',26,'0')" if batch else "lpad('12345',26,'0')"
    bad_args = "lpad('12345',26,'0')||'suffix',lpad('12346',26,'0')||' '" if batch else "lpad('12345',26,'0')||'suffix'"
    space_args = "lpad('12345',26,'0')||' ',lpad('12346',26,'0')||'suffix'" if batch else "lpad('12345',26,'0')||' '"
    sql.append(f"""
DO $check$ DECLARE plan json; found text; BEGIN
 EXECUTE {literal(f'EXPLAIN (FORMAT JSON) EXECUTE actual_{number}({args})')} INTO plan;
 IF plan::text NOT LIKE '%Index Scan%' OR plan::text LIKE '%Seq Scan%' THEN
  RAISE EXCEPTION 'ID lookup {number} bypasses native index: %',plan;
 END IF;
 EXECUTE {literal(f'EXECUTE semantics_{number}({args})')} INTO found;
 IF found IS NULL THEN RAISE EXCEPTION 'ID lookup {number} lost valid row'; END IF;
 EXECUTE {literal(f'EXECUTE semantics_{number}({bad_args})')} INTO found;
 IF found IS NOT NULL THEN RAISE EXCEPTION 'ID lookup {number} truncated oversized input'; END IF;
 EXECUTE {literal(f'EXECUTE semantics_{number}({space_args})')} INTO found;
 IF found IS NOT NULL THEN RAISE EXCEPTION 'ID lookup {number} changed trailing-space semantics'; END IF;
END $check$;
DEALLOCATE actual_{number}; DEALLOCATE semantics_{number};
""")
sql.append("ROLLBACK;")
subprocess.run((sys.argv[1:] or ["psql"]) + ["-X", "-v", "ON_ERROR_STOP=1"],
               input="\n".join(sql), text=True, check=True)
print("CONTENT_ID_INDEX_OK: all 16 scalar/batch/read/delete-plan paths use the native index")
