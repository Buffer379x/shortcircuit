# Unraid installation

ShortCircuit runs as one container. The image uses UID and GID 10001, public port 8080, and admin port 8081. The Unraid template and a test on Unraid are release work. [Project repository](https://github.com/Buffer379x/shortcircuit)

## Prepare storage and networking

Select a local pool for SQLite. Use a direct path such as `/mnt/cache/appdata/shortcircuit` if the pool is named `cache`. A verified exclusive share can resolve directly to the same pool. Keep the database and its WAL on that filesystem and exclude the directory from Mover. Grant write access to the runtime UID and GID. [Unraid shares](https://docs.unraid.net/unraid-os/using-unraid-to/manage-storage/shares/) · [SQLite WAL](https://www.sqlite.org/wal.html)

Connect ShortCircuit and Nginx Proxy Manager to a shared user-defined Docker bridge. Restrict the admin hostname to the LAN or VPN. The example assumes an existing bridge named `shortcircuit-proxy`. On a custom VLAN, verify the firewall rules for IPv4 and IPv6. [Unraid container networking](https://docs.unraid.net/unraid-os/using-unraid-to/run-docker-containers/managing-and-customizing-containers/) · [Reverse proxy configuration](REVERSE_PROXY.md)

## Build and start

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
