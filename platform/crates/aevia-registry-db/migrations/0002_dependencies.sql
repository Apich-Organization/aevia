-- Dependencies as rows, so "who depends on this package?" is an index lookup.
--
-- `versions.dependencies` stays the source of truth for what a version declared
-- (it is what the sparse index serves, byte for byte). This table is derived
-- from it, in the same transaction that inserts the version, and exists only to
-- answer reverse-dependency queries without scanning every JSON document.
--
-- `dep_key` is `owner/name` with the name folded to lower case, the same key
-- the resolver, the lock file and the index use.

CREATE TABLE version_dependencies (
    version_id BIGINT NOT NULL REFERENCES versions (id) ON DELETE CASCADE,
    dep_key TEXT NOT NULL,
    req TEXT NOT NULL,
    -- The import root, when it is not the default of the package.
    import TEXT,
    PRIMARY KEY (version_id, dep_key, req)
);

CREATE INDEX version_dependencies_target ON version_dependencies (dep_key);

-- Rows written after this migration are populated by the publish transaction.
-- Backfill the versions that already carry registry-shaped dependencies; entries
-- from before the registry understood requirements (`{"import", "path"}`) name
-- no package and are skipped.
INSERT INTO version_dependencies (version_id, dep_key, req, import)
SELECT DISTINCT v.id,
       lower(dependency ->> 'package'),
       dependency ->> 'req',
       dependency ->> 'import'
FROM versions v,
     LATERAL jsonb_array_elements(
         CASE WHEN jsonb_typeof(v.dependencies) = 'array' THEN v.dependencies ELSE '[]'::jsonb END
     ) AS dependency
WHERE dependency ? 'package' AND dependency ? 'req'
ON CONFLICT DO NOTHING;
