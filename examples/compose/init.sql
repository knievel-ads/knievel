-- One-time DB provisioning for the compose stack.
--
-- Postgres runs files under /docker-entrypoint-initdb.d/ exactly
-- once when the data directory is first initialized. We use that
-- hook for the operator-equivalent steps from
-- `MIGRATION_RX.md` "One-time provisioning":
--
--   1. pgcrypto for `gen_random_uuid()`.
--   2. The `knievel` schema.
--   3. Default search_path on the application role so unqualified
--      DDL in migrations lands in `knievel`, not `public`.
--
-- The application binary's `database.auto_migrate: true` then runs
-- the rest. Migrations are idempotent — calling them again on a
-- pre-provisioned cluster is a no-op.

CREATE EXTENSION IF NOT EXISTS pgcrypto;
-- Postgres 16.13 rejects self-NOSUPERUSER even from a verified
-- superuser (CLAUDE.md gotcha #17), so we cannot bootstrap as
-- `knievel_app` and then drop superuser. compose.yaml bootstraps
-- as `postgres` instead; init.sql CREATEs the app role
-- NOSUPERUSER from the start. Same pattern as ci.yml's
-- db-integ / api-contract / acceptance jobs. CREATEDB is kept so
-- testlib's ephemeral fixtures can spin up scratch DBs.
CREATE ROLE knievel_app WITH NOSUPERUSER CREATEDB LOGIN PASSWORD 'dev';
GRANT ALL PRIVILEGES ON DATABASE knievel TO knievel_app;
GRANT ALL PRIVILEGES ON SCHEMA public TO knievel_app;
CREATE SCHEMA IF NOT EXISTS knievel AUTHORIZATION knievel_app;
ALTER ROLE knievel_app SET search_path = knievel, public;

-- Background-loader role. BYPASSRLS so the in-process snapshot loader
-- (and the rollup) can read across tenants — the request path keeps
-- strict per-tenant RLS and never assumes this role. NOLOGIN: the app
-- role `SET LOCAL ROLE knievel_loader` for its background reads. Only a
-- superuser can grant BYPASSRLS, so this runs at provisioning time, not
-- via the app's auto_migrate.
DO $$ BEGIN
  CREATE ROLE knievel_loader NOLOGIN BYPASSRLS;
EXCEPTION WHEN duplicate_object THEN NULL; END $$;
GRANT knievel_loader TO knievel_app;
GRANT USAGE ON SCHEMA knievel TO knievel_loader;
-- Tables/sequences are created later by auto_migrate (as knievel_app);
-- default privileges auto-grant the loader these as each table lands.
--
-- SELECT covers the snapshot loader. INSERT/UPDATE cover the hourly
-- rollup, which (also a `SET LOCAL ROLE knievel_loader` path)
-- INSERT/UPDATEs `events_rollup` and UPDATEs `events_rollup_watermark`.
-- Because init.sql runs before the tables exist, we grant via default
-- privileges rather than per-table. This is broader than production —
-- the loader only writes the two rollup tables there (see
-- MIGRATION_RX.md "One-time provisioning") — but it's a dev-stack
-- convenience: the capability is unused on every other table since the
-- only roles that `SET ROLE knievel_loader` are the read-only snapshot
-- loader and the rollup.
ALTER DEFAULT PRIVILEGES FOR ROLE knievel_app IN SCHEMA knievel
  GRANT SELECT, INSERT, UPDATE ON TABLES TO knievel_loader;
ALTER DEFAULT PRIVILEGES FOR ROLE knievel_app IN SCHEMA knievel
  GRANT SELECT ON SEQUENCES TO knievel_loader;
