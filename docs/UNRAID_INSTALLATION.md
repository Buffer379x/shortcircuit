# Unraid installation

ShortCircuit runs as one container. The image uses UID and GID 10001, public port 8080, and admin port 8081. The Docker Manager template uses the locally built image. [Template XML](../unraid/shortcircuit.xml) · [Docker icon](../assets/branding/unraid-docker.png) · [SVG favicon](../assets/branding/favicon.svg) · [PNG favicon](../assets/branding/favicon.png)

## Prepare storage and networking

Select a local pool for SQLite. Use a direct path such as `/mnt/cache/appdata/shortcircuit` if the pool is named `cache`. A verified exclusive share can resolve directly to the same pool. Keep the database and its WAL on that filesystem and exclude the directory from Mover. Grant write access to the runtime UID and GID. [Unraid shares](https://docs.unraid.net/unraid-os/using-unraid-to/manage-storage/shares/) · [SQLite WAL](https://www.sqlite.org/wal.html)

Connect ShortCircuit and Nginx Proxy Manager to a shared user-defined Docker bridge. Restrict the admin hostname to the LAN or VPN. The example assumes an existing bridge named `shortcircuit-proxy`. On a custom VLAN, verify the firewall rules for IPv4 and IPv6. [Unraid container networking](https://docs.unraid.net/unraid-os/using-unraid-to/run-docker-containers/managing-and-customizing-containers/) · [Reverse proxy configuration](REVERSE_PROXY.md)

## Install with Docker Manager

1. Build the image on Unraid from the repository root with `docker build --tag shortcircuit:0.6.0 .`. Prepare the local appdata directory as shown below.
2. For a new installation, save the XML as `my-shortcircuit.xml` in `/boot/config/plugins/dockerMan/templates-user/`. In **Docker → Add Container**, select the ShortCircuit user template and set **Appdata** to your local pool path.
3. For tunnel access, keep the default bridge and admin origin. The two `--publish` options in **Extra Parameters** bind to IPv4 loopback. Use Docker Engine 28.0.0 or newer for this setup. Forward the ports from your client, then use `http://localhost:8081` for administration. [Docker port publishing](https://docs.docker.com/engine/network/port-publishing/)

   ~~~sh
   ssh -N -L 8081:127.0.0.1:8081 -L 8080:127.0.0.1:8080 root@unraid
   ~~~

4. For Nginx Proxy Manager, select the bridge shared with the proxy and remove the two `--publish` options. Set **Admin origin** and the Docker Manager **WebUI** field to your exact HTTPS admin URL. Set **Public hosts** to your redirect domains. Keep the admin hostname restricted to the LAN or VPN. [Proxy host configuration](REVERSE_PROXY.md)

To update a local build, rebuild the image under the configured tag and recreate the container with the same appdata path.

## Build and start from the command line

Use the Dockerfile from the revision you intend to run. Substitute the storage directory, bridge name, and DNS names for your installation.

~~~sh
docker build --tag shortcircuit:0.6.0 .

shortcircuit_data=/mnt/cache/appdata/shortcircuit
install -d -m 0700 -o 10001 -g 10001 "$shortcircuit_data"

docker run --detach --name shortcircuit \
  --network shortcircuit-proxy \
  --restart unless-stopped --stop-timeout 25 \
  --read-only --cap-drop ALL --security-opt no-new-privileges \
  --mount "type=bind,source=$shortcircuit_data,target=/data" \
  --env PUBLIC_HOSTS=go.example.com \
  --env ADMIN_ORIGIN=https://shortcircuit.example.com \
  shortcircuit:0.6.0
~~~

Reach both listeners through the proxy on the shared bridge. [Proxy host configuration](REVERSE_PROXY.md)

## First sign-in

1. Verify the private admin HTTPS hostname from your LAN or VPN.
2. Run `docker exec shortcircuit shortcircuit setup-token`. Enter the one-time token, username, and password on the admin hostname.
3. Open **Domains** to configure canonical origins and fallbacks. Preserve the hostname and case-sensitive slug when recreating an existing address.
4. Open **Analytics** to choose collection and retention. Take a verified backup after setup.

## Configuration

| Setting | Current behavior |
| --- | --- |
| `DATA_DIR` | Persistent directory. The image uses `/data`. |
| `PUBLIC_BIND` and `ADMIN_BIND` | Container listener addresses. The default binds are `0.0.0.0:8080` and `0.0.0.0:8081`. |
| `PUBLIC_HOSTS` | Comma-separated public hostnames. The first active host is the default for new links. |
| `ADMIN_ORIGIN` | Exact external origin for administration. Use HTTPS outside loopback. |
| `LOG_LEVEL` | Default `info`. |
| `BACKUP_INTERVAL_HOURS` and `BACKUP_KEEP` | Optional backup interval and number of scheduled copies to keep. Default scheduling is disabled. |

Take a verified backup before upgrading. Preserve the public hostnames, admin origin, and storage mount. Recover with the offline restore workflow on the same data volume. [Backup and restore](BACKUP_RESTORE.md)
