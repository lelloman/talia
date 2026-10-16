# Boxo services in periodic reports — 2026-10-16

Fixed all three live reports: `homelab-telegram-09` v13,
`homelab-telegram-14` v12, and `homelab-telegram-19` v12.
Only `reachability`, `availability` and `facts` steps changed. Existing schedules,
destinations, composers and other steps were preserved and read back exactly.
No Telegram message was sent as part of verification.

The live definitions still used the legacy Docker targets for both Pezzottify
and Pezzottflix, and counted intentional suspension in ordinary 24-hour uptime.
An earlier partial sleep-aware change existed only in source. It also retained
Pezzottflix's retired address and had an overescaped IP regex in the query.

Both services now use boxo's `192.168.1.34:9091` and `:9092` targets.
Reachability includes fresh node state only when the controller is healthy.
Expected sleep/transition states suppress service scrape/error-ratio findings;
awake, error or unknown state still checks them. An unavailable controller must
not conceal an application outage. Availability includes only samples from
settled awake/idle time (no node-state changes in the preceding five minutes),
with healthy controller observations. Old Docker instances and suspended time
are excluded; lack of settled awake samples alone is not an outage.

Validation: existing checklist tests and regressions for both services passed,
including expected sleep, awake failures, unknown/failed controller, old targets
and query escaping. Both PromQL queries executed on live Prometheus with no
warnings. A completed `send=false` run of the evening report showed all eleven
items OK, including both media services, and zero deliveries:
`report-255444b9d157ff627eeaf6e08ec017ed`.

Before/after definitions and optimistic-concurrency write results are preserved
on homelab under `~/homelab-data/talia-report-fixes/20261016-boxo`.
