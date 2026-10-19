# Bundles the React SPA and the Axum backend into one image. Lives at the
# repo root because the build needs both frontend/ and backend/:
#   podman build -t admin:local .
#
# Base images come from Red Hat's registry (UBI), fully qualified because
# Podman doesn't silently resolve short names.

########################################################################
# Stage 1/3 — Frontend: compile the React/Vite/Tailwind SPA into dist/
########################################################################
FROM registry.access.redhat.com/ubi10/nodejs-22 AS frontend
USER 0
WORKDIR /app/frontend

COPY frontend/package.json frontend/package-lock.json ./
RUN npm ci

COPY frontend/ ./
RUN npm run build

########################################################################
# Stage 2/3 — Backend: compile the Axum release binary. Cache mounts keep
# the cargo registry and target/ between builds, so only changed crates
# recompile.
########################################################################
FROM registry.access.redhat.com/ubi10/ubi AS backend
RUN dnf install -y --setopt=install_weak_deps=False gcc \
    && dnf clean all

ENV RUSTUP_HOME=/usr/local/rustup \
    CARGO_HOME=/usr/local/cargo \
    PATH=/usr/local/cargo/bin:$PATH
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
    | sh -s -- -y --profile minimal --default-toolchain stable

WORKDIR /app
COPY backend/Cargo.toml backend/Cargo.lock ./
COPY backend/src ./src
# sqlx::migrate!() embeds these at compile time.
COPY backend/migrations ./migrations
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/app/target \
    cargo build --release --locked --bin admin \
    && cp target/release/admin /app/admin

########################################################################
# Stage 3/3 — Runtime: minimal UBI image, non-root, read-only app files
########################################################################
FROM registry.access.redhat.com/ubi10/ubi-minimal AS runtime
WORKDIR /app

COPY --from=backend /app/admin ./admin
COPY --from=frontend /app/frontend/dist ./web/dist

USER 10001:10001

ENV BIND_ADDR=0.0.0.0:8080
EXPOSE 8080

ENTRYPOINT ["/app/admin"]
