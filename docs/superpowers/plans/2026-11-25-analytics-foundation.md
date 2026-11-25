# Analytics foundation

The public listener registers a successful GET redirect, an unmatched GET on a configured domain, or a blocked GET. HEAD contributes zero analytics. The access counter for a limited link remains transactional and independent of analytics. A lost analytics event cannot undo an allowed redirect or add capacity to a limited link.

## Delivery order

1. Add checked SQLite settings and the per-link switch. Store detailed visit events and UTC-day rollups in one transaction. Leave a single bounded writer responsible for persistence and retention.
2. Route analytics on the admin listener and restrict the reporting, preferences, and deletion endpoints to administrator sessions. Return period comparisons, UTC daily series, visit classes, and a bounded list of top links.
3. Attach the optional queue after a redirect or failed lookup is classified. Hold no request-specific data in the queue beyond the selected fields. Respect Do Not Track by default. Omit raw IP addresses and user-agents and drop sensitive URL components.
4. Add an Analytics page with English and German interface text. Show the observed loss counter and active data retention windows.
5. Verify controlled concurrent redirects, access counters, queue overflow, stale events after deletion, per-link opt-out, expired detail retention with preserved aggregates, unauthorized access, and Chromium workflows.

## Operating limits

Use fixed UTC days for this milestone. Compare the selected window with the preceding equal-length window. Report browser and bot classification as estimates. The dashboard reports process-local queue and database losses. An abrupt restart can lose pending events without an exact count. It is possible for a completed public request to appear later in reports after the writer commits. The explicit clear action discards pending events associated with the previous generation.

Additional visit dimension comparisons, raw event deletion, local geolocation, analytical CSV export, and mass-load measurement build on this foundation.
