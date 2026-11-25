FROM node:24.22.3-bookworm-slim AS frontend
WORKDIR /web
COPY frontend/package.json frontend/package-lock.json ./
RUN npm ci --no-audit --no-fund
COPY frontend/ ./
RUN npm run build

FROM rust:1.100.0-bookworm AS build
WORKDIR /build
COPY Cargo.toml Cargo.lock rust-toolchain.toml build.rs ./
COPY src/ ./src/
COPY migrations/ ./migrations/
COPY assets/branding/favicon.svg ./assets/branding/favicon.svg
COPY --from=frontend /web/dist ./frontend/dist
RUN cargo build --locked --release

FROM debian:bookworm-slim AS runtime
LABEL org.opencontainers.image.title="ShortCircuit" \
      org.opencontainers.image.description="Self-hosted URL shortener and link management" \
      org.opencontainers.image.version="0.6.0"
RUN install -d -m 0700 -o 10001 -g 10001 /data
COPY --from=build /build/target/release/shortcircuit /usr/local/bin/shortcircuit
ENV DATA_DIR=/data \
    PUBLIC_BIND=0.0.0.0:8080 \
    ADMIN_BIND=0.0.0.0:8081 \
    ADMIN_ORIGIN=http://localhost:8081 \
    PUBLIC_HOSTS=localhost,127.0.0.1,[::1] \
    LOG_LEVEL=info
USER 10001:10001
EXPOSE 8080 8081
HEALTHCHECK --interval=30s --timeout=5s --start-period=5s \
    CMD ["/usr/local/bin/shortcircuit", "healthcheck"]
ENTRYPOINT ["/usr/local/bin/shortcircuit"]
