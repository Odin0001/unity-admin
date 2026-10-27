-- Each customer tenant gets a PIN: six characters from an alphabet without
-- look-alikes (no 0/O, 1/I). Staff type it on the shared login page to find
-- their company. It's a locator, not a secret.

ALTER TABLE tenants ADD COLUMN pin text UNIQUE CHECK (pin ~ '^[2-9A-HJ-NP-Z]{6}$');

DO $$
DECLARE
    tenant record;
BEGIN
    FOR tenant IN SELECT id FROM tenants WHERE NOT is_master LOOP
        LOOP
            BEGIN
                UPDATE tenants
                SET pin = (SELECT string_agg(substr('23456789ABCDEFGHJKLMNPQRSTUVWXYZ',
                                                    1 + floor(random() * 32)::int, 1), '')
                           FROM generate_series(1, 6))
                WHERE id = tenant.id;
                EXIT;
            EXCEPTION WHEN unique_violation THEN
                -- taken already, roll again
            END;
        END LOOP;
    END LOOP;
END $$;

ALTER TABLE tenants ADD CONSTRAINT tenants_pin_required CHECK (is_master OR pin IS NOT NULL);
