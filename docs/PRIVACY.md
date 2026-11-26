# Privacy and Analytics

This design assumes ShortCircuit can run on its own. The visitor's browser, the proxy, and the destination server are separate systems. The operator decides why data is collected, what to collect, what to disclose to visitors, who can access the data, how long to retain it, and how to delete it. Privacy-protective defaults and export and deletion tools support those decisions. The GDPR addresses purpose limitation and data minimisation in Article 5, the right to erasure in Article 17, and data protection by design and by default in Article 25. [GDPR on EUR-Lex](https://eur-lex.europa.eu/eli/reg/2016/679/oj)

## Current implementation

The current application exposes analytics in the admin UI. It records successful GET redirects, blocked links, and unmatched non-root requests on configured public domains. It excludes HEAD and recognizes `DNT: 1` by default. Per-link opt-out covers matched redirects and blocks. A configured fallback for an unmatched path is reported as an unmatched request.

Detail records contain time, host, the optional link ID, outcome, estimated visit class, a fixed client category, and an optional DNS hostname from the referrer. The collector discards literal-IP referrers, credentials, path, query, fragment, raw user agent, and visitor IP. The dashboard shows UTC daily rollups. Collection options, retention, and clear-all apply locally. The admin API lists and deletes retained visits. Removing a link also removes its detailed visits and daily link counts. Geolocation and analytics exports are planned.

The bounded queue has capacity for 1,024 events. It writes in batches of up to 64 events and gives the writer two seconds to drain after HTTP shutdown. The dashboard counts events lost to queue congestion and failed batch commits since startup. An abrupt stop or expired flush can lose events without a count.

## Data Collection

| Data or feature | Proposed default for new instances | Configuration |
| --- | --- | --- |
| Core analytics | Enabled on the server. Record successful visits with a UTC timestamp, link and domain, and broad visit class. | Disable globally or per link. Operational access counts work independently. |
| IP addresses | Process full IPs briefly in memory for rate limits, authorized network rules, and local geolocation lookups. | Optionally retain a shortened prefix for an explicitly defined purpose under its own retention policy. Explain IPv4 and IPv6 handling in the operator interface. Prefixes may still identify people. |
| User agent | Derive browser family, operating system family, device category, and bot class with visible uncertainty. Discard the raw user agent. | Enable or disable individual classifications. Keep missing or unusable values as unknown. |
| Referrer | Keep the validated hostname only. Treat missing or unparseable values as unknown. | Referrer collection can be disabled. Hostnames can contain sensitive information and are subject to retention and export controls. |
| Unmatched requests and errors | Record the domain, time bucket, and reason. Discard arbitrary unmatched paths and query strings. | Disable individual error classes. Aggregate to handle potentially high cardinality and stay within the write budget. |
| Geo-IP | Disabled. Opt in to local MMDB lookups for country and region. City data requires its own opt-in. | The operator selects and updates the local database. Visitor IPs stay local. |
| Tracking cookies | Disabled. Private sessions use session cookies. Protected link confirmations use a short-lived signed form field. | The v1 design excludes client-side analytics and fingerprinting. |
| Do Not Track | Honor a recognized `DNT: 1` header by suppressing the analytics event. | Document this as a configurable product setting. Security checks and operational access limits continue to apply. |
| Telemetry and outbound requests | Disable telemetry, update reporting, monitoring, title retrieval, and provider integrations by default. | Before activation, describe the purpose, destination, and transmitted fields for each optional integration. |

Destination URLs can contain confidential query parameters, paths, and fragments. Restrict complete URLs to authorized users. Apply the same controls to dashboards, job diagnostics, logs, audit records, notifications, API errors, and reverse proxy configuration. An active health check sends the destination server the requested URL, including its query string, and can consume one-time tokens. Enable health checks separately and provide per-link exceptions. During a browser redirect, the visitor connects to the destination server. Set a suitable referrer policy on the public response.

## Counting and Reporting Visits

- **All Visits** covers recorded successful ShortCircuit visits of every class. **Human Visits** is an estimate. **Bot Visits** includes detected bots, crawlers, link previews, and monitoring traffic, with filters for supported groups. **Unknown Visits** covers visits that cannot be reliably classified. Report explicit outcomes such as blocked or unmatched requests in their own metrics.
- A GET redirect counts as a visit when ShortCircuit returns a valid destination and collection is enabled. For a protected link, count the confirmed redirect. Exclude HEAD from visit counts and optionally record it in an independently configurable operational metric. Internal health jobs generate no public visit events.
- Access limits and one-time use depend on an atomic authorization decision independent of analytics. Loading the confirmation page and failed form submissions do not register as visits. A scanner that submits the form can consume a passwordless one-time link. Use password protection when access must depend on a separate shared secret.
- Manually recreating an existing short link starts a new analytics history on ShortCircuit.
- Analytics events can be lost when the queue fills, the process terminates, the flush deadline expires, or the database becomes unwritable. Show loss and error counters with affected time ranges when available and redact sensitive details. Mark any unmeasurable losses after an abrupt termination as unknown. Treat click rates and comparisons as estimates under these conditions.

## Retention, Export, and Deletion

The proposed retention periods are 90 days for visit details and 24 months for daily aggregates, both configurable. Indefinite daily retention requires an explicit choice. Authorize exports by time range, domain, and data type, and make CSV values safe for spreadsheets. The API returns the stored granularity and the data's provenance. Use aggregates for longer time ranges and label whether the rollups use UTC days or finer intervals. Show the effective precision and grouping for the selected display timezone and its daylight saving rules, especially when only UTC daily aggregates remain.

Manually deleting an available visit event adjusts its detail and aggregate counts in one transaction. Routine detail expiration retains the daily aggregates already created. Once details expire, deletion operates on the corresponding aggregate range. Clearing all analytics removes detail records, rollups, and queued events while preserving operational access counts. History and audit records have their own retention policies. Define the order of concurrent imports, exports, and deletion requests. Older backups can still contain previously deleted data. The operator's procedure covers backup retention, deletion of affected backup generations, and reapplying deletion requests after restore. See [backup and restore](BACKUP_RESTORE.md) for protected artifacts and configuration.

Link history can retain earlier destinations, including tokens in URL parameters. The application exposes up to 25 revisions from the last 30 days through an authenticated administrator browser session. Use **Clear history** to remove a link's revisions immediately from the live database. Review downloaded history files and backups separately when removing sensitive destinations.

## Local Geo-IP and Integrations

Optional local MaxMind GeoLite lookups use a file supplied by the operator. Under the EULA reviewed for this design, MaxMind requires database updates, deletion of superseded versions within 30 days of a new release, and compliance with the applicable use, disclosure, and attribution terms. Keep GeoLite files out of images, release artifacts, and long-retained ShortCircuit backups. Provide a fresh file separately after a restore. Geolocation values are approximate and have their own retention settings. An explicitly configured download contacts the provider using the operator's account. Visitor lookups stay local. [MaxMind GeoLite EULA](https://www.maxmind.com/en/geolite/eula) · [MaxMind GeoLite Documentation](https://dev.maxmind.com/geoip/geolite2-free-geolocation-data/)
