# Unraid installation

ShortCircuit runs in one container on `linux/amd64`. Docker Manager pulls `ghcr.io/buffer379x/shortcircuit:0.6.0-preview.1` and creates a persistent Docker volume on first start. The image uses UID and GID 10001, public port 8080, and admin port 8081. [Download the template](https://github.com/Buffer379x/shortcircuit/releases/download/v0.6.0-preview.1/my-shortcircuit.xml) · [Docker icon](../assets/branding/unraid-docker.png) · [SVG favicon](../assets/branding/favicon.svg) · [PNG favicon](../assets/branding/favicon.png)

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

If you already have data in an appdata bind mount, edit the existing container and change only **Repository** to the published image reference. Preserve the existing `/data` mapping. For a new bind mount, remove `--volume=shortcircuit-data:/data` and add a path mapping to `/data`. Use a local pool directory writable by UID/GID 10001. [Unraid shares](https://docs.unraid.net/unraid-os/using-unraid-to/manage-storage/shares/) · [SQLite WAL](https://www.sqlite.org/wal.html)

Take a verified backup before changing the version in **Repository**. Preserve the `/data` mount, public hostnames, and admin origin when updating.

## Configuration

| Setting | Current behavior |
| --- | --- |
| `DATA_DIR` | Persistent directory. The image uses `/data`. |
| `PUBLIC_BIND` and `ADMIN_BIND` | Container listener addresses. The default binds are `0.0.0.0:8080` and `0.0.0.0:8081`. |
| `PUBLIC_HOSTS` | Comma-separated public hostnames. The first active host is the default for new links. |
| `ADMIN_ORIGIN` | Exact external origin for administration. Use HTTPS outside loopback. |
| `LOG_LEVEL` | Default `info`. |
| `BACKUP_INTERVAL_HOURS` and `BACKUP_KEEP` | Optional backup interval and number of scheduled copies to keep. Default scheduling is disabled. |

