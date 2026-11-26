FROM node:24.22.3-bookworm-slim AS frontend
WORKDIR /web
COPY frontend/package.json frontend/package-lock.json ./
RUN npm ci --no-audit --no-fund
COPY frontend/ ./
RUN npm run build
RUN mkdir -p /notices/npm && cd node_modules && \
    find . -type f \( -iname 'license*' -o -iname 'copying*' -o -iname 'notice*' -o -iname 'copyright*' \) \
    -exec cp --parents --target-directory=/notices/npm {} +

FROM rust:1.100.0-bookworm AS build
WORKDIR /build
COPY Cargo.toml Cargo.lock rust-toolchain.toml build.rs ./
COPY src/ ./src/
COPY migrations/ ./migrations/
COPY docs/openapi.json ./docs/openapi.json
COPY assets/branding/favicon.svg assets/branding/favicon.png ./assets/branding/
COPY --from=frontend /web/dist ./frontend/dist
ARG SHORTCIRCUIT_BUILD_REVISION=""
RUN SHORTCIRCUIT_BUILD_REVISION="$SHORTCIRCUIT_BUILD_REVISION" cargo build --locked --release
RUN mkdir -p /notices/rust && cd /usr/local/cargo/registry/src && \
    find . -type f \( -iname 'license*' -o -iname 'copying*' -o -iname 'notice*' -o -iname 'copyright*' \) \
    -exec cp --parents --target-directory=/notices/rust {} +

FROM debian:bookworm-slim AS runtime
LABEL org.opencontainers.image.title="ShortCircuit" \
      org.opencontainers.image.description="Self-hosted URL shortener and link management" \
      org.opencontainers.image.source="https://github.com/Buffer379x/shortcircuit" \
      org.opencontainers.image.licenses="MIT" \
      org.opencontainers.image.version="0.6.0-preview.2"
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && install -d -m 0700 -o 10001 -g 10001 /data
COPY --from=build /build/target/release/shortcircuit /usr/local/bin/shortcircuit
COPY LICENSE /usr/share/doc/shortcircuit/LICENSE
COPY --from=build /notices/rust /usr/share/doc/shortcircuit/licenses/rust
COPY --from=frontend /notices/npm /usr/share/doc/shortcircuit/licenses/npm
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
