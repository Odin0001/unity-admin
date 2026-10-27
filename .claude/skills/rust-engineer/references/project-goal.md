# Project: rust-unity

## Project Goal & Architectural Blueprint

## Final goal (read before making ANY change)
This skill establishes the comprehensive blueprint and vision for the ERP Modular Monolith application, bundling a TypeScript/React SPA directly into a single Rust/Axum executable inside a single Podman container. The final project should be an admin software that monitors and controls other tenants in rust-unity-tenants including users, billing, databases, pricing, etc..

This repo is the **control plane**: it is the source of truth for tenants, plans, allowances, PINs, usage and billing, and it monitors and controls every tenant instance. Read `references/architecture.md` before touching any of those areas.

## Target Architecture & Tech Stack
* Architecture: Modular Monolith with Domain-Driven Design (DDD) principles.
* Backend: Rust, Axum 0.7+, SQLx (PostgreSQL). Sessions are opaque tokens stored (hashed) in Postgres; Redis is for rate limiting and short-lived caches only.
* Frontend: TypeScript, React 18+, Vite, Tailwind CSS.
* Container Strategy: Single Podman container bundling both static React assets (dist/) and the compiled Axum Rust binary.
* Deployment Target: AWS App Runner or AWS ECS Fargate via Amazon ECR.
* Repository Strategy: Two repositories that form one platform: `rust-unity-admin` (this one, control plane) and `rust-unity-tenants` (one instance per customer).
* Payments: Stripe (subscriptions, metered usage, prepaid credit). See the Stripe note in `references/architecture.md`.

## Business rules (decided, don't re-litigate)
- Target market is the United States.
- Every tenant gets its own frontend + backend instance and its own database, created from a pre-built image when the customer subscribes. All tenant databases live on one shared Postgres cluster.
- Tenants have three paid modules: Sales, Collection, HR. Also paid: seats (users), leads, cases, e-signatures, call minutes, and a fee per debtor payment transaction.
- Limits are hard. Going over is blocked with an upgrade message, never allowed and billed later.
- Subscriptions are monthly or annual. Calls can be prepaid credit or billed afterwards, per tenant.
- Each tenant has a PIN. Users enter it on one shared login page, which uses it to find their tenant and send them to that tenant's own login.
- Each tenant employee gets custom create/read/update/delete permissions per section of each page (for example a case's status, contact information or notes), set by that tenant's executive manager, optionally starting from a permission template.

## Non-negotiable constraints
- Backend is Rust (axum/tokio), frontend is react 18+; they talk via REST and WebSocket when needed
- Must run in Podman on Linux; target performance and code quality must be peak in both development and prouction environments as if written by a senior engineer with no bloated or unnecessary bloated code that will make things more complicated
- No new dependencies without justification
- Humanize each code you write so that it doesn't look AI generated or over complicated
- Make sure you write the code like a senior engineer would do, no bloated code no unnecessary complex logic

## Core Guidelines & Development Workflow
1. Local Development
Backend: Run cargo watch -x run inside the service directory for automatic recompilation on .rs file changes.
Frontend: Run npm run dev inside the app directory for Vite Hot Module Replacement (HMR).
API Proxying: Configure vite.config.ts proxy to route /api requests to the backend (admin runs on 8081 so it can sit next to a tenant on 8080).

2. Multi-Stage Podman Build Strategy
- Stage 1 (Frontend): Build React SPA with Node 22 LTS into static dist/ (Node 20 reached end of life in April 2026).
- Stage 2 (Rust Chef): Cache Rust dependencies using cargo-chef.
- Stage 3 (Builder): Copy dist/ and compile Axum binary with static asset embedding (tower_http::services::ServeDir).
- Stage 4 (Runtime): Minimal debian:bookworm-slim container running as non-root (uid=10001).

3. Key Technical Guarantees
Zero unwrap() or expect() in production code paths.
Opaque session tokens (256-bit, stored as SHA-256) in Postgres, sent as `Authorization: Bearer`.
Admin never connects to a tenant database; it talks to tenants over their internal API.
Every money amount is an integer number of cents (or `rust_decimal`), never a float.

## Before every change, check:
1. Does this move the project toward the final goal above?
2. Does it break any constraint or business rule listed above?
3. Does it keep the admin ↔ tenant contracts in `references/architecture.md` intact? Change both repos together if not.
4. If a request conflicts with the goal, point out the conflict before writing code.
5. After the change, update the **Features** section of the README in **both** repos (`rust-unity-admin/README.md` and `rust-unity-tenants/README.md`) so it describes how every feature works today. Write it for someone who uses or runs the product, not for a Rust developer: what the feature does, who can use it, what happens step by step, and what the limits and edge cases are. Keep routes and table names to a minimum. A change that alters behaviour isn't done until both READMEs match it.
