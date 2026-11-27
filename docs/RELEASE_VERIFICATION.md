# Release verification and rollback

This procedure verifies a release candidate (currently `0.8.0-preview.2`, upgrading from `0.8.0-preview.1`, `0.7.0-preview.1`, or `0.6.0-preview.2`) for the owner's Unraid host with Nginx Proxy Manager (NPM). It separates checks that run in CI or a disposable local environment from checks that need the target host, the real proxy, real domains, or a person with a specific device. A release, a new image or template tag, and any change to the live Unraid host, proxy, domains, or data require the owner's explicit approval. [Testing](TESTING.md) · [Unraid installation](UNRAID_INSTALLATION.md) · [Reverse proxy](REVERSE_PROXY.md) · [Backup and restore](BACKUP_RESTORE.md)

## What the candidate changes on upgrade

- Startup writes a verified snapshot to `/data/backups/pre-upgrade` and then applies schema migrations 12 through 24 to a preview.2 database (schema 11). Migrations 19, 20, and 21 rebuild `admin_api_keys`, `analytics_events`, `analytics_daily`, and `admin_audit` inside the migration transaction. The analytics rebuild temporarily needs free space comparable to the analytics tables. SQLite temporary files use `/data/tmp`, because the root filesystem is read-only. The candidate's diagnostics report free space on the data volume and mark it low below 512 MiB or three times the database size.
- New features stay off until configured: the destination policy (`Domains` page), title lookup (`TITLE_LOOKUP_ENABLED`), notifications (`NOTIFICATION_WEBHOOK_URL` or `_FILE`), and the internal connector (`NOTIFICATION_CONNECTOR_*`). Existing links, policies, protection, visit counters, and analytics keep their meaning.
- `MONITOR_WEBHOOK_URL` and `MONITOR_WEBHOOK_URL_FILE` still work as an alias for link-health notifications. The payload changes from one `link_health_changed` batch to one `link_health` event per link. Update a receiver that parses the old format.
- An older image cannot open a database that a newer image migrated, because SQLx rejects unknown applied migrations. Rollback therefore uses the pre-upgrade snapshot, not the upgraded volume.

## Disposable verification

Run these in CI or on a workstation with Docker. Use a separate Compose project name and volume, synthetic data, and loopback ports. [Core checks workflow](../.github/workflows/ci.yaml)

| Check | Command or workflow step | Acceptance |
|---|---|---|
| Source checks | The commands in [Testing](TESTING.md#verification-commands) | All pass on the same revision. |
| Clean start, listener separation, non-root | CI `container` job, steps up to "Check listener separation" | Admin `/readyz` is `200`, public `/readyz` is `404`, the unauthenticated link API is `401`, and the container user is `10001:10001`. |
| Browser workflow | `tests/admin-browser.cjs` | The script completes without page errors or CSP violations. |
| Backup and offline restore | CI step "Exercise backup and offline restore" | A live restore is refused, the stopped restore succeeds, and `/backup-check` still redirects to `https://example.test/backup-check`. |
| Upgrade from the published preview | CI step "Upgrade a disposable volume from the installed preview" with `tests/preview_upgrade.py` | The existing account signs in and the seeded link redirects to its exact destination after the upgrade. |
| Rollback rehearsal | Section [Rollback](#rollback) on a disposable volume | The older image starts on a volume restored from the pre-upgrade snapshot and serves the seeded link. |
| Image scan and SBOM | CI Trivy steps and the `shortcircuit-candidate-sbom` artifact | No fixed high or critical finding. Every unfixed high or critical finding is reviewed for applicability and recorded with a decision. The SBOM artifact exists for the tested revision. |
| Loopback smoke benchmark | CI step "Measure loopback redirect latency" | All requests return the expected status. Record P50, P95, P99, and memory. |

## Target host verification

Each check needs the owner's Unraid host, NPM, DNS, certificates, client devices, or a person. Record the image digest, schema version, date, and result for each row.

| Area | Procedure | Acceptance |
|---|---|---|
| Pre-upgrade snapshot | In the running preview, open **System and backups** and download a verified backup, or run `shortcircuit backup` in the container console. Copy the file outside the Docker data store. Note the image digest with `docker image inspect`. | A snapshot file outside the volume, the old digest, and the old template XML are stored together. |
| Storage placement | Confirm that `/data` is on a local pool or the Docker volume, not on SMB or NFS, and that free space exceeds three times the database size. | Diagnostics show the storage state **Sufficient** after the upgrade. |
| Upgrade | Edit the container in Docker Manager, change only **Repository** to the approved candidate tag or digest, and apply. Keep the volume, network, fixed IP, public hosts, and admin origin. | `/readyz` returns `200` within 60 seconds. The log names the pre-upgrade snapshot. Diagnostics show schema version 24 and the expected build revision. The existing administrator signs in. A burst of redirects leaves the dropped-event count at 0. |
| Exact published redirects | For each address that must keep working, run `curl --silent --show-error --head https://<public-host>/<exact-slug>` from outside the LAN, over IPv4 and IPv6 where the domain has AAAA records. | Status and `Location` match the recorded expectation byte for byte, including query and fragment. HEAD does not change any visit counter. |
| Proxy trust and client addresses | Set `TRUSTED_PROXIES` to the NPM container address or bridge network only. Make one protected-link password attempt from a known client. | The rate limit counts the client, not the proxy, and a direct request to the backend from another container cannot set the client address with `X-Forwarded-For`. |
| Listener routing | From outside the LAN, request `https://<public-host>/api/v1/auth/session` and the admin hostname. | The public host returns the missing-link response. The admin hostname is unreachable from outside the LAN or VPN. |
| Backup and restore on the target | Download a backup from the browser, stage it with **Restore**, restart the container, and verify a known link. | The restart applies the staged snapshot, the audit log records the restore, sessions require sign-in again, and pending notifications are discarded. |
| Notifications, if enabled | Configure the webhook, send a test from **System and backups**, then stop the receiver and send another. | The first test shows **Delivered**. The second shows retries with growing delays and finally **Failed** after eight attempts, about two hours later. The receiver can see a delivery twice after an interrupted attempt and should deduplicate by `id`. |
| LAN benchmark | Follow [Performance](PERFORMANCE.md) from a LAN client through NPM and directly to the backend. | Record P50, P95, P99, errors, idle process RSS, container memory, idle CPU, startup time, and image size with the hardware details. |
| Mobile Safari | Sign in, create a link, copy it, open the QR dialog, and open the short URL on an iPhone. | Every step completes without layout overflow, and the QR code scans to the exact short URL. |
| Screen reader and keyboard | With VoiceOver or NVDA and the keyboard only, sign in, create and edit a link, change a policy, and run a backup authorization. | Every control has an accessible name, focus is visible and returns after dialogs, and status and error messages are announced. |

## Rollback

1. Stop the candidate container. Do not start an older image on the upgraded volume.
2. Copy the snapshot from `/data/backups/pre-upgrade` or your own pre-upgrade backup, make it readable by UID 10001 (for example `chmod 644`), create a new volume or appdata directory, and restore it with the older image: `docker run --rm --user 10001:10001 -v <new-volume>:/data -v <snapshot-dir>:/restore:ro <old-image> restore /restore/<snapshot>.sqlite --confirm`.
3. Start the older image with the new volume and the old template settings. Verify `/readyz`, sign-in, and the exact published redirects before moving proxy traffic back.
4. Keep the upgraded volume untouched until the cause is understood. Links and analytics created after the snapshot exist only there.

## Stable artifact checks

The core workflow now also reports unfixed high and critical findings without failing, generates a CycloneDX SBOM of the candidate image, and keeps it as a workflow artifact for 30 days. The release workflow generates an SBOM for the published digest and attaches it to the GitHub release with an `image-digest.txt` file. Base images in the Dockerfile are pinned by digest. The binary is built with `cargo auditable`, so the image SBOM and scans include the Rust crates, and a CycloneDX list of the bundled frontend packages ships at `/usr/share/doc/shortcircuit/frontend-sbom.cdx.json`. The release SBOM job runs without write permissions. Updating a base image is a reviewed change that updates the digest. Signing images and attesting provenance remain open owner decisions.
