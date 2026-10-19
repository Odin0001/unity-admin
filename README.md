# Platform Admin

Super-tenant console for the debt collection platform: the platform
owner's side for managing tenants, subscriptions, and module access. The
tenant-facing app lives in its own repository (`rust-unity`).

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

A layer starts as one file and becomes a folder once it outgrows it.

- `identity`: users, sign-in, sessions and the super user
- `tenants`: the tenant registry and the endpoint tenant deployments call

`platform/` holds shared technical pieces (database pool, error responses,
token and password hashing) and `shared_kernel/` holds `TenantId`.

## Run everything

Requires Podman and [podman-compose](https://github.com/containers/podman-compose)
(`sudo pacman -S podman-compose` on Arch). All images come from `quay.io`
and Red Hat's registry (`registry.access.redhat.com`).

```bash
cd backend
cp .env.example .env   # adjust credentials if you want; defaults work locally
podman-compose up -d --build
```

The first build takes several minutes (image pulls + a full Rust release
compile); later builds reuse the cargo cache mounts.

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
| `redis`    | not used yet (sessions live in Postgres)           | 6380  |
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

## Authentication

`POST /api/v1/auth/login` with `{ "email", "password" }` returns
`{ token, expires_at, user }`. Send the token back as
`Authorization: Bearer <token>` on every request; there are no cookies.
`POST /api/v1/auth/logout` revokes it. Sessions are rows in
`auth_sessions` (only the token's SHA-256 is stored) and end after 12 hours,
or 60 minutes without a request.

A fresh database has no users, so on startup the service creates a
**super user** in the master tenant from `SUPER_USER_EMAIL` /
`SUPER_USER_PASSWORD` (see `.env.example`). It only does this when no user
has that email yet; changing the password variable later doesn't touch an
existing account.

Tenant deployments authenticate with the API key returned once by
`POST /api/v1/tenants`, sent as `X-Tenant-Key`.

## Tests

```bash
cd backend && cargo test
```

Integration tests use `#[sqlx::test]`, which creates a throwaway database
per test, so `DATABASE_URL` (from `.env`) must point at a reachable Postgres
whose role has `CREATEDB`:

```sql
ALTER ROLE admin_owner CREATEDB;
```

## Individual container builds

```bash
# from the repo root
podman build -t admin:local .
podman run --rm -p 8081:8080 \
  -e DATABASE_URL=postgres://admin_owner:admin_dev_password@host.containers.internal:5433/platform_admin \
  -e REDIS_URL=redis://:admin_dev_password@host.containers.internal:6380 \
  admin:local
```

Runs as non-root (`uid=10001`) on a UBI 10 minimal base.
