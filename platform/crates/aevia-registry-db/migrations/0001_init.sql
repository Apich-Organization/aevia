-- The Aevia registry schema.
--
-- Three rules shape it:
--
-- 1. **Identity comes from outside.** A publisher is a GitHub account, an
--    ORCID iD or a GitHub organisation, never a password this service stores.
--    The `publishers` table is a cache of what those providers told us.
-- 2. **Versions are immutable.** There is no `UPDATE versions SET checksum`.
--    The only mutable columns are `yanked`, the documentation state and the
--    download counter; everything a signature covers is written once.
-- 3. **The log is append-only.** `publish_log` is a hash chain: each row hashes
--    its own payload together with the previous row's hash, so a silent rewrite
--    of history breaks every later hash.

CREATE TABLE publishers (
    id BIGSERIAL PRIMARY KEY,
    -- `github`, `orcid` or `github-org`. The last one is created only after a
    -- GitHub App installation proves the organisation exists and the user is a
    -- member of it.
    kind TEXT NOT NULL CHECK (kind IN ('github', 'orcid', 'github-org')),
    -- The GitHub login, the ORCID iD or the organisation login, normalised by
    -- `aevia_registry_core::name::normalize_owner`.
    handle TEXT NOT NULL,
    display_name TEXT,
    avatar_url TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (kind, handle)
);

CREATE TABLE signing_keys (
    id BIGSERIAL PRIMARY KEY,
    publisher_id BIGINT NOT NULL REFERENCES publishers (id) ON DELETE CASCADE,
    kind TEXT NOT NULL CHECK (kind IN ('openpgp', 'ssh')),
    -- Upper-case hex without spaces for OpenPGP, `SHA256:…` for SSH.
    fingerprint TEXT NOT NULL,
    -- The ASCII-armoured public key. Kept so a client can fetch it to verify a
    -- download, and so a publish can be re-verified years later.
    public_key TEXT NOT NULL,
    label TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    revoked_at TIMESTAMPTZ,
    UNIQUE (kind, fingerprint)
);

CREATE INDEX signing_keys_publisher ON signing_keys (publisher_id);

CREATE TABLE packages (
    id BIGSERIAL PRIMARY KEY,
    owner_id BIGINT NOT NULL REFERENCES publishers (id),
    -- As the author spells it.
    name TEXT NOT NULL,
    -- `lower(name)`: the uniqueness key, so `Aevia` and `aevia` are one package.
    name_key TEXT NOT NULL,
    description TEXT,
    license TEXT,
    repository TEXT,
    keywords TEXT[] NOT NULL DEFAULT '{}',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (owner_id, name_key)
);

CREATE INDEX packages_name_key ON packages (name_key);

CREATE TABLE versions (
    id BIGSERIAL PRIMARY KEY,
    package_id BIGINT NOT NULL REFERENCES packages (id) ON DELETE CASCADE,
    version TEXT NOT NULL,
    -- `sha256:<hex>` of the archive.
    checksum TEXT NOT NULL,
    size_bytes BIGINT NOT NULL,
    -- The parsed `aevia.toml`, so the page can show dependencies without
    -- unpacking an archive.
    manifest JSONB NOT NULL,
    dependencies JSONB NOT NULL DEFAULT '[]',
    readme TEXT,
    -- Who signed it, with the key as it was at publish time. The foreign keys
    -- are informational: a key may later be revoked, and the version must keep
    -- saying who signed it.
    published_by BIGINT REFERENCES publishers (id),
    key_kind TEXT NOT NULL,
    fingerprint TEXT NOT NULL,
    signature TEXT NOT NULL,
    -- The exact bytes that were signed. Stored verbatim so anyone can re-check
    -- the signature without reconstructing the statement.
    statement TEXT NOT NULL,
    yanked BOOLEAN NOT NULL DEFAULT false,
    yanked_at TIMESTAMPTZ,
    -- `pending`, `built`, `failed` or `skipped`.
    docs_status TEXT NOT NULL DEFAULT 'pending',
    docs_error TEXT,
    docs_generated_at TIMESTAMPTZ,
    downloads BIGINT NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (package_id, version)
);

CREATE INDEX versions_package ON versions (package_id, created_at DESC);

CREATE TABLE docs_pages (
    version_id BIGINT NOT NULL REFERENCES versions (id) ON DELETE CASCADE,
    -- The module path, or '' for the landing page.
    path TEXT NOT NULL,
    title TEXT NOT NULL,
    html TEXT NOT NULL,
    decls INT NOT NULL DEFAULT 0,
    undocumented INT NOT NULL DEFAULT 0,
    source_file TEXT NOT NULL DEFAULT '',
    PRIMARY KEY (version_id, path)
);

-- A nonce the server issued for one statement. A row exists only between the
-- challenge and the publish, and `consumed_at` makes it single-use: a replayed
-- signature finds a consumed row and is refused.
CREATE TABLE challenges (
    nonce TEXT PRIMARY KEY,
    kind TEXT NOT NULL CHECK (kind IN ('publish', 'yank')),
    statement TEXT NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL,
    consumed_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX challenges_expiry ON challenges (expires_at);

-- Device authorisation: the client presents a public key, the human approves it
-- in a browser, and the key becomes bound to their account. This replaces
-- `cargo login` entirely.
CREATE TABLE device_codes (
    device_code TEXT PRIMARY KEY,
    user_code TEXT NOT NULL UNIQUE,
    key_kind TEXT NOT NULL,
    public_key TEXT NOT NULL,
    fingerprint TEXT NOT NULL,
    label TEXT,
    status TEXT NOT NULL DEFAULT 'pending'
        CHECK (status IN ('pending', 'approved', 'denied', 'expired')),
    publisher_id BIGINT REFERENCES publishers (id),
    expires_at TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE sessions (
    -- 32 random bytes, hex. Stored as the key itself: the value is a bearer
    -- credential, so it is compared in constant time by the query planner
    -- looking up a unique index, and it expires quickly.
    id TEXT PRIMARY KEY,
    publisher_id BIGINT NOT NULL REFERENCES publishers (id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at TIMESTAMPTZ NOT NULL,
    user_agent TEXT
);

CREATE INDEX sessions_expiry ON sessions (expires_at);

-- The `state` parameter of an OAuth round trip, so a callback cannot be forged.
CREATE TABLE oauth_states (
    state TEXT PRIMARY KEY,
    provider TEXT NOT NULL,
    -- Where to send the browser once the round trip succeeds, e.g. the
    -- activation page a device flow started from.
    redirect TEXT NOT NULL DEFAULT '/account',
    expires_at TIMESTAMPTZ NOT NULL
);

-- Membership of a GitHub organisation, confirmed through the App installation.
-- Only members may publish under the organisation's scope.
CREATE TABLE org_members (
    org TEXT NOT NULL,
    publisher_id BIGINT NOT NULL REFERENCES publishers (id) ON DELETE CASCADE,
    role TEXT NOT NULL DEFAULT 'member',
    checked_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (org, publisher_id)
);

CREATE TABLE publish_log (
    seq BIGSERIAL PRIMARY KEY,
    kind TEXT NOT NULL,
    summary TEXT NOT NULL,
    payload JSONB NOT NULL,
    prev_hash TEXT NOT NULL,
    entry_hash TEXT NOT NULL UNIQUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
