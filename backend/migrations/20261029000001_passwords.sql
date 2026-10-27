-- One-time links for forgotten passwords and for new super users setting
-- their first one. A user who was invited but hasn't set a password yet
-- has no hash and can't sign in.

ALTER TABLE users ALTER COLUMN password_hash DROP NOT NULL;

CREATE TABLE password_resets (
    -- SHA-256 of the token in the link; the link itself is never stored.
    token_hash text        PRIMARY KEY,
    user_id    uuid        NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    expires_at timestamptz NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX password_resets_user_id ON password_resets (user_id);
