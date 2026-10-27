# Bundles the React SPA and the Axum backend into one image. Lives at the
# repo root because the build needs both frontend/ and backend/:
#   podman build -t admin:local .
#
# Debian bookworm throughout, so the binary is built against the same glibc
# it runs on. Image names are fully qualified because Podman doesn't guess
# registries for short names.

ARG CHEF=docker.io/lukemathwalker/cargo-chef:0.1.78-rust-1.99.1-slim-bookworm

########################################################################
# Stage 1/4 — Frontend: compile the React/Vite/Tailwind SPA into dist/
########################################################################
FROM docker.io/library/node:22-bookworm-slim AS frontend
WORKDIR /app/frontend

# Where packages come from. Networks that block registry.npmjs.org can
# build through a mirror, e.g. --build-arg NPM_REGISTRY=https://registry.npmmirror.com.
# Every package is still checked against the hashes in package-lock.json.
ARG NPM_REGISTRY=https://registry.npmjs.org
COPY frontend/package.json frontend/package-lock.json ./
RUN npm ci --registry="$NPM_REGISTRY"

COPY frontend/ ./
RUN npm run build

########################################################################
# Stage 2/4 — Plan: cargo-chef boils the manifests down to a recipe that
# only changes when dependencies do.
########################################################################
FROM ${CHEF} AS planner
WORKDIR /app
COPY backend/ ./
RUN cargo chef prepare --recipe-path recipe.json

########################################################################
# Stage 3/4 — Build: dependencies first, as their own cached layer, then
# our code. A code-only change skips straight to the last step.
########################################################################
FROM ${CHEF} AS builder
# aws-lc (rustls' crypto) may need cmake to compile its C code.
RUN apt-get update \
    && apt-get install -y --no-install-recommends cmake \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app

COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --locked --recipe-path recipe.json

COPY backend/Cargo.toml backend/Cargo.lock ./
COPY backend/src ./src
# sqlx::migrate!() embeds these at compile time.
COPY backend/migrations ./migrations
RUN cargo build --release --locked --bin admin

########################################################################
# Stage 4/4 — Runtime: Debian slim, non-root, CA certificates for HTTPS
# calls (admin <-> tenants).
########################################################################
FROM docker.io/library/debian:bookworm-slim AS runtime
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app

COPY --from=builder /app/target/release/admin ./admin
COPY --from=frontend /app/frontend/dist ./web/dist

USER 10001:10001

ENV BIND_ADDR=0.0.0.0:8080
EXPOSE 8080

ENTRYPOINT ["/app/admin"]
