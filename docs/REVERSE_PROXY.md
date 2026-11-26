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
4. Apply client-level limits at the proxy as needed. ShortCircuit's login throttling uses the direct TCP peer. Review the proxy log format and retention to keep credentials and query values out of operational logs.
5. Sign in through the admin hostname from your LAN or VPN. Test access from outside and confirm that public redirects remain available. Add required aliases in **Domains** or `PUBLIC_HOSTS`.

## Verify redirects

Use HEAD to read the redirect status and exact `Location` without consuming a visit limit. Test a saved short link, an invalid path, and any configured fallback from the real client networks.

~~~sh
curl --silent --show-error --head https://go.example.com/your-slug
curl --silent --show-error --head https://go.example.com/api/v1/auth/session
~~~

The session API is on the admin listener. The second check shows the public missing-path behavior unless that address belongs to an existing short link. Retain the public hostname and exact path for existing QR codes.
