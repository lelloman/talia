# Monitoring contract (P3)

Rust owns DataSources, Pipelines, Variables, Watches and scheduling, independent of
connected clients. Bounded JavaScript defines Pipeline and Watch behavior. Runtime
configuration changes do not require restart. This contract records the approved
P3 decisions; Crumbles TALIA-4 and its children own implementation status.

## Definitions, bindings and state

A versioned configuration contains source connections, shared definitions and
instances. Instances bind local input/output/source/action aliases to server IDs
and carry parameters. Updating a definition updates every referencing instance;
editing an instance affects only that instance. State and parameters use the P2
lossless value envelope. Missing, undefined, null, NaN and infinities stay distinct.

Activation validates the whole graph, JS entry points, named time zones, bindings,
schemas and optional migrations before one SQLite commit. Invalid updates leave
the old configuration active. Each changed instance advances a generation which
fences unfinished old work. A migration is synchronous, capability-free and bounded;
one failure prevents the entire activation. Parameter-change policy is preserve,
reset or migrate. The disk Watch resets for source/threshold changes and evaluates
fresh inputs. Migration receives `(state, newParameters, oldParameters)`.

DataSources use HTTP(S) base URLs without embedded credentials, queries or fragments.
Credential references resolve on the server only. HTTP/probes have deadlines and
response-size limits. Prometheus remains the scraper and time-series store: Talìa
queries current or historical series, persists latest results, and retains additional
local history only when explicitly configured. Source definitions do not own timing.
Source failures are visible in result quality and run status.

Pipelines publish to explicitly bound stored Variables; each output has one configured
Pipeline producer. Watches read declared inputs and request bound Pipeline actions.
Dependency cycles are rejected, including feedback through action outputs. Trigger
chains and active work have finite limits. Dashboards never receive source secrets.

## Execution and schedules

An active Pipeline instance skips another scheduled occurrence. Manual requests
return the active run. Watch requests coalesce into one pending follow-up; different
instances progress concurrently while awaiting I/O. A follow-up uses its instance's
parameters and reads current inputs when it starts; trigger identities remain
associated with the admitted run. This is a request to reevaluate, not arbitrary
merging of caller-supplied argument objects.

Every run has an explicit overall deadline. Retries are off by default. Optional
bounded retries include their delay within that deadline and require explicit
repeat-safety for side effects. Timeout or explicit run cancellation stops remaining
work, cancels I/O where possible and fences late results/actions. Already committed
changes/effects remain. An uncertain external outcome is unknown, never permission
to replay. Cancelling a caller's wait does not cancel the server-owned run.

Intervals are independent of time zone. Calendar schedules have a named zone; the
initial calendar form is a daily HH:MM with optional ISO weekdays (1=Monday).
Repeated DST local times run once; nonexistent times run at the first valid instant
after the clock gap. Scheduled time advances independently of run duration. Missed
occurrences during downtime are an observable gap and one fresh recovery check,
not a burst replaying all missed work.

## Watches, observations and recovery

A Watch has sequential evaluations per instance, without blocking other instances
during I/O. Value/quality changes and configurable timers trigger evaluation.
Fresh observations refresh age even if their measured value is unchanged. Keep the
last value and timestamp on collection failure and expose error/stale status. A
threshold Watch freezes its flags until fresh valid data arrives; an availability
Watch may alert after a configurable grace period using a timer.

The example disk Watch stages at below 10% and below 6% free. A 12% to 5% jump sets
both flags in order and investigates once. A first valid 5% reading does the same.
Fluctuation near 6% does not repeat the investigation. Recovery to a configurable
threshold (example 12%) resets both flags. These rules belong to reusable JS logic,
not a fixed server threshold language. Persisted flags survive restarts.

Watch state and investigation requests commit atomically. Recovery may dispatch a
durable request known not to have started. Started work with an uncertain outcome
is reported unknown and is not automatically replayed. UI delivery may coalesce
state snapshots; server Watch processing must not silently discard an intervening
threshold transition. Overload must be explicit rather than appearing healthy.

## Persistence and scope

SQLite schema 2 adds monitoring configuration and per-instance runtime state to P2.
Existing values, timestamps, histories and action records survive migration. No
SQLite transaction spans I/O. Later execution tables extend this schema monotonically.
Web and native Android consume named engine resources with identical value semantics.
Full MCP authoring, remote agent/ticket integrations, alert delivery, authentication
and production deployment are later roadmap stages.

## Source adapter API

A Pipeline requests a bound source with `{kind:"query",query,time?}` or
`{kind:"range",query,start,end,step}` (seconds), or an HTTP request with
`{kind:"http",path,method?,body?,text?}`. HTTP defaults to GET and JSON decoding;
`text:true` returns UTF-8 text. Paths must remain on the configured origin;
redirects are not followed. POST/PUT/PATCH/DELETE are effectful operations.
Bearer credentials resolve from `TALIA_SECRET_<credential_ref>` at request time;
errors omit bodies/URLs and echoed credential strings are redacted from results.

Prometheus results retain `resultType`, `warnings`, `infos` and `result`. Vector and
matrix entries contain `metric` labels and `samples` pairs `[timestamp, number]`.
Scalar/string results are one pair. Numeric strings become lossless JS numbers;
label strings remain strings. Native histogram samples currently return an explicit
unsupported-result error. The adapter follows the
[Prometheus HTTP API](https://prometheus.io/docs/prometheus/latest/querying/api/).
Range requests are bounded to 11,001 time steps; decoded values must also fit the
engine's 128 KiB value budget. There is no implicit cross-query cache; Pipeline
instances control polling and persisted latest results retain their original age.
