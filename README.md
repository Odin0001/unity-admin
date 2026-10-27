# Platform Admin

The control plane of the debt collection platform. The platform is two
products that work as one:

- **Admin** (this repository) is run by us, the platform owner. It keeps the
  list of customers (tenants), decides what each one has paid for, and will
  later handle billing, provisioning and monitoring.
- **Tenants** (`rust-unity-tenants`) is the app a customer's staff uses.
  Every customer gets their own copy with its own database. It asks admin
  what it is allowed to do and enforces that on its own.

Only the backend is built so far; the frontend is a placeholder.

## Features

### The super user

Admin has one kind of account today: platform staff in the **master
tenant** (called `platform`, created with the database). When the service
starts on an empty database it creates the first of these accounts, the
super user, from the email and password in its settings
(`SUPER_USER_EMAIL`, `SUPER_USER_PASSWORD`). If an account with that email
already exists nothing happens, so changing the password setting later does
not change a real password. The password must be at least 12 characters.

Everything in admin (tenants, plans, subscriptions) is limited to master
tenant staff. Anyone else who manages to sign in is refused.

### More super users

Any super user can add another one by entering their email and name. All
super users are equal: each can do everything, including adding more.

1. The new person gets an email with a link to set their own password, so
   nobody else, not even the person who added them, ever knows it.
2. The link works once and for 3 days. Until they use it they can't sign
   in, and the super users' list shows them as not set up yet.
3. If the link expires, they use "forgot password" with their email to
   get a fresh one.

An email can only belong to one account.

### Signing in and sessions

1. The user sends their email and password. Emails are not case
   sensitive.
2. On success admin hands back a random token (256 bits) and the time it
   expires. Admin only keeps a fingerprint (hash) of it, so a copy of the
   database can't be used to sign in.
3. The app sends that token with every request. There are no cookies.
4. A session ends after 12 hours, after 60 minutes without any request, on
   sign-out, or as soon as the user or their tenant is disabled.
5. Every 15 minutes admin clears out ended sessions.

A wrong email and a wrong password get the same answer, so nobody can find
out which emails have accounts.

Guessing passwords is slowed down. After 5 failed sign-ins for one email,
or 30 from one address, within 15 minutes, further attempts are refused
until that window ends. The answer says how many seconds to wait. That
includes attempts with the right password, so a guesser can't keep going
until something works. Successful sign-ins never count.

### Forgotten passwords

1. On the sign-in page the user enters their email and asks for a reset.
2. If the email belongs to an account, admin emails a link to choose a new
   password. The page shows the same message whether or not it does, so
   nobody can use it to find out who has an account.
3. The link works once, for one hour. Asking again replaces the earlier
   link, so only the newest one works.
4. Choosing the new password (at least 12 characters) signs the user out
   everywhere, in case someone else had got in.

To stop anyone from flooding an inbox, one email gets at most 3 reset
emails an hour, and one address can ask 10 times in 15 minutes.

Emails go out through an SMTP server (`SMTP_URL`, for example AWS SES).
Without one, as in local development, the email is written to admin's log
instead, link included.

### Changing your password

Signed-in users can change their password by giving the current one and
a new one. Every other session of theirs ends; the one they're using
stays signed in.

### Tenants

The super user registers a customer with a short name used in its web
address (the slug, e.g. `acme`: 2 to 63 lowercase letters, digits or
dashes) and a display name. Slugs are unique.

Registering a tenant produces its **API key**. It is shown exactly once and
goes into that tenant's deployment settings; admin keeps only its hash. The
tenant's deployment uses the key to identify itself whenever it talks to
admin, and every such call updates the tenant's "last seen" time, which is
the start of monitoring.

Each tenant also has the web address of its deployment (for example
`https://acme.example.com`). It can be given when the tenant is registered
or added later, together with a new display name. Only `http://` and
`https://` addresses are accepted. Admin needs the address to send a
deployment instructions (see "Controlling tenants").

### PINs and the shared login page

Every customer tenant gets a **PIN** when it's registered: six characters
like `K7Q2M9`, made only of characters that can't be mixed up (no 0 or O,
no 1 or I). Each PIN is unique, and the master tenant has none.

Staff of every customer start on one shared login page. It works like this:

1. They type their company's PIN. Upper or lower case and stray spaces
   don't matter.
2. Admin looks the PIN up and answers with the company's name and the
   address of its own sign-in page, with the PIN filled in.
3. The browser goes there, and they sign in with email and password on
   their company's deployment.

The lookup says "not found" for a PIN that doesn't exist, for a suspended
or cancelled company, and for one that isn't deployed yet. From outside
these all look the same. To stop anyone from trying PINs one after another
to discover our customers, one address can make 10 lookups a minute.

The PIN only finds the company; it isn't a password. If it spreads too
widely, the super user can give the tenant a new one. The old PIN stops
working immediately, both on the login page and on the tenant itself.

### Tenant status

A tenant is always in one of four states:

| Status | What its employees experience |
| --- | --- |
| Active | Everything works. |
| Past due | Everything still works; this is the grace period after a failed payment. |
| Suspended | Nobody can sign in or change anything. Data is kept. |
| Cancelled | Same as suspended. |

The super user switches between them. The master tenant's status can't be
changed. Every switch counts as a change to the tenant's allowances (see
below), so the tenant always knows its current status. Later, Stripe
payment events will drive past due and suspended automatically.

### Controlling tenants

When the super user changes a tenant's status or plan, admin immediately
tells that tenant's deployment to fetch its allowances again. A suspension
or an upgrade therefore takes effect within a second instead of within 5
minutes. Admin does this in the background, so the super user never waits
on a slow or unreachable tenant. If the tenant can't be reached, or has no
address yet, it picks up the change on its own at its next 5-minute
refresh.

Every instruction admin sends is signed with admin's private key, and each
tenant checks it with admin's public key. That means:

- nobody else, not even another tenant, can pretend to be admin;
- an instruction can't be altered on the way, or reused against a
  different tenant endpoint;
- an instruction more than 5 minutes old is refused.

Admin works without a signing key, but then it can't send instructions;
tenants still follow changes every 5 minutes. On startup admin logs its
public key, which goes into every tenant's settings.

Today the only instruction is "fetch your allowances now". Revoking an
employee's sessions, resetting the executive manager's password and
read-only mode are planned.

### Monitoring

Every tenant deployment checks in once a minute with a **heartbeat**. It
reports:

- the software version it runs;
- its latest database update;
- how many active employees it has;
- how many people are signed in;
- how large its database is.

The super user sees one list of every customer tenant. It shows each
tenant's status, when it last contacted admin, and its latest heartbeat.
A tenant with no heartbeat for 3 minutes, or none ever, is marked
**quiet** and listed first. Quiet usually means the deployment is down or
can't reach admin.

Admin keeps only the latest heartbeat per tenant, not a history. The quiet
flag is the only alert so far; nothing emails anyone yet.

### Plans

Three plans exist, with placeholder prices:

| | Starter | Growth | Enterprise |
| --- | --- | --- | --- |
| Modules | Collection | Sales, Collection | Sales, Collection, HR |
| Users (seats) | 5 | 25 | 100 |
| Leads | 1,000 | 10,000 | 100,000 |
| Cases | 500 | 5,000 | 50,000 |
| E-signatures a month | 50 | 300 | 2,000 |
| Calls | prepaid, 3¢/min | prepaid, 2¢/min | billed after, 1¢/min |
| Fee per debtor payment | 2% + 30¢ | 1.5% + 25¢ | 1% + 20¢ |
| Monthly / annual | $99 / $990 | $299 / $2,990 | $999 / $9,990 |

### Subscriptions

The super user puts a tenant on a plan, monthly or annual. The current
billing period starts that day and runs one month or one year. Changing the
plan or the interval later replaces the old one and starts a new period.
The master tenant can't be put on a plan. Stripe will take this over later.

### Call credit

Tenants on prepaid calls have a credit balance. Every top-up and every use
is a line in a ledger, and the balance is their sum, so the history is never
lost. Top-ups are added by hand for now.

### Allowances: what a tenant is allowed to do

From the tenant's status, its plan and its credit balance, admin builds one
document, the **allowances**: which modules are paid for, every limit, how
calls are billed and the credit left, the transaction fee, the billing
period, the PIN, and a "valid until" time three days ahead. Each change to
the subscription, the status or the PIN gives it a new version number.

Tenant deployments fetch this document with their API key (on startup and
every 5 minutes) and enforce it themselves, so a tenant keeps working while
admin is briefly down. The paid modules decide which parts of the tenant
app anyone there can open. Moving a tenant to a plan without a module
closes every section of that module's pages for all its employees within
5 minutes, and moving back reopens them with the permissions they had. Admin also pushes
every change straight to the tenant (see "Controlling tenants"). A tenant
without a plan gets nothing and works in a reduced mode (see the tenants
README). The super user can view any tenant's document as well.

### Dummy data for development

`seeds/dev.sql` adds three tenants:

| Tenant | PIN | Plan | Notes |
| --- | --- | --- | --- |
| `acme` | `ACME42` | Growth, monthly | $1,250 call credit; API key from your `.env`; points at the tenant backend on port 8080 |
| `globex` | `GLBX42` | Starter, annual | $200 call credit; API key `dev-globex-key` |
| `initech` | `NTCH77` | Enterprise | suspended; API key `dev-initech-key` |

```bash
cd backend && psql "$DATABASE_URL" -f seeds/dev.sql
```

### Not built yet

Stripe billing, automatic provisioning of new tenants, the shared login
page itself (the PIN lookup behind it is done), usage reporting from
tenants, alerts for quiet tenants,
and further instructions to tenants (revoke sessions, reset a password,
read-only mode). The plan for all of it is in
`.claude/skills/rust-engineer/references/architecture.md`.

## API reference

All routes are under `/api/v1`. Signed-in routes take
`Authorization: Bearer <token>`; tenant routes take `X-Tenant-Key`.

```
POST  /auth/login                   {email, password} -> {token, expires_at, user}
POST  /auth/logout
GET   /auth/me
POST  /auth/password                {current_password, new_password}
POST  /auth/password-reset          (public) {email} -> always 202
POST  /auth/password-reset/confirm  (public) {token, password}
GET   /users                        super users, with has_password
POST  /users                        {email, full_name} -> invitation email
POST  /directory/lookup             (public) {pin} -> {name, login_url}
GET   /tenants                      list tenants
POST  /tenants                      {slug, name, api_url?} -> {tenant, api_key}
PATCH /tenants/{id}                 {name?, api_url?}
PUT   /tenants/{id}/status          {status: "active" | "past_due" | "suspended" | "cancelled"}
POST  /tenants/{id}/pin             new PIN
GET   /plans
PUT   /subscriptions/{tenant_id}    {plan, interval: "monthly" | "annual"}
GET   /subscriptions/{tenant_id}    the tenant's allowances
GET   /monitoring                   every tenant's latest heartbeat, quiet ones first
GET   /internal/tenant              (tenant key) who am I
GET   /internal/allowances          (tenant key) my allowances
POST  /internal/heartbeat           (tenant key) {version, last_migration, active_employees, active_sessions, database_bytes}
```

Rate-limited requests get `429` with a `Retry-After` header.

Calls from admin to a tenant are signed as described in
`.claude/skills/rust-engineer/references/architecture.md`, section 2.
Generate a signing key with:

```bash
openssl genpkey -algorithm ed25519 -outform DER | tail -c 32 | basenc --base64url | tr -d '='
```

## Code layout

```
backend/    Rust (Axum + SQLx) API, serves the built frontend too
frontend/   React + TypeScript + Vite + Tailwind SPA
```

Backend code follows DDD. Each business module under `backend/src/modules/`
has the same three layers:

```
domain.rs           types and business rules
application.rs      use cases, called by the HTTP layer
infrastructure/     Postgres queries and HTTP handlers
```

- `identity`: users, sign-in, sessions and the super user
- `tenants`: the tenant registry, statuses, and the signed client that
  calls tenant deployments
- `billing`: plans, subscriptions, call credit and the allowances document
- `monitoring`: heartbeats and the health overview

`platform/` holds shared technical pieces (database pool, error responses,
token and password hashing, rate limiting) and `shared_kernel/` holds
`TenantId`.

## Run everything

Requires Podman and [podman-compose](https://github.com/containers/podman-compose)
(`sudo pacman -S podman-compose` on Arch). The app image is built on
Debian (cargo-chef for cached Rust builds, `debian:bookworm-slim` to run);
Postgres and Redis come from `quay.io`. The host's distribution doesn't
matter: the image brings its own Debian userspace and runs the same on
Arch, Fedora or AWS.

```bash
cd backend
cp .env.example .env   # adjust credentials if you want; defaults work locally
podman-compose up -d --build
```

The first build takes several minutes (image pulls + a full Rust release
compile). After that, dependencies sit in their own cached layer and are
only rebuilt when `Cargo.lock` changes, so a code change recompiles just
this crate.

Check it's running:

```bash
podman-compose ps            # postgres/redis should be healthy before app starts
podman-compose logs -f app
```

This starts three containers on one Podman network. Host ports are offset
from the tenant platform's stack so both can run side by side:

| service    | what                                              | port  |
| ---------- | ------------------------------------------------- | ----- |
| `postgres` | `platform_admin` database                          | 5433  |
| `redis`    | rate-limit counters (sessions live in Postgres)    | 6380  |
| `app`      | backend + built frontend, one image, one process   | 8081  |

The `Containerfile` lives at the repo root because the image needs both
`backend/` and `frontend/`; `backend/compose.yaml` builds it with the
root as context.

Once it's up: **http://localhost:8081** serves the SPA, and the API lives
under `http://localhost:8081/api/v1`.

Tear down with `podman-compose down` (add `-v` to also drop the Postgres
and Redis volumes).

## Run for local development (hot reload)

```bash
# 1. Data services only
cd backend && podman-compose up -d postgres redis

# 2. Backend (new terminal)
cd backend && cargo run

# 3. Frontend (new terminal) — proxies /api/v1 to localhost:8081, see
#    frontend/vite.config.ts
cd frontend && npm install && npm run dev
```

Frontend dev server: **http://localhost:5174**. Backend directly:
**http://localhost:8081**.

## Tests

```bash
cd backend && cargo test
```

Integration tests use `#[sqlx::test]`, which creates a throwaway database
per test, so `DATABASE_URL` (from `.env`) must point at a reachable Postgres.
The rate-limit tests also need Redis, at
`REDIS_TEST_URL` (default `redis://127.0.0.1:6379`). Their keys expire on
their own within 15 minutes. The Postgres role needs `CREATEDB`:

```sql
ALTER ROLE admin_owner CREATEDB;
```

## Individual container builds

```bash
# from the repo root
podman build -t admin:local .
# if registry.npmjs.org is blocked on your network, use a mirror:
podman build --build-arg NPM_REGISTRY=https://registry.npmmirror.com -t admin:local .
podman run --rm -p 8081:8080 \
  -e DATABASE_URL=postgres://admin_owner:admin_dev_password@host.containers.internal:5433/platform_admin \
  -e REDIS_URL=redis://:admin_dev_password@host.containers.internal:6380 \
  admin:local
```

Runs as non-root (`uid=10001`) on `debian:bookworm-slim`.
