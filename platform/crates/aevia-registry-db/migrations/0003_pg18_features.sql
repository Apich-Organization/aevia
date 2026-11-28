-- PostgreSQL 18 schema upgrades.
--
-- This registry targets PostgreSQL 18+, using native 18 features where they
-- tighten schema invariants or avoid redundant storage:
--
-- 1. Virtual generated columns (`GENERATED ALWAYS AS (...) VIRTUAL`, default in
--    PG 18) compute `packages.name_key` on read with zero heap footprint, paired
--    with expression indexes on `(lower(name))` that the PG 18 planner matches
--    automatically when filtering or sorting by `name_key`.
-- 2. Native time-ordered UUIDv7 (`uuidv7()`) gives `publish_log` entries a
--    globally unique, B-tree-friendly external correlation identifier.
-- 3. `NOT VALID` + `VALIDATE CONSTRAINT` adds domain checks without holding an
--    `ACCESS EXCLUSIVE` lock during validation scans on live tables.

ALTER TABLE packages DROP COLUMN name_key;
ALTER TABLE packages ADD COLUMN name_key TEXT GENERATED ALWAYS AS (lower(name)) VIRTUAL;

CREATE UNIQUE INDEX packages_owner_name_key ON packages (owner_id, (lower(name)));
CREATE INDEX packages_name_key ON packages ((lower(name)));

ALTER TABLE publish_log
    ADD COLUMN event_id UUID NOT NULL DEFAULT uuidv7(),
    ADD CONSTRAINT publish_log_event_id_key UNIQUE (event_id);

ALTER TABLE versions
    ADD CONSTRAINT versions_size_bytes_nonneg CHECK (size_bytes >= 0) NOT VALID,
    ADD CONSTRAINT versions_downloads_nonneg CHECK (downloads >= 0) NOT VALID,
    ADD CONSTRAINT versions_key_kind_check CHECK (key_kind IN ('openpgp', 'ssh')) NOT VALID,
    ADD CONSTRAINT versions_docs_status_check
        CHECK (docs_status IN ('pending', 'built', 'failed', 'skipped')) NOT VALID;
ALTER TABLE versions VALIDATE CONSTRAINT versions_size_bytes_nonneg;
ALTER TABLE versions VALIDATE CONSTRAINT versions_downloads_nonneg;
ALTER TABLE versions VALIDATE CONSTRAINT versions_key_kind_check;
ALTER TABLE versions VALIDATE CONSTRAINT versions_docs_status_check;

ALTER TABLE device_codes
    ADD CONSTRAINT device_codes_key_kind_check CHECK (key_kind IN ('openpgp', 'ssh')) NOT VALID;
ALTER TABLE device_codes VALIDATE CONSTRAINT device_codes_key_kind_check;
