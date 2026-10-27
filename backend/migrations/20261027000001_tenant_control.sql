-- Admin control over tenant deployments: the full set of billing states,
-- where each deployment can be reached, and the latest heartbeat it sent.

ALTER TABLE tenants DROP CONSTRAINT tenants_status_check;
ALTER TABLE tenants ADD CONSTRAINT tenants_status_check
    CHECK (status IN ('active', 'past_due', 'suspended', 'cancelled'));

-- Base URL of the deployment, e.g. https://acme.example.com. NULL until
-- it's deployed; admin can't push to it before then.
ALTER TABLE tenants ADD COLUMN api_url text;

CREATE TABLE tenant_heartbeats (
    tenant_id        uuid        PRIMARY KEY REFERENCES tenants (id) ON DELETE CASCADE,
    version          text        NOT NULL,
    last_migration   bigint,
    active_employees bigint      NOT NULL,
    active_sessions  bigint      NOT NULL,
    database_bytes   bigint      NOT NULL,
    received_at      timestamptz NOT NULL DEFAULT now()
);
