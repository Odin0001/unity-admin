# Platform architecture: admin ↔ tenants

This file is identical in `rust-unity-admin` and `rust-unity-tenants`. It is the contract between the two. If you change a contract here, change it in both repos and both codebases in the same piece of work.

**Rule of thumb:** admin is the source of truth for what a tenant is *allowed*. The tenant is the source of truth for what *happened*. The tenant enforces locally from a cached copy of its allowances. It never asks admin per request, so admin downtime never takes tenants down.

## Who owns what

| Concern | Admin (control plane) | Tenant (one instance per customer) |
|---|---|---|
| Plans, prices, Stripe | owns | never sees |
| Allowances (modules, limits, status, PIN) | publishes | caches + enforces |
| Employees, permissions, sessions | read/control via tenant API | owns |
| Leads, cases, calls, debtor payments | aggregates for billing | owns, records usage |
| Usage events | ingests, bills | writes to outbox, ships |
| Health | stores heartbeats, alerts | sends heartbeat |
| Databases | provisions on the shared cluster | owns its own; admin never connects |

## 1. Tenant lifecycle and provisioning (admin)

Status: `pending_payment → provisioning → active → past_due → suspended → cancelled → removed`. Built so far: `active`, `past_due`, `suspended` and `cancelled`, set by hand through `PUT /api/v1/tenants/{id}/status`. Each change bumps the allowances `version`. The rest come with provisioning and Stripe.

A new customer subscribes through Stripe Checkout. The verified `checkout.session.completed` webhook starts a **provisioning job**: a row with the current step, attempts and last error. A worker runs the steps, and every step is idempotent and safe to retry:

1. Create the database `tenant_<slug>` and role `tenant_<slug>` on the shared Postgres cluster.
   - `REVOKE CONNECT ... FROM PUBLIC`, and set a per-role connection limit.
   - Admin uses a dedicated provisioning role for this and never reads tenant data.
2. Generate the secrets and store them in AWS Secrets Manager:
   - `DATABASE_URL`;
   - `TENANT_API_KEY` (admin keeps only its SHA-256);
   - the PIN;
   - the executive manager's email and a one-time password (`EXECUTIVE_MANAGER_*`). The tenant marks that password one-time: until they choose their own, every route except `/auth/me`, `/auth/password` and sign-out answers `403 password_change_required`;
   - `ADMIN_PUBLIC_KEY`.
3. Deploy the pre-built tenant image (a pinned version tag, never `latest`) as a new ECS/App Runner service with those env vars.
4. Point DNS `<slug>.<domain>` at the service, with a TLS certificate (Route 53 + ACM).
5. Wait for the first heartbeat, then mark the tenant `active` and email the executive manager.

Upgrades: every tenant runs the same image version, rolled out in batches. Each tenant runs its own `sqlx` migrations on startup, so migrations must be expand/contract: add the new thing, ship, remove the old thing in a later release.

## 2. Admin ↔ tenant calls

**Tenant → admin.** Header `X-Tenant-Key` (the existing key). Endpoints, all under `/api/v1/internal/` on admin:
- `GET  tenant`: profile (exists today).
- `GET  allowances`: the allowances document (section 3).
- `POST usage`: a batch of usage events (section 5).
- `POST heartbeat`, every 60 s (built):
  - body `{version, last_migration, active_employees, active_sessions, database_bytes}`; lead and case counts get added once those exist;
  - admin keeps only the latest one per tenant in `tenant_heartbeats`;
  - `GET /api/v1/monitoring` on admin flags a tenant as `quiet` after 3 minutes without one.

**Admin → tenant.** Signed with Ed25519: admin holds the private key (`ADMIN_SIGNING_KEY`), and each tenant gets the public key as `ADMIN_PUBLIC_KEY`. Both are 32 bytes, base64url without padding. Admin logs the public key on startup.
- The signed bytes are exactly `METHOD\npath\ntimestamp\nhex(sha256(body))`:
  - `path` is the full request path, for example `/api/v1/internal/admin/refresh-allowances`;
  - `timestamp` is unix seconds.
- The signature goes in `X-Admin-Signature` (base64url, no padding) and the timestamp in `X-Admin-Timestamp`.
- Requests more than 5 minutes off are rejected. Inside that window a captured request could be replayed, so every admin action must be idempotent.
- A tenant without `ADMIN_PUBLIC_KEY` refuses every admin call.
- `/internal/admin/*` bypasses the allowances gate, so admin can still reach a suspended tenant and lift the suspension.
- Admin knows where to call through `tenants.api_url`. Pushes run in the background; if one fails, the tenant still catches up at its next 5-minute refresh.
- A leaked tenant can't impersonate admin.
- Endpoints under `/api/v1/internal/admin/` on the tenant:
  - `POST refresh-allowances` (built): refetch now, so suspensions and upgrades apply in seconds. Admin sends it on every status or plan change;
  - `GET employees`;
  - `POST employees/{id}/revoke-sessions`;
  - `POST executive/reset-password`;
  - `POST read-only` (on/off).

Admin never opens a connection to a tenant database. Everything goes through the API above.

## 3. Allowances document

Admin builds it from the subscription, plan and overrides, and bumps `version` on every change:

```json
{
  "version": 42,
  "status": "active",
  "pin": "K7Q2M9",
  "modules": ["sales", "collection"],
  "limits": { "seats": 25, "leads": 10000, "cases": 5000, "esignatures_per_month": 300 },
  "calls": { "billing": "prepaid", "balance_cents": 125000, "price_per_minute_cents": 2 },
  "transaction_fee": { "percent_bp": 150, "fixed_cents": 0 },
  "period": { "start": "2026-10-01", "end": "2026-11-01" },
  "valid_until": "2026-10-29T00:00:00Z"
}
```

`calls.billing` is `"prepaid"` or `"postpaid"`. `percent_bp` is basis points (150 = 1.5%). Money is always integer cents.

**Tenant side (`platform/feature_flags/`):**
- **Fetching.** Fetch on startup, every 5 minutes, and whenever admin calls `refresh-allowances`. Keep it in memory and in a one-row table, so a restart without admin still works.
- **Offline.** If admin is unreachable, keep using the cached copy until `valid_until`, then go read-only.
- **Status.** `suspended` or `cancelled` blocks login and every write.

## 4. Hard limits (tenant)

Over-limit is **always blocked, never allowed and billed later**. Every block returns `402 Payment Required` with a body the frontend turns into an upgrade prompt:

```json
{ "error": "limit_reached", "limit": "seats", "allowed": 25, "used": 25, "upgrade": true,
  "message": "Your plan includes 25 users. Upgrade to add more." }
```

Other `error` values: `module_not_in_plan` (with `"module"`), `call_credit_exhausted`, `subscription_inactive`.

If the tenant has no current allowances document (none received yet, or past `valid_until`), reads and sign-in still work, but writes get `503` with `"error": "allowances_unavailable"`. That is not a billing problem, so there is no upgrade prompt.

How each limit is checked:

| What | Where | How |
|---|---|---|
| Module access | `route_layer(require_module(Module::Sales))` on each module's router | Module missing from `modules` → 402 |
| Seats, leads, cases | Application layer, **inside the transaction that creates the row** | `pg_advisory_xact_lock(hashtext('seats'))`, count, compare, insert. The lock stops two requests both slipping under the limit. |
| E-signatures per month | Same, before sending | Count `usage_events` for the current `period` |
| Prepaid calls | Before dialing, and again during a call | `balance_cents` minus usage not yet billed must cover at least one minute. Hang up at zero. |

## 5. Usage metering and billing

**Tenant: the outbox.** A billable action writes a `usage_events` row in the **same transaction** as the business record:

```
usage_events(id uuid primary key, metric text, quantity bigint, amount_cents bigint null,
             occurred_at timestamptz, reference text, sent_at timestamptz null)
```

| Metric | Written when | Quantity |
|---|---|---|
| `call_minutes` | Telephony provider's call-ended webhook | Minutes, rounded up per call |
| `payment_transactions` | Debtor-payment gateway's success webhook | 1, with `amount_cents` of the payment |
| `esignatures` | E-signature provider confirms sending | 1 |

Seats, leads and cases are capped allowances, not metered. Their counts travel in the heartbeat for monitoring.

A background loop, like the session purge, sends unsent rows to admin `POST usage` in batches of up to 500 and sets `sent_at`. Admin inserts with `ON CONFLICT (id) DO NOTHING`, so resending never double-bills.

**Admin: billing (`modules/billing/`, all Stripe code in `infrastructure/stripe.rs`).**
- **Stripe products and prices:**
  - one product per module, with a monthly and an annual price;
  - a per-seat price (subscription item `quantity` = seats);
  - plan tiers for the lead, case and e-signature limits;
  - metered prices (Billing Meters) for postpaid call minutes and transaction fees.
- **Annual plans.** Stripe subscription items must share one billing interval. So an annual customer has an annual subscription for modules and seats, plus a monthly subscription for metered usage.
- **Postpaid calls.** Report each usage event as a Stripe meter event. Use the usage event `id` as the idempotency key.
- **Prepaid calls.** The customer buys credit through Checkout (one-time payment). Admin keeps a `credit_ledger` (top-ups +, usage −) as the source of truth, and publishes the balance in the allowances document. At zero the tenant blocks calls (section 4).
- **Upgrades** go through Stripe's Customer Portal or a Checkout session. The upgrade message's call to action links there.
- **Webhooks:**
  - verify the signature, store the event id, and ignore duplicates;
  - handle `checkout.session.completed`, `customer.subscription.updated|deleted`, `invoice.paid` and `invoice.payment_failed`;
  - a failed payment moves the tenant to `past_due`; after a grace period (default 7 days) it becomes `suspended`, and admin pushes `refresh-allowances`.
- **Card data** never touches our servers. Store Stripe ids only.

**Stripe caveats (verify before building billing):**
- A Stripe account must belong to a business registered in a supported country. The US is supported; Lebanon is not. Billing therefore needs a US entity with a US bank account (for example through Stripe Atlas). Whether a Lebanon-resident owner can open one is unconfirmed, so check with Stripe first. Development can use test mode.
- Stripe's restricted-businesses list **prohibits debt collection agencies**. That makes Stripe usable, at most, for *our* subscription billing; confirm with Stripe that SaaS sold to collection agencies is acceptable.
- **Debtor payments** (tenants charging debtors) must use a separate processor that underwrites collections merchants. In the tenant it sits behind one adapter in `modules/collection/infrastructure/`. The per-transaction fee is measured from that processor's webhooks, not from Stripe.

## 6. Tenant PIN and shared login

- **The shared login page** (`login.<domain>`) is served by admin. The user enters the PIN, then the page calls the public `POST /api/v1/directory/lookup {"pin"}`.
  - It gets back `{name, login_url}`, where `login_url` is `<api_url>/login?pin=<PIN>`, and sends the browser there to sign in with email and password.
  - Built: the lookup. The page itself is frontend work.
- **PIN format:** 6 characters from `23456789ABCDEFGHJKLMNPQRSTUVWXYZ` (no 0/O/1/I), random, unique, stored on admin's `tenants.pin`. Input is trimmed and uppercased before comparing.
  - The master tenant has none; every other tenant must have one (CHECK constraint).
  - `POST /api/v1/tenants/{id}/pin` rotates it. That bumps the allowances `version` and pushes `refresh-allowances`, so the old PIN stops working within a second.
- **Lookup responses:** 404 for an unknown or malformed PIN, a suspended or cancelled tenant, or one without `api_url`. All of these look the same from outside.
- **Rate limits** live in Redis (`platform/rate_limit.rs`, the same file in both repos) as fixed windows, with keys `ratelimit:<service>:<rule>:<sha256 prefix of the key>`:
  - PIN lookup: 10 per minute per IP;
  - sign-in failures, on admin and on the tenant: 30 per 15 minutes per IP, and 5 per 15 minutes per email. A wrong PIN counts as a failure;
  - successful sign-ins don't count;
  - over the limit gets `429` with `Retry-After`.
  - The client IP is the connection's peer address, or the last `X-Forwarded-For` entry when `TRUST_PROXY=true`.
  - If Redis is unreachable at runtime the limits are skipped (fail open, logged). An unreachable `REDIS_URL` at startup is fatal; no `REDIS_URL` means no limits.
- **Tenant check:** each tenant compares the submitted `pin` (required in the login body) with `allowances.pin`. A mismatch is `401` `"this PIN belongs to a different company"`. Before the first allowances document arrives any PIN is accepted.
- **The PIN is a locator, not a secret.** It never replaces the password.

## 7. Permissions (tenant, `platform/permissions/`)

- **Targets are sections of pages**, written `<module>.<page>.<section>` and stored as text, for example `collection.cases.status`, `collection.cases.notes`, `core.employees.profile`.
  - They're defined in code (`permissions/sections.rs`), not in a table. `Page` lists the pages; `SectionKind` lists section types; `Page::sections()` says which sections a page has.
  - A new page or section is a code change. An unknown key is rejected with 422.
- **Switches per section.** Each section declares which of create / read / update / delete mean something for it (`SectionKind::actions`):
  - lists of items (notes, documents) have all four;
  - single values (status, details, tags, custom fields) only read and update;
  - every page has a **`record`** section: create the record, see it in lists, delete it. Editing the record happens section by section.
  - Saving a switch a section doesn't have is a 400. Stored switches a section has since lost are dropped when access is loaded.
- **Opening a page:** someone may open a page if they can read at least one of its sections.
- **Shared features are sections on each page** (`collection.cases.notes`, `collection.debtors.notes`), so they're granted per page.
- **The backend enforces sections, not just the UI:**
  - responses leave out sections the employee can't read;
  - each section is changed through its own endpoint (`PATCH /cases/{id}/status`), so one endpoint means one check.
- **Core pages** such as `core.employees` aren't tied to a paid module and are always available.
- **Effective access = the module is in `allowances.modules` AND the employee holds the switch.**
  - The executive manager has every switch of every section of the paid modules and core pages.
  - Nobody, the executive included, gets into an unpaid module.
- Grants on sections of unpaid modules are stored but have no effect. They come back on upgrade. Stored rows for sections that no longer exist are skipped.
- **Templates** (`permission_templates`): named sets of section permissions ("Collector"). The executive manager creates them and applies them to employees; applying copies the grants, which can then be adjusted. Editing a template later doesn't change employees it was applied to.
- **Setting permissions and managing templates is executive-only.** `core.employees.permissions` grants only *seeing* other employees' permissions.
- `GET /auth/me` and `GET /employees/{id}` return effective permissions for the UI. `GET /permissions/sections` returns every page, its sections, their switches, and whether the module is paid.
- A module's routes return 402 `module_not_in_plan` through `FeatureFlags::require_module`. That check accepts a stale document, because the gate already blocks writes once it's stale. The effective-access filter is the safety net if a router forgets the layer.

## 8. Monitoring

- **Business health** comes from heartbeats and is shown in the admin console: which tenants are quiet, which version each runs, usage against limits, failed payments.
- **System health:** each tenant sends OpenTelemetry traces and metrics to CloudWatch or Grafana.
- **Audit:** permission changes, limit blocks and admin control actions are written to an `audit_log` table on the side where they happen.

## 9. Shared features (tenant), and where admin takes part

The tenant has shared **shared features** used by all modules: notes, reminders, chat, tags, folders and documents, import/export, user session records, custom fields, and status/sub-status. They live in the tenants repo under `src/shared_features/`, and the full design is in the tenants skill, `references/shared-features.md`. Admin needs no code for most of them. Where it takes part (*proposed*, pending the user's confirmation):
- **Plan limits in the allowances `limits`:**
  - `storage_bytes` (documents), enforced on upload with the usual `402 limit_reached`;
  - optionally `custom_fields` and `import_rows_per_month`.
- **Provisioning (section 1):** also creates the tenant's S3 prefix and storage credentials, stored with the other secrets.
- **Heartbeat (section 2):** also reports `storage_bytes_used`, `documents` and `jobs_failed_24h`.
- **Support access (section 2):** signed `GET /internal/admin/session-records` on the tenant. It's read-only and used for security investigations; it gets added to the list of admin→tenant endpoints when built.

Rule of thumb, unchanged: admin decides how much a tenant may use; the tenant owns the data, including notes, documents, chats and session records, which never leave the tenant except through these explicit, signed calls.
