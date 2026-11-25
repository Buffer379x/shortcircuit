# Unraid Installation Design

ShortCircuit provides a Dockerfile and a local Compose profile. The Unraid XML template, a verified image, and an installation test on Unraid are release gates. See the [project status](PROJECT_STATUS.md).

- Run one ShortCircuit container as an unprivileged user. Drop capabilities, leave the Docker socket unmounted, and use a read-only root filesystem where possible. Include the required timezone and CA data in the runtime. Deploy a pinned OCI version tag or digest, with amd64 as the first target.
- Mount persistent storage at `/data` and grant write access to the runtime UID and GID. The Dockerfile uses UID and GID 10001. Prefer `/mnt/<poolname>/appdata/shortcircuit` on a suitable local pool. A verified exclusive share at `/mnt/user/appdata/shortcircuit` can resolve directly to the same pool. Keep the active database on local storage and exclude it from Mover and concurrent access by other processes. [Unraid Shares](https://docs.unraid.net/unraid-os/using-unraid-to/manage-storage/shares/) · [SQLite WAL](https://www.sqlite.org/wal.html)
- Use internal container ports `8080` and `8081` on a custom network shared with Nginx Proxy Manager. Serve the interface on the admin hostname. If bridge port publishing is needed, bind it to an internal IP. Verify the firewall rules for VLANs, IPv4 and IPv6, and direct access to container IPs.
- The server reads `PUBLIC_BIND`, `ADMIN_BIND`, `ADMIN_ORIGIN`, `PUBLIC_HOSTS`, `DATA_DIR`, and `LOG_LEVEL`. Use the listeners on a restricted network behind the proxy. Connection limits and header deadlines require implementation and testing before direct internet exposure.
- Planned release settings include `DATA_DIR`, `PUBLIC_PORT`, `ADMIN_PORT`, and `LOG_LEVEL`, with optional initial values for `PUBLIC_BASE_URL`, `ADMIN_BASE_URL`, and `TRUSTED_PROXIES`. Set the application and display timezones during setup. Provide secrets through the controlled local CLI workflow or an explicitly supported secret file. Verify the settings after implementation.
- The XML template should include the name, repository or registry, description, category, admin web interface, both ports, appdata, essential variables, icon, support link, and project link. Verify the template in a reproducible installation and check the icon at small sizes. Add support and image URLs after the owner, repository, and publication are approved.
- Use the `unless-stopped` restart policy, defined internal health checks, and sufficient shutdown time. Verify a backup before upgrading. Test upgrades against a database from the previous schema version, including migration, return to service, and restore.

For LAN or VPN access through Nginx Proxy Manager, see the [reverse proxy design](REVERSE_PROXY.md).

## Planned First Run

1. Prepare the local appdata location and private admin access. Activate redirect domains only after successful setup.
2. Generate a short-lived, single-use setup token through the local CLI. Use HTTPS on the private admin host to enter the token and configure the administrator, public domain, admin origin, language, and timezone.
3. Verify the proxy and host settings using redacted status details. Show the privacy defaults and offer backup settings and optional file import. The operator can start an import later.
4. Complete setup and consume the token in a single transaction, then create the new admin session. Failed setup leaves the account uncreated and admin access restricted.
