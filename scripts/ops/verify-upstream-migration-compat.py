#!/usr/bin/env python3
"""Verify fork migration compatibility using metadata and an isolated test DB.

Inputs: production schema-only pg_dump (no owners/ACLs) and read-only query output:
  SELECT version, success, encode(checksum, 'hex')
  FROM _sqlx_migrations ORDER BY version;

No production connection is accepted. Docker creates a disposable database with
no network or exposed ports. The migrator joins only that container's network
namespace. Production migration bookkeeping is never edited.
"""

import argparse
import hashlib
import pathlib
import re
import subprocess
import time
import uuid


ROOT = pathlib.Path(__file__).resolve().parents[2]


def run(*args, stdin=None, check=True):
    result = subprocess.run(args, input=stdin, text=True, capture_output=True)
    if check and result.returncode:
        raise RuntimeError(f"{args[0]} failed ({result.returncode}):\n{result.stdout}\n{result.stderr}")
    return result


def migrations():
    files = {}
    for path in sorted((ROOT / "services/migrations").glob("*.sql")):
        match = re.fullmatch(r"(\d+)_(.+)\.sql", path.name)
        if not match:
            raise ValueError(f"Unrecognized migration: {path.name}")
        version = int(match[1])
        if version in files:
            raise ValueError(f"Duplicate migration version {version}")
        files[version] = path
    return files


FIXTURES = """
INSERT INTO users (id, email, password_hash) VALUES
 ('00000000000000000000000001', 'migration-fixture@example.invalid', 'fixture');
INSERT INTO sources (id, name, source_type, created_by) VALUES
 ('00000000000000000000000002', 'migration fixture', 'telegram', '00000000000000000000000001');
INSERT INTO service_credentials (id, source_id, provider, auth_type, credentials) VALUES
 ('00000000000000000000000003', '00000000000000000000000002', 'telegram', 'api_key',
 '{"encrypted_data": {}, "version": 1}');
INSERT INTO people (id, email, display_name, department, metadata) VALUES
 ('00000000000000000000000010', 'Person@example.invalid', 'Longest Display Name', NULL, '{"keeper": true}'),
 ('00000000000000000000000011', 'person@example.invalid', 'Short', 'Engineering', '{"loser": true}'),
 ('00000000000000000000000012', 'report@example.invalid', 'Report', NULL, '{}');
UPDATE people SET manager_id = '00000000000000000000000011'
 WHERE id = '00000000000000000000000012';
"""


ASSERTIONS = """
DO $$ BEGIN
 IF NOT EXISTS (SELECT 1 FROM sources WHERE source_type = 'telegram' AND integration_type = 'connector')
    OR NOT EXISTS (SELECT 1 FROM service_credentials WHERE provider = 'telegram') THEN
   RAISE EXCEPTION 'Telegram fixture was lost';
 END IF;
 IF (SELECT count(*) FROM people WHERE lower(email) = 'person@example.invalid') <> 1
    OR NOT EXISTS (SELECT 1 FROM people WHERE id = '00000000000000000000000010'
       AND email = 'person@example.invalid' AND department = 'Engineering'
       AND metadata @> '{"keeper": true, "loser": true}')
    OR NOT EXISTS (SELECT 1 FROM people WHERE id = '00000000000000000000000012'
       AND manager_id = '00000000000000000000000010') THEN
   RAISE EXCEPTION 'People deduplication lost fields or manager relationship';
 END IF;
 IF to_regclass('public.skills') IS NULL OR to_regclass('public.tasks') IS NULL
    OR to_regclass('public.idx_sync_runs_source_started') IS NULL THEN
   RAISE EXCEPTION 'Expected upstream schema is missing';
 END IF;
END $$;
INSERT INTO sources (id, name, source_type, created_by) VALUES
 ('00000000000000000000000004', 'new telegram', 'telegram', '00000000000000000000000001'),
 ('00000000000000000000000005', 'windshift', 'windshift', '00000000000000000000000001');
INSERT INTO sources (id, name, source_type, integration_type, created_by) VALUES
 ('00000000000000000000000006', 'remote', 'fixture-app', 'remote_mcp', '00000000000000000000000001');
INSERT INTO service_credentials (id, source_id, provider, auth_type, credentials) VALUES
 ('00000000000000000000000007', '00000000000000000000000004', 'telegram', 'api_key', '{"encrypted_data": {}, "version": 1}'),
 ('00000000000000000000000008', '00000000000000000000000005', 'windshift', 'api_key', '{"encrypted_data": {}, "version": 1}'),
 ('00000000000000000000000009', '00000000000000000000000006', 'remote_mcp', 'api_key', '{"encrypted_data": {}, "version": 1}');
DO $$ BEGIN
 BEGIN
   UPDATE sources SET source_type = 'unknown-native' WHERE id = '00000000000000000000000004';
   RAISE EXCEPTION 'Unknown native source was accepted';
 EXCEPTION WHEN check_violation THEN NULL;
 END;
 BEGIN
   UPDATE sources SET source_type = 'Invalid Slug' WHERE id = '00000000000000000000000006';
   RAISE EXCEPTION 'Invalid remote slug was accepted';
 EXCEPTION WHEN check_violation THEN NULL;
 END;
 BEGIN
   UPDATE service_credentials SET provider = 'unknown' WHERE id = '00000000000000000000000007';
   RAISE EXCEPTION 'Unknown credential provider was accepted';
 EXCEPTION WHEN check_violation THEN NULL;
 END;
END $$;
"""


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--applied-checksums", type=pathlib.Path, required=True)
    parser.add_argument("--schema-dump", type=pathlib.Path)
    parser.add_argument("--migrator-image", help="Existing image containing sqlx 0.8.6; never pulled")
    parser.add_argument("--postgres-image", default="paradedb/paradedb:0.24.0-pg17")
    parser.add_argument("--static-only", action="store_true")
    args = parser.parse_args()
    files = migrations()
    applied = {}
    for line in args.applied_checksums.read_text().splitlines():
        if not line.strip():
            continue
        version, success, checksum = line.strip().split("|")
        version = int(version)
        if version in applied or success != "t" or not re.fullmatch(r"[0-9a-f]{96}", checksum):
            raise ValueError(f"Invalid/unsuccessful applied metadata for {version}")
        if version not in files or hashlib.sha384(files[version].read_bytes()).hexdigest() != checksum:
            raise ValueError(f"Applied migration {version} is missing or modified")
        applied[version] = checksum
    if set(applied) != set(range(1, 106)):
        raise ValueError("This gate requires the captured production baseline 001–105")
    if set(files) != set(range(1, 114)):
        raise ValueError("Expected unique contiguous fork migrations 001–113")
    print("PASS: all 105 applied SHA-384 checksums preserved; versions 001–113 unique", flush=True)
    if args.static_only:
        return
    if not args.schema_dump or not args.migrator_image:
        parser.error("Database gate requires --schema-dump and --migrator-image")
    schema = args.schema_dump.read_text()
    name = "omni-migration-gate-" + uuid.uuid4().hex[:12]

    def psql(sql):
        return run("docker", "exec", "-i", name, "psql", "-X", "-U", "gate", "-d", "gate",
                   "-v", "ON_ERROR_STOP=1", "-At", stdin=sql).stdout

    created = False
    try:
        run("docker", "run", "--pull=never", "--detach", "--name", name,
            "--network", "none", "--cpus", "1", "--memory", "2g", "--shm-size", "512m",
            "-e", "POSTGRES_USER=gate", "-e", "POSTGRES_DB=bootstrap",
            "-e", "POSTGRES_HOST_AUTH_METHOD=trust", args.postgres_image,
            "postgres", "-c", "shared_buffers=256MB", "-c", "max_parallel_workers_per_gather=0",
            "-c", "max_parallel_maintenance_workers=0", "-c", "maintenance_work_mem=128MB")
        created = True
        for _ in range(60):
            # The image's temporary initialization server only listens on the
            # Unix socket; require TCP so restore cannot race its shutdown.
            ready = run("docker", "exec", name, "pg_isready", "-h", "127.0.0.1", "-U", "gate", "-d", "bootstrap", check=False)
            if ready.returncode == 0:
                break
            time.sleep(1)
        else:
            raise RuntimeError("Disposable database did not become ready")
        run("docker", "exec", name, "createdb", "-U", "gate", "--template=template0", "gate")
        psql(schema)
        rows = []
        for version, checksum in sorted(applied.items()):
            description = files[version].stem.split("_", 1)[1].replace("_", " ").replace("'", "''")
            rows.append(f"({version}, '{description}', now(), true, decode('{checksum}', 'hex'), 0)")
        psql("TRUNCATE public._sqlx_migrations; INSERT INTO public._sqlx_migrations "
             "(version, description, installed_on, success, checksum, execution_time) VALUES "
             + ",".join(rows) + ";\n" + FIXTURES)
        before = psql("SELECT version, encode(checksum, 'hex') FROM _sqlx_migrations ORDER BY version;")
        migrate = ("docker", "run", "--rm", "--pull=never", "--network", "container:" + name,
                   "--cpus", "1", "--memory", "512m", "--entrypoint", "sqlx",
                   "-e", "DATABASE_URL=postgresql://gate@127.0.0.1:5432/gate",
                   "--mount", f"type=bind,src={ROOT / 'services/migrations'},dst=/migrations,readonly",
                   args.migrator_image, "migrate", "run", "--source", "/migrations")
        run(*migrate)
        psql(ASSERTIONS)
        after = psql("SELECT version, encode(checksum, 'hex') FROM _sqlx_migrations WHERE version <= 105 ORDER BY version;")
        if before != after:
            raise RuntimeError("Applied migration metadata changed during upgrade")
        observed = psql("SELECT version, success, encode(checksum, 'hex') FROM _sqlx_migrations ORDER BY version;")
        expected = "".join(f"{version}|t|{hashlib.sha384(path.read_bytes()).hexdigest()}\n"
                           for version, path in sorted(files.items()))
        if observed != expected:
            raise RuntimeError("Final SQLx migration history does not match candidate files")
        run(*migrate)
        print("PASS: isolated SQLx upgrade 105→113; Telegram and people fixtures; strict constraints; rerun", flush=True)
    except Exception:
        if created:
            logs = run("docker", "logs", "--tail", "40", name, check=False)
            print(logs.stdout + logs.stderr, flush=True)
        raise
    finally:
        if created:
            run("docker", "rm", "--force", "--volumes", name)


if __name__ == "__main__":
    main()
