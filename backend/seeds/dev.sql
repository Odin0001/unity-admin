-- Dummy data for local development only. Safe to re-run.
--   psql "$DATABASE_URL" -f seeds/dev.sql
--
-- acme     PIN ACME42, growth, monthly, $1,250 prepaid call credit (API key from
--          your .env), reachable at http://127.0.0.1:8080
-- globex   PIN GLBX42, starter, annual, $200 credit    (API key: dev-globex-key)
-- initech  PIN NTCH77, enterprise, monthly, suspended  (API key: dev-initech-key)

INSERT INTO tenants (slug, name, pin, api_key_hash)
VALUES ('globex', 'Globex Recovery', 'GLBX42', encode(sha256('dev-globex-key'::bytea), 'hex')),
       ('initech', 'Initech Collections', 'NTCH77', encode(sha256('dev-initech-key'::bytea), 'hex'))
ON CONFLICT (slug) DO NOTHING;

UPDATE tenants SET status = 'suspended' WHERE slug = 'initech';

-- The tenant backend you run locally with `cargo run`.
UPDATE tenants SET api_url = 'http://127.0.0.1:8080' WHERE slug = 'acme';

-- Easy-to-type PINs instead of the random ones the migration handed out.
UPDATE tenants t SET pin = v.pin
FROM (VALUES ('acme', 'ACME42'), ('globex', 'GLBX42'), ('initech', 'NTCH77')) AS v (slug, pin)
WHERE t.slug = v.slug AND t.pin IS DISTINCT FROM v.pin;

INSERT INTO subscriptions (tenant_id, plan_id, billing_interval, current_period_start, current_period_end)
SELECT t.id, p.id, v.interval, current_date,
       (current_date + CASE WHEN v.interval = 'annual' THEN interval '1 year'
                            ELSE interval '1 month' END)::date
FROM (VALUES ('acme', 'growth', 'monthly'),
             ('globex', 'starter', 'annual'),
             ('initech', 'enterprise', 'monthly')) AS v (slug, plan, interval)
JOIN tenants t ON t.slug = v.slug
JOIN plans p ON p.code = v.plan
ON CONFLICT (tenant_id) DO NOTHING;

INSERT INTO credit_ledger (tenant_id, amount_cents, reason)
SELECT t.id, v.amount, 'dev top-up'
FROM (VALUES ('acme', 125000), ('globex', 20000)) AS v (slug, amount)
JOIN tenants t ON t.slug = v.slug
WHERE NOT EXISTS (
    SELECT 1 FROM credit_ledger c WHERE c.tenant_id = t.id AND c.reason = 'dev top-up'
);
