-- What tenants can buy, what each one bought, and their prepaid call credit.
-- Everything here feeds the allowances document tenants enforce.

CREATE TABLE plans (
    id                          uuid        PRIMARY KEY DEFAULT gen_random_uuid(),
    code                        text        NOT NULL UNIQUE,
    name                        text        NOT NULL,
    modules                     text[]      NOT NULL CHECK (modules <@ ARRAY['sales', 'collection', 'hr']),
    seats                       integer     NOT NULL CHECK (seats > 0),
    leads                       integer     NOT NULL CHECK (leads >= 0),
    cases                       integer     NOT NULL CHECK (cases >= 0),
    esignatures_per_month       integer     NOT NULL CHECK (esignatures_per_month >= 0),
    call_billing                text        NOT NULL CHECK (call_billing IN ('prepaid', 'postpaid')),
    price_per_minute_cents      integer     NOT NULL CHECK (price_per_minute_cents >= 0),
    transaction_fee_bp          integer     NOT NULL CHECK (transaction_fee_bp >= 0),
    transaction_fee_fixed_cents integer     NOT NULL CHECK (transaction_fee_fixed_cents >= 0),
    monthly_price_cents         bigint      NOT NULL CHECK (monthly_price_cents >= 0),
    annual_price_cents          bigint      NOT NULL CHECK (annual_price_cents >= 0),
    created_at                  timestamptz NOT NULL DEFAULT now()
);

-- One current subscription per tenant. `version` goes up on every change so
-- tenants can tell a new allowances document from the one they cached.
CREATE TABLE subscriptions (
    tenant_id            uuid        PRIMARY KEY REFERENCES tenants (id) ON DELETE CASCADE,
    plan_id              uuid        NOT NULL REFERENCES plans (id),
    billing_interval     text        NOT NULL CHECK (billing_interval IN ('monthly', 'annual')),
    current_period_start date        NOT NULL,
    current_period_end   date        NOT NULL CHECK (current_period_end > current_period_start),
    version              bigint      NOT NULL DEFAULT 1,
    updated_at           timestamptz NOT NULL DEFAULT now()
);

-- Prepaid call credit: top-ups are positive, usage negative. The balance is
-- the sum.
CREATE TABLE credit_ledger (
    id           uuid        PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id    uuid        NOT NULL REFERENCES tenants (id) ON DELETE CASCADE,
    amount_cents bigint      NOT NULL,
    reason       text        NOT NULL,
    created_at   timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX credit_ledger_tenant_id_idx ON credit_ledger (tenant_id);

-- Placeholder catalogue. Prices and limits are dummy values until pricing
-- is decided; change them with a new migration, not by editing this one.
INSERT INTO plans (code, name, modules, seats, leads, cases, esignatures_per_month,
                   call_billing, price_per_minute_cents, transaction_fee_bp,
                   transaction_fee_fixed_cents, monthly_price_cents, annual_price_cents)
VALUES
    ('starter',    'Starter',    ARRAY['collection'],                 5,   1000,   500,   50, 'prepaid',  3, 200, 30,  9900,   99000),
    ('growth',     'Growth',     ARRAY['sales', 'collection'],       25,  10000,  5000,  300, 'prepaid',  2, 150, 25, 29900,  299000),
    ('enterprise', 'Enterprise', ARRAY['sales', 'collection', 'hr'], 100, 100000, 50000, 2000, 'postpaid', 1, 100, 20, 99900,  999000);
