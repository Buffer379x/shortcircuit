# ShortCircuit

ShortCircuit is a self-hosted URL shortener and link manager. It uses Rust and SQLite and is designed to run in one Docker container. You can recreate an existing short URL by configuring its original host and creating a link with the same slug.

**0.6.0-preview.2.** ShortCircuit is licensed under the [MIT License](LICENSE). See the [project plan](docs/PROJECT_PLAN.md) and [verification status](docs/PROJECT_STATUS.md).

## Install on Unraid

Download `my-shortcircuit.xml` from the [GitHub prerelease](https://github.com/Buffer379x/shortcircuit/releases/tag/v0.6.0-preview.2) and select it in Unraid Docker Manager. Docker Manager pulls the versioned amd64 image from GitHub Container Registry and initializes its data volume automatically. [Installation and upgrade](docs/UNRAID_INSTALLATION.md)

## Start the local container

```sh
docker compose up --build --wait
docker compose exec -T shortcircuit shortcircuit setup-token
```

Open `http://localhost:8081` and enter the one-time token, a username, and a password. The token expires after 15 minutes. Use the admin interface to create short links, edit their destinations and tags, and provision scoped API keys. Local short links are served on port 8080. Local backups are available on demand and can be scheduled. [Backup and restore](docs/BACKUP_RESTORE.md)

The Analytics page shows UTC daily counts, period comparisons, and privacy settings. QR codes are available from the link list.

Compose builds the development image `shortcircuit:0.6.0-preview.2`, publishes both ports on loopback, and runs as UID/GID 10001. The application uses one persistent Docker volume. For a reverse proxy, set `ADMIN_ORIGIN` to the external HTTPS origin and keep the cleartext admin backend reachable only by the proxy. Use a dedicated admin hostname. [Network setup](docs/REVERSE_PROXY.md)

Upgrading an existing development instance applies the schema updates and preserves its link IDs, hosts, slugs, and destinations. Remove `ADMIN_TOKEN_FILE` and use `setup-token` to create the first administrator. Issue new scoped keys from the admin interface. For account recovery, use `docker compose exec -T shortcircuit shortcircuit recovery-token` and choose **Reset password** on the sign-in screen.

## Build from source

Use Rust 1.100.0 and Node 24.22.3. Build the admin assets before starting the Rust process.

```sh
npm ci --prefix frontend
npm run build --prefix frontend
cargo run --locked
```

Run `cargo run --locked -- setup-token` in a second shell, using the same `DATA_DIR` if configured. The Docker image embeds the frontend in the Rust binary. [API reference](docs/API.md) · [CI results](https://github.com/Buffer379x/shortcircuit/actions)

## Design preview

Open [prototype/index.html](prototype/index.html) in a browser. You can also serve the repository locally and open `http://127.0.0.1:8765/prototype/`.

```sh
python3 -m http.server 8765 --bind 127.0.0.1
```

The preview uses sample data and stores changes in your browser profile. To reset it, clear the preview's site data or use a fresh profile. Use the sidebar to navigate, open the sign-in preview from the profile button, and switch themes from the top bar. The preview runs offline without a build.

[Desktop dark](assets/design-renders/desktop-overview-dark.png) · [Desktop light](assets/design-renders/desktop-overview-light.png) · [Mobile dashboard](assets/design-renders/mobile-overview.png) · [Mobile link creation](assets/design-renders/mobile-create-link.png) · [Design system](docs/DESIGN_SYSTEM.md)

The PNGs above are design renders. To capture the interactive preview in Chromium, run the capture script with Node.js and Playwright. The following commands install Playwright in a temporary directory.

```sh
sc_capture_deps=$(mktemp -d)
npm install --prefix "$sc_capture_deps" --no-save playwright
"$sc_capture_deps/node_modules/.bin/playwright" install chromium
SHORTCIRCUIT_PLAYWRIGHT="$sc_capture_deps/node_modules/playwright" node prototype/capture.cjs
```

## Documentation

- [Project plan](docs/PROJECT_PLAN.md), [feature matrix](docs/FEATURE_MATRIX.md), and [roadmap](docs/ROADMAP.md)
- [Architecture](docs/ARCHITECTURE.md), [data model](docs/DATABASE.md), [API](docs/API.md), and [performance](docs/PERFORMANCE.md)
- [Threat model](docs/THREAT_MODEL.md), [security](docs/SECURITY.md), [privacy](docs/PRIVACY.md), and [backup and restore](docs/BACKUP_RESTORE.md)
- [Unraid](docs/UNRAID_INSTALLATION.md), [Nginx Proxy Manager](docs/REVERSE_PROXY.md), and [testing](docs/TESTING.md)
- [Decisions](docs/DECISIONS.md), [name and licensing](docs/NAME_AND_LICENSE.md), and [contributor guidance](CLAUDE.md)
