# Unraid installation

ShortCircuit runs in one container on `linux/amd64`. Docker Manager pulls `ghcr.io/buffer379x/shortcircuit:0.8.0-preview.2` and creates a persistent Docker volume on first start. The image uses UID and GID 10001, public port 8080, and admin port 8081. [Download the template](https://github.com/Buffer379x/shortcircuit/releases/download/v0.6.0-preview.2/my-shortcircuit.xml) · [Docker icon](../assets/branding/unraid-docker.png) · [SVG favicon](../assets/branding/favicon.svg) · [PNG favicon](../assets/branding/favicon.png)

## Install with Docker Manager

1. Copy `my-shortcircuit.xml` to `/boot/config/plugins/dockerMan/templates-user/` using the Unraid file manager or an existing flash share. In **Docker → Add Container**, select the `shortcircuit` user template. [Unraid templates](https://docs.unraid.net/community-applications/)
2. For Nginx Proxy Manager, select the Docker bridge shared with the proxy and remove both `--publish` entries from **Extra Parameters**. Set **Admin origin** and **WebUI** to your exact HTTPS admin URL, and **Public hosts** to your redirect hostnames. Limit the admin hostname to your LAN or VPN. If the container has its own LAN IP, follow the next section instead. [Proxy host configuration](REVERSE_PROXY.md)
3. Click **Apply**. Docker Manager downloads the image and starts the container. Follow the first sign-in steps below.

For an isolated install, keep the default bridge and IPv4 loopback bindings. Use Docker Engine 28.0.0 or newer with these bindings. Forward the two ports from your client with an SSH tunnel, then open `http://localhost:8081`. [Docker port publishing](https://docs.docker.com/engine/network/port-publishing/)

~~~sh
ssh -N -L 8081:127.0.0.1:8081 -L 8080:127.0.0.1:8080 root@unraid
~~~

## Use a fixed LAN IP

1. In the Unraid container editor, keep the custom network and assigned IP. In **Advanced View**, remove both `--publish` entries from **Extra Parameters**. Retain the existing `/data` mount.
2. In Nginx Proxy Manager, add a proxy host for your private admin hostname. Set the forward scheme to `http`, the forward IP to the container's assigned IP, and the forward port to `8081`. Assign a valid TLS certificate and restrict the hostname to your LAN or VPN. The admin hostname should resolve to the proxy.
3. In Unraid, set **Admin origin** and **WebUI** to the exact browser address, such as `https://shortcircuit.example.com`. Apply the changes and open that address. For public redirects, forward the public hostname to port `8080` and include the hostname in **Public hosts**.

Plain HTTP in **Admin origin** is allowed only for loopback addresses. An `http://` LAN address there causes startup to fail with `ADMIN_ORIGIN requires HTTPS outside loopback`. The default `http://localhost:8081` is for local or tunneled access. From a LAN client, `http://<container-ip>:8081/readyz` tests the backend connection without opening the admin interface. If the proxy returns `502`, check whether it can reach the custom network. A shared user-defined Docker bridge is an alternative when the proxy cannot reach the LAN IP. [Unraid container networking](https://docs.unraid.net/unraid-os/using-unraid-to/run-docker-containers/managing-and-customizing-containers/) · [Proxy host configuration](REVERSE_PROXY.md)

## First sign-in

1. Open the admin HTTPS hostname from your LAN or VPN, or use the loopback address through your tunnel.
2. After the container starts, click its icon in the Docker tab and open **Console**. Run `shortcircuit setup-token`. Enter the one-time token, username, and password in the setup form.
3. Open **Domains** to configure canonical origins and fallbacks. Preserve the hostname and case-sensitive slug when recreating an existing address.
4. Open **Analytics** to choose collection and retention. Take a verified backup after setup.

## Persistent storage and updates

The named volume `shortcircuit-data` persists across container replacement. It resides in Docker's data store. Export a verified backup outside that store before recreating it. Use a separate volume name if you install a second ShortCircuit instance. [Docker volumes](https://docs.docker.com/engine/storage/volumes/) · [Backup and restore](BACKUP_RESTORE.md)

For a new appdata bind mount, remove `--volume=shortcircuit-data:/data` and add a path mapping to `/data`. Use a local pool directory writable by UID/GID 10001. [Unraid shares](https://docs.unraid.net/unraid-os/using-unraid-to/manage-storage/shares/) · [SQLite WAL](https://www.sqlite.org/wal.html)

To upgrade from 0.8.0-preview.1 or 0.7.0-preview.1, download a verified backup on **System › Backups** and copy it outside the Docker data store. Note the current image digest. In Docker Manager, edit the existing container, set **Repository** to `ghcr.io/buffer379x/shortcircuit:0.8.0-preview.2`, and click **Apply**. Keep the existing `/data` mount, network and any fixed IP, public hostnames, and admin origin. 0.8.0-preview.2 keeps schema version 24, so no migration runs. 0.8.0-preview.1 brought the Polar Signal interface and additive API fields. From 0.6.0-preview.2, startup first writes a verified snapshot to `/data/backups/pre-upgrade` and migrates schema 11 to 24. Optionally add **Trusted proxies** and **Backup interval** from the updated template: an existing container keeps its own settings.

For rollback to 0.8.0-preview.1 or 0.7.0-preview.1, set **Repository** back to the noted digest; all of them use schema 24, so the volume stays usable. For a rollback to 0.6.0-preview.2, follow [Release verification](RELEASE_VERIFICATION.md#rollback): restore the pre-upgrade snapshot into a separate data volume with that image, because it refuses an upgraded volume. Verify the restored container and its links before moving traffic to it. [Backup and rollback](BACKUP_RESTORE.md)

## Configuration

| Setting | Current behavior |
| --- | --- |
| `DATA_DIR` | Persistent directory. The image uses `/data`. |
| `SQLITE_TMPDIR` | SQLite temporary files. The image after preview.2 sets `/data/tmp`, because the root filesystem is read-only. Keep the default unless `/data` moves. |
| `PUBLIC_BIND` and `ADMIN_BIND` | Container listener addresses. The default binds are `0.0.0.0:8080` and `0.0.0.0:8081`. |
| `PUBLIC_HOSTS` | Comma-separated public hostnames. The first active host is the default for new links. |
| `ADMIN_ORIGIN` | Exact external origin for administration. Use HTTPS outside loopback. |
| `LOG_LEVEL` | Default `info`. |
| `SHORTCODE_LENGTH` | Generated shortcode length from 8 to 32. Defaults to 10. Added after preview.2. |
| `SQLITE_BUSY_TIMEOUT_MS` | SQLite writer wait in milliseconds from 1 to 10,000. Defaults to 2,000. |
| `TRUSTED_PROXIES` | Comma-separated trusted proxy IP addresses or CIDR networks. Empty by default. |
| `MONITOR_ENABLED` | Set `true` to allow explicit monitor enrollment. Defaults to `false`. |
| `MONITOR_INTERVAL_HOURS` | Target interval for stable links, 1–720 hours. Defaults to 24. |
| `MONITOR_PRIORITY_HOURS` | Target interval for priority links. Defaults to 6 hours or the regular interval if shorter. |
| `MONITOR_WEBHOOK_URL_FILE` | Alias for `NOTIFICATION_WEBHOOK_URL_FILE` that selects only link-health events. Use this or `MONITOR_WEBHOOK_URL`, not both, and not together with `NOTIFICATION_WEBHOOK_URL`. |
| `NOTIFICATION_WEBHOOK_URL_FILE` | Read-only file with the HTTPS notification webhook URL. Setting it, or `NOTIFICATION_WEBHOOK_URL`, activates notifications. Added after preview.2. |
| `NOTIFICATION_EVENTS` | Comma-separated event kinds: `link_health`, `link_expiring`, `link_expired`, `click_milestone`, `backup_failed`, `job_failed`, `storage_low`, `security`. Defaults to all kinds except `click_milestone` unless milestones are set. |
| `NOTIFICATION_CLICK_MILESTONES` | Up to 10 redirect counts, for example `100,1000`. Empty by default. |
| `NOTIFICATION_EXPIRY_NOTICE_HOURS` | Hours before an expiry for `link_expiring`, 1–720. Defaults to 24. |
| `NOTIFICATION_WEBHOOK_AUTHORIZATION_FILE` | Optional read-only file with an `Authorization` header value for the webhook. |
| `NOTIFICATION_CONNECTOR_ORIGIN`, `NOTIFICATION_CONNECTOR_NETWORKS`, `NOTIFICATION_CONNECTOR_CA_FILE` | Optional internal connector: the exact HTTPS origin, permitted CIDR ranges, and an optional PEM CA file. Required only for a receiver on a private network. |
| `TITLE_LOOKUP_ENABLED` | Set `true` to allow administrators to fetch page titles for untitled links. Defaults to `false`. Added after preview.2. |
| `BACKUP_INTERVAL_HOURS` and `BACKUP_KEEP` | Optional backup interval and number of scheduled copies to keep. Default scheduling is disabled. |


Settings marked as added after preview.2, and the monitor, notification, title lookup, and proxy settings, apply to source builds after preview.2. The published preview.2 image ignores them. [Monitoring and rate limits](SECURITY.md#implemented-outbound-monitoring-policy)

## Storage, proxy, and ports

- **Keep data outside `docker.img`.** The template stores `/data`, every backup, and every rollback copy in the named volume `shortcircuit-data` inside Docker's data store. Recreating `docker.img` deletes all of them. Map `/data` to a local appdata path such as `/mnt/<pool>/appdata/shortcircuit` owned by UID and GID 10001, or copy backups from `/data/backups` to other storage regularly. Set `BACKUP_INTERVAL_HOURS` to enable scheduled backups.
- **Trust the proxy.** Behind Nginx Proxy Manager, add the variable `TRUSTED_PROXIES` with the proxy container address or its bridge network, for example `172.18.0.0/16`. Without it, every client shares the proxy's rate-limit budget for sign-in and protected links.
- **Changed ports.** If you publish other host ports, set **Admin origin** and **WebUI** to the same address, and set the domain origin on **Domains**, so copied and QR short URLs use the published port.

- **Admin page says the administration isn't available at this address.** The browser address must match **Admin origin** exactly, including `localhost` versus `127.0.0.1` and the port. The log records the first mismatched request with the expected origin.

Since 0.7.0-preview.1 the template enables daily scheduled backups and offers the **Trusted proxies** field. It keeps the Docker volume by default, because a new appdata directory is owned by `nobody:users` and the container runs as UID 10001.
