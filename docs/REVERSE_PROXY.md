# Nginx Proxy Manager configuration

Configure independent TLS proxy hosts for the public and admin listeners. Use your DNS names in `PUBLIC_HOSTS=go.example.com` and `ADMIN_ORIGIN=https://shortcircuit.example.com`. Connect Nginx Proxy Manager and ShortCircuit to the same user-defined Docker bridge. [Nginx Proxy Manager networking](https://nginxproxymanager.com/advanced-config/) · [Unraid installation](UNRAID_INSTALLATION.md)

| Proxy host | Forward scheme | Forward hostname | Forward port | Access |
| --- | --- | --- | --- | --- |
| `go.example.com` | `http` | `shortcircuit` | `8080` | Public |
| `shortcircuit.example.com` | `http` | `shortcircuit` | `8081` | LAN or VPN |

## Add the proxy hosts

1. In **Hosts → Proxy Hosts**, add each hostname with the corresponding upstream. Disable proxy caching for these hosts to apply link and policy changes immediately.
2. Assign a valid certificate to each host and enable **Force SSL** after testing HTTPS. Resolve the admin hostname within the LAN or VPN and use an Access List or network rules to restrict it. Verify IPv4 and IPv6 access separately.
3. Preserve the incoming host and request URI. Verify encoded paths and query strings after any custom rewrite. ShortCircuit validates browser mutations against the exact `ADMIN_ORIGIN` and issues Secure cookies for HTTPS. Use port 443 unless you have verified that the proxy preserves the alternate port in the Host header. [Nginx Proxy Manager forwarding](https://github.com/NginxProxyManager/nginx-proxy-manager/blob/develop/docker/rootfs/etc/nginx/conf.d/include/proxy.conf)
4. Apply client-level limits at the proxy as needed. Configure `TRUSTED_PROXIES` only for the explicitly trusted addresses that can reach the backends. Configure each trusted ingress to overwrite `X-Forwarded-For` with the observed client address or append the observed sender when preserving a trusted chain. Review the proxy log format and retention to keep credentials and query values out of operational logs.
5. Sign in through the admin hostname from your LAN or VPN. Test access from outside and confirm that public redirects remain available. Add required aliases in **Domains** or `PUBLIC_HOSTS`.

## Unraid with a fixed container IP

On Unraid the container often has its own address on a custom network such as `br0`. NPM then cannot resolve the name `shortcircuit`. Enter the container address instead:

| Proxy host | Forward scheme | Forward hostname / IP | Forward port |
| --- | --- | --- | --- |
| `go.example.com` | `http` | `192.168.1.50` | `8080` |
| `shortcircuit.example.com` | `http` | `192.168.1.50` | `8081` |

The **Details** tab is all ShortCircuit needs. NPM already sends `Host`, `X-Real-IP`, `X-Forwarded-For`, and `X-Forwarded-Proto` for every proxy host. A custom `location /` block in the **Advanced** tab replaces NPM's own location and also works, but it is not required, and it hides settings from the NPM interface.

Set `TRUSTED_PROXIES` in the container to the address ShortCircuit sees for NPM: the NPM container's fixed IP, or the Unraid host address when NPM runs in bridge or host mode. Without it, every visitor shares NPM's address. Visitors then share one budget for password attempts and sign-in, and analytics cannot tell clients apart.

| Symptom | Likely cause |
| --- | --- |
| NPM shows `502 Bad Gateway` | Wrong IP or port, or NPM cannot reach the custom network. A container on `br0` (macvlan) is not reachable from the Unraid host or from bridge-mode containers unless **Settings › Docker › Host access to custom networks** is enabled. Alternatively, put NPM on the same custom network. |
| ShortCircuit's unknown-domain page | The hostname is not in **Domains** or `PUBLIC_HOSTS`, or a custom rewrite changed the `Host` header. |
| ShortCircuit's missing-link page | The host is known but the slug does not exist, is inactive, or the path was rewritten. Check the exact path with `curl --head`. |
| The admin address shows a page asking to open the configured administration address | The hostname in the browser differs from the host in `ADMIN_ORIGIN`. |
| Sign-in and saving fail with `403` (`invalid_origin`) | The scheme or port in the browser differs from `ADMIN_ORIGIN`, for example `http://` in `ADMIN_ORIGIN` while the proxy serves `https://`. |
| Many visitors hit "Too many attempts" together | `TRUSTED_PROXIES` is missing or does not contain NPM's address. |

## Verify redirects

Use HEAD to read the redirect status and exact `Location` without consuming a visit limit. Test a saved short link, an invalid path, and any configured fallback from the real client networks.

~~~sh
curl --silent --show-error --head https://go.example.com/your-slug
curl --silent --show-error --head https://go.example.com/api/v1/auth/session
~~~

The session API is on the admin listener. The second check shows the public missing-path behavior unless that address belongs to an existing short link. Retain the public hostname and exact path for existing QR codes.
