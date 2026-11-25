# ShortCircuit

ShortCircuit is a self-hosted URL shortener and link manager. It uses Rust and SQLite and is designed to run in one Docker container. You can recreate an existing short URL by configuring its original host and creating a link with the same slug.

**0.1.0 is in development.** ShortCircuit is licensed under the [MIT License](LICENSE). See the [project plan](docs/PROJECT_PLAN.md) and [verification status](docs/PROJECT_STATUS.md).

## Local development

The development server binds both listeners to loopback by default. Enable link creation and listing on the admin listener with a locally generated token file. The full admin sign-in flow is planned for 0.2.

Keep this build on loopback or an isolated test network. Production use requires the network and HTTP hardening listed in the [security guide](docs/SECURITY.md).

```sh
umask 077
openssl rand -hex 32 > admin-token
ADMIN_TOKEN_FILE=admin-token cargo run
```

Create a sample link and inspect the redirect from a second shell in the repository.

```sh
umask 077
printf 'Authorization: Bearer %s\n' "$(cat admin-token)" > admin-header
curl --fail-with-body --header @admin-header --json '{"slug":"handbook","target":"https://example.org/handbook"}' http://localhost:8081/api/v1/links
curl --head http://localhost:8080/handbook
```

`docker compose up --build` runs the server with a persistent local Docker volume and publishes both ports on the host's loopback interface. The container runs as UID/GID 10001. The default Compose file leaves the link administration API disabled. To enable it, configure `ADMIN_TOKEN_FILE` and mount a token file readable only by the runtime user. [API reference](docs/API.md) · [CI results](https://github.com/Buffer379x/shortcircuit/actions)

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
