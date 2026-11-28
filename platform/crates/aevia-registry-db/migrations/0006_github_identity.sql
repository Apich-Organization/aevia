ALTER TABLE publishers
    ADD COLUMN github_id BIGINT,
    ADD COLUMN github_membership_generation BIGINT NOT NULL DEFAULT 0,
    ADD CONSTRAINT publishers_github_identity_check
        CHECK (github_id IS NULL OR (github_id > 0 AND kind IN ('github', 'github-org'))),
    ADD CONSTRAINT publishers_github_identity_key UNIQUE (kind, github_id);

-- A historical session or pending OAuth callback authenticated a login string.
DELETE FROM sessions s USING publishers p
    WHERE s.publisher_id = p.id AND p.kind = 'github';
DELETE FROM oauth_states WHERE provider = 'github';

-- Old grants did not record the organisation's provider identity.
DELETE FROM org_members;
ALTER TABLE org_members
    DROP COLUMN org,
    ADD COLUMN org_id BIGINT NOT NULL REFERENCES publishers (id) ON DELETE CASCADE,
    ADD PRIMARY KEY (org_id, publisher_id);

CREATE INDEX org_members_publisher ON org_members (publisher_id);
CREATE INDEX publishers_namespace_lookup ON publishers (lower(handle));
