-- Control-plane schema: tenants, the users that sign in to them, and the
-- opaque-token sessions those users hold.

CREATE TABLE tenants (
    id           uuid        PRIMARY KEY DEFAULT gen_random_uuid(),
    slug         text        NOT NULL UNIQUE CHECK (slug ~ '^[a-z0-9][a-z0-9-]{1,62}$'),
    name         text        NOT NULL,
    status       text        NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'suspended')),
    -- The platform owner's own tenant. Its users run this admin console.
    is_master    boolean     NOT NULL DEFAULT false,
    -- SHA-256 of the key a tenant deployment uses to call this service.
    -- NULL for the master tenant, which never calls itself.
    api_key_hash text        UNIQUE,
    last_seen_at timestamptz,
    created_at   timestamptz NOT NULL DEFAULT now(),
    updated_at   timestamptz NOT NULL DEFAULT now()
);

CREATE UNIQUE INDEX tenants_single_master ON tenants (is_master) WHERE is_master;

INSERT INTO tenants (slug, name, is_master) VALUES ('platform', 'Platform Admin', true);

CREATE TABLE users (
    id            uuid        PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id     uuid        NOT NULL REFERENCES tenants (id),
    email         text        NOT NULL UNIQUE CHECK (email = lower(email)),
    password_hash text        NOT NULL,
    full_name     text        NOT NULL,
    is_active     boolean     NOT NULL DEFAULT true,
    created_at    timestamptz NOT NULL DEFAULT now(),
    updated_at    timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX users_tenant_id_idx ON users (tenant_id);

-- The raw token only ever lives on the client; we keep its SHA-256 so a
-- leaked dump of this table can't be replayed as live sessions.
CREATE TABLE auth_sessions (
    token_hash       text        PRIMARY KEY,
    user_id          uuid        NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    user_agent       text,
    created_at       timestamptz NOT NULL DEFAULT now(),
    last_activity_at timestamptz NOT NULL DEFAULT now(),
    expires_at       timestamptz NOT NULL
);

CREATE INDEX auth_sessions_user_id_idx ON auth_sessions (user_id);
CREATE INDEX auth_sessions_expires_at_idx ON auth_sessions (expires_at);
