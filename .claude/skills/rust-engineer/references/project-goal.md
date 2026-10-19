# Project: rust-unity

## Project Goal & Architectural Blueprint

## Final goal (read before making ANY change)
This skill establishes the comprehensive blueprint and vision for the ERP Modular Monolith application, bundling a TypeScript/React SPA directly into a single Rust/Axum executable inside a single Podman container. The final project should be a modular software that uses pay-per-module strategy with customizable permissions 

## Target Architecture & Tech Stack
* Architecture: Modular Monolith with Domain-Driven Design (DDD) principles.
* Backend: Rust, Axum 0.7+, SQLx (PostgreSQL), Redis (Session management & Opaque Tokens).
* Frontend: TypeScript, React 18+, Vite, Tailwind CSS.
* Container Strategy: Single Podman container bundling both static React assets (dist/) and the compiled Axum Rust binary.
* Deployment Target: AWS App Runner or AWS ECS Fargate via Amazon ECR.
* Repository Strategy: Single GitHub Monorepo for all apps and services.

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
API Proxying: Configure vite.config.ts proxy to route /api requests to http://localhost:8080.

2. Multi-Stage Podman Build Strategy
- Stage 1 (Frontend): Build React SPA with Node 20 into static dist/.
- Stage 2 (Rust Chef): Cache Rust dependencies using cargo-chef.
- Stage 3 (Builder): Copy dist/ and compile Axum binary with static asset embedding (tower_http::services::ServeDir).
- Stage 4 (Runtime): Minimal debian:bookworm-slim container running as non-root (uid=10001).

3. Key Technical Guarantees
Zero unwrap() or expect() in production code paths.
Redis-backed Opaque Tokens in platform/auth/.
Module-level entitlement checks in platform/feature_flags/.

## Before every change, check:
1. Does this move the project toward the final goal above?
2. Does it break any constraint listed above?
3. If a request conflicts with the goal, point out the conflict before writing code.
