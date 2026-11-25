# Nginx Proxy Manager Configuration Design

Configure separate TLS-terminating proxy hosts for the public and admin listeners. Keep connections from the proxy to ShortCircuit on a dedicated network. Apply the appropriate network access rules to each host. This is the planned deployment and requires verification before production use.

| Proxy host | Scheme and upstream | Access |
| --- | --- | --- |
| `go.example.com` | `http` → `shortcircuit:8080` | Public access for configured hosts and redirect paths |
| `shortcircuit.example.com` | `http` → `shortcircuit:8081` | LAN or VPN access with application authentication |

1. Add a proxy host and certificate for each hostname in Nginx Proxy Manager (NPM). Enforce HTTPS after verifying DNS and TLS on both hosts. Explicitly configure any public DNS aliases needed by manually recreated links on the redirect listener.
2. Resolve the admin hostname through split DNS or the VPN and restrict access at the proxy or network. If the proxy is shared with public services, restrict the admin host with an Access List and independently verified IPv4 and IPv6 rules. Keep the admin listener port internal.
3. Use a shared internal Docker network. Trust only the dedicated NPM address or the smallest required CIDR range. Configure NPM to set the `Host` header and forwarded HTTPS scheme and to derive client forwarding headers from the verified proxy chain. Ignore or overwrite forwarding headers supplied by external clients. Inspect the generated Nginx configuration and verify the policy for any upstream proxy chain.
4. The planned admin configuration binds cookies to the HTTPS origin set with `ADMIN_BASE_URL=https://...` and generates absolute URLs from stored configuration. Verify that admin routes and setup functions are reachable only through the private listener.
5. Keep query tokens and destination URLs out of access logs. For sensitive shortcodes, review the paths the reverse proxy logs and how long it retains them. Verify that proxy redirects and rewrite rules preserve encoded path and query components on manually recreated URLs.
6. Before switching traffic, test both hostnames using temporary DNS resolution and verify A and AAAA routes and TLS. Check the redirect status and raw `Location` value for encoded slugs. Check for redirect loops, verify Secure cookies on the private host, and test rejection of unconfigured hosts and enforcement of IPv6 network boundaries. Verify the planned rollback path after the switch.

Same-domain operation is a possible later option and requires a path collision check before activation. [Nginx Proxy Manager Advanced Configuration](https://nginxproxymanager.com/advanced-config/) · [Nginx Proxy Module](https://nginx.org/en/docs/http/ngx_http_proxy_module.html)
