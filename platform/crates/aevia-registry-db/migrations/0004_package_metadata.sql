-- Cargo-style `[package]` metadata columns on `packages`.
--
-- Extends `packages` so all metadata declared in `aevia.toml` (or supplied via
-- `aevia-pkg.toml` / publish overrides) is stored cleanly alongside each
-- package, and enforces the 5-item cap on `keywords` and `categories`.

ALTER TABLE packages
    ADD COLUMN edition TEXT,
    ADD COLUMN authors TEXT[] NOT NULL DEFAULT '{}',
    ADD COLUMN license_file TEXT,
    ADD COLUMN homepage TEXT,
    ADD COLUMN documentation TEXT,
    ADD COLUMN categories TEXT[] NOT NULL DEFAULT '{}';

ALTER TABLE packages
    ADD CONSTRAINT packages_keywords_max_five CHECK (cardinality(keywords) <= 5) NOT VALID,
    ADD CONSTRAINT packages_categories_max_five CHECK (cardinality(categories) <= 5) NOT VALID;
ALTER TABLE packages VALIDATE CONSTRAINT packages_keywords_max_five;
ALTER TABLE packages VALIDATE CONSTRAINT packages_categories_max_five;

CREATE INDEX packages_keywords_gin ON packages USING gin (keywords);
CREATE INDEX packages_categories_gin ON packages USING gin (categories);
