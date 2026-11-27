# Base images are pinned by digest. Update a tag and its digest together in a reviewed change.
FROM node:24.22.3-bookworm-slim@sha256:873f586f3794f52281bd039bf02c048a0be08fde619338f9941bad1eb6b34530 AS frontend
WORKDIR /web
COPY frontend/package.json frontend/package-lock.json ./
RUN npm ci --no-audit --no-fund
COPY frontend/ ./
RUN npm run build
# The bundled admin interface is built from development dependencies such as Svelte,
# so the release SBOM lists the whole locked frontend build tree.
RUN npm sbom --sbom-format cyclonedx > /web/frontend-sbom.cdx.json
RUN mkdir -p /notices/npm && cd node_modules && \
    find . -type f \( -iname 'license*' -o -iname 'copying*' -o -iname 'notice*' -o -iname 'copyright*' \) \
    -exec cp --parents --target-directory=/notices/npm {} +

FROM rust:1.100.0-bookworm@sha256:75d26c23c24cc6ea1cdabf0714a6bff8fd7bf3248716043fd7ae86dac7f41b94 AS build
WORKDIR /build
COPY Cargo.toml Cargo.lock rust-toolchain.toml build.rs ./
COPY src/ ./src/
COPY migrations/ ./migrations/
COPY docs/openapi.json ./docs/openapi.json
COPY assets/branding/favicon.svg assets/branding/favicon.png assets/branding/apple-touch-icon-180.png \
     assets/branding/pwa-192.png assets/branding/pwa-512.png assets/branding/pwa-maskable-512.png \
     assets/branding/manifest.webmanifest ./assets/branding/
COPY --from=frontend /web/dist ./frontend/dist
# cargo-auditable embeds the locked dependency list, so image scanners and SBOMs see the crates.
RUN cargo install cargo-auditable --version 0.7.8 --locked
ARG SHORTCIRCUIT_BUILD_REVISION=""
ENV SHORTCIRCUIT_BUILD_REVISION=$SHORTCIRCUIT_BUILD_REVISION
RUN cargo auditable build --locked --release
RUN mkdir -p /notices/rust && cd /usr/local/cargo/registry/src && \
    find . -type f \( -iname 'license*' -o -iname 'copying*' -o -iname 'notice*' -o -iname 'copyright*' \) \
    -exec cp --parents --target-directory=/notices/rust {} +

FROM debian:bookworm-slim@sha256:734a72b3d6142a71993b4bc580ca01d8ca687f77a2c422d96a961d3376b179bc AS runtime
LABEL org.opencontainers.image.title="ShortCircuit" \
      org.opencontainers.image.description="Self-hosted URL shortener and link management" \
      org.opencontainers.image.source="https://github.com/Buffer379x/shortcircuit" \
      org.opencontainers.image.licenses="MIT" \
      org.opencontainers.image.version="0.8.0-preview.2"
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && install -d -m 0700 -o 10001 -g 10001 /data
COPY --from=build /build/target/release/shortcircuit /usr/local/bin/shortcircuit
COPY LICENSE /usr/share/doc/shortcircuit/LICENSE
COPY --from=build /notices/rust /usr/share/doc/shortcircuit/licenses/rust
COPY --from=frontend /notices/npm /usr/share/doc/shortcircuit/licenses/npm
COPY --from=frontend /web/frontend-sbom.cdx.json /usr/share/doc/shortcircuit/frontend-sbom.cdx.json
# SQLite needs a writable directory for statement journals, sorts, and VACUUM.
# The root filesystem is read-only, so temporary files use the data volume.
ENV DATA_DIR=/data \
    SQLITE_TMPDIR=/data/tmp \
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
