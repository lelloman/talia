# Configurable alerts

Implementation is tracked by [TALIA-47](https://crumbles.lelloman.com/w/LLPR/TALIA/47).
This contract captures the accepted design; qualification records separately state
which paths have implementation evidence. Alerts precede deployment and homelab
migration. Simple Agents and Crumbles delegation follow migration.

## Identity, lifecycle and authority

A stable string key identifies the monitored condition (for example
`disk-space:host-a:/var`). Repeated observations update the current occurrence.
Recovery resolves it; a subsequent breach creates a new occurrence, with fresh
acknowledgement and delivery state. History retains previous occurrences.
Server state is authoritative. Acknowledgement records actor/time and is global,
but does not resolve a condition. Policies decide which actions stop on acknowledgement
and which transitions reset it. Dashboard presentation remains active until recovery.

Alert operations use explicit permissions: view, acknowledge, silence, configure,
and raise/update. They are independently grantable and audited. Client identity is
not permission. Revision/occurrence guards reject stale acknowledgement and mutation;
no client silently queues acknowledgement while disconnected. Dashboard failure must
not prevent the host from presenting or acknowledging alerts.

## Policies

Policies are reusable definitions referenced by bindings with independent parameters,
inputs and private state. A binding maps named engine input IDs to a policy, a stable
alert key and labels. Each evaluation receives current sample value, quality and
original timestamp, current occurrence, parameters, private state and trusted time.
Bounded server JavaScript returns the current condition, severity, message, stage,
updated private state and requested named actions. Missing/stale data must be handled
explicitly; evaluation failure freezes notification progression and is visible.
The script has no network or credential access. Host APIs commit state and schedule
only declared actions; I/O yields outside SQLite transactions.

Named stages define actions, delays, repeat intervals, stop-on-acknowledgement and
optional acknowledgement reset on entry. No actions means dashboard-only. A response
policy can show a warning immediately, request Telegram after ten minutes, repeat
until acknowledged, then request push on critical escalation. Resolution cancels
obsolete active actions and can schedule separately declared recovery actions.

Shared definition updates adopt the latest version for every binding without restart.
Instance parameter updates affect only that binding. Preserve occurrence history and
acknowledgement unless the policy explicitly resets it. Removing an occupied stage
requires an explicit old-to-new stage mapping; otherwise reject the entire update.
Configuration updates use optimistic version guards. Changed policy/action definitions
invalidate obsolete pending deliveries before the next dispatch.

## Silences, scheduling and recovery

Silences match a key or labels, have an expiry and a recorded actor/reason. They suppress
notifications, not evaluation or dashboard visibility. On expiry evaluate current state;
do not replay missed reminders. Delay and repeat schedules persist in SQLite.
Before dispatch or retry recheck occurrence, stage, acknowledgement, policy revision,
silence, expiry and evaluation health. Delivery is tracked per destination. Successful
destinations are not retried because a different destination failed. Configure retry
delay and maximum attempts. Failed delivery remains visible independently of alert state.

On restart mark in-flight delivery as uncertain, re-evaluate current conditions before
sending and coalesce missed intervals to at most one overdue action per destination.
Then schedule forward from current time, not from a catch-up loop. No network operation
holds the engine queue or a SQLite transaction. Acknowledgement cannot undo an external
effect already dispatched. An accepted message with a lost response can be delivered
twice after retry: no exactly-once claim is made.

## Destinations and Android

Named destinations select email, Telegram, or Android push. Policies reference names;
credentials and provider addresses are host-managed and never enter guest JS. Each
installation has a stable ID across restart/update and a separate renewable token.
Reinstall/data clearing creates a new identity. Registration requires authentication;
a client cannot claim another user's devices. Destinations can target one installation,
a named group or every installation owned by a user. Token rotation must not create
a second installation. Revoked/disabled destinations cannot dispatch pending work.

Push is an expiring snapshot. Opening it fetches current state; direct notification
acknowledgement and in-app acknowledgement both require server confirmation and stale
occurrence guards. Offline failure is visible and requires an explicit retry. The
installation identity may also own dashboard assignments; it is distinct from the
short-lived live-dashboard instance ID. A push token is not authentication.

Email uses configurable SMTP transport with TLS and optional authentication; Telegram
uses the Bot API. FCM is the implementation default for Android push; the shared delivery envelope
and lifecycle remain independent of the provider adapter.
Production credentials and designated recipients are deployment configuration, never
committed test fixtures. Qualification uses local provider fixtures and an Android
emulator, and must distinguish fixture evidence from real provider delivery.

## Public operation envelope

The authenticated alert API accepts `op` and operation-specific `args`. Mutations use
expected configuration/occurrence revisions and stable request IDs where effects can
be retried. Operations cover snapshot/history, observe, acknowledge, policies/bindings,
silences, destinations and device registration. MCP exposes discoverable dedicated
alert tools with schemas. Both platform hosts expose the same read/action operations
to their shared JS layer; no bearer token enters a dashboard definition or ViewModel.

Reference provider documentation: [Telegram Bot API](https://core.telegram.org/bots/api),
[SMTP transport](https://docs.rs/lettre/latest/lettre/transport/smtp/index.html),
[Android FCM setup](https://firebase.google.com/docs/cloud-messaging/android/get-started).

## Provider configuration

Set `TALIA_ALERT_PROVIDERS` to a private operator-owned JSON file mapping provider
names to configurations. It is read asynchronously for each dispatch; replace the
file atomically to rotate credentials without restart. The authored destination
contains only the provider name. Do not put this file in source control.

```json
{
  "mail": {"kind":"smtp","host":"smtp.example.test","port":587,
           "tls":"starttls","from":"talia@example.test",
           "username":"operator-supplied","password":"operator-supplied"},
  "chat": {"kind":"telegram","token":"operator-supplied"}
}
```

SMTP supports required STARTTLS or implicit TLS (`tls`); plaintext `none_loopback`
is restricted to loopback fixtures/relays. Telegram defaults to its HTTPS Bot API;
HTTP overrides are restricted to loopback test fixtures. Redirects are disabled.
Requests have time/response bounds, and provider error details never expose remote
bodies, credentials or credential-bearing URLs. A provider's positive acceptance is
recorded as `sent`; this does not prove that the human read the message.


## Android push setup

The server's FCM provider uses HTTP v1 and service-account OAuth (RS256). Add a
provider such as `"android":{"kind":"fcm","project":"your-project-id",
"service_account":"/run/secrets/talia-firebase.json"}` to the operator file. The
service-account JSON is read asynchronously and never returned by the alert API.
Configure Firebase project credentials in the Android build using Gradle properties
`firebaseAppId`, `firebaseApiKey`, `firebaseProjectId`, and `firebaseSenderId`.
Without these the APK still provides dashboards and alert controls, and explicitly
reports that push is not configured. No Firebase project is created by this work.

The app creates a stable installation UUID, keeps it across updates/restarts, and
registers its renewable FCM token under the authenticated alert principal. It retries
registration through an Android network-constrained JobScheduler job. Device groups
are operator-assigned; registration cannot grant group membership. The alert access
token is host configuration, separate from the push token and ViewModel state.
Grant `alerts` family `register`, `read` and `acknowledge` permissions as needed;
`configure`, `silence` and `audit` remain independently grantable.

Enable notifications through the native Alerts screen to request Android's runtime
notification permission. Tapping a push opens current server state. Direct
acknowledgement uses the pushed occurrence and revision; stale or disconnected
requests show an acknowledgement failure and never claim success. Expired payloads
are ignored and displayed notifications expire automatically. Clearing application
data or reinstalling produces a new installation UUID.

The current development host still uses the existing loopback connection and ADB
reverse port. Reachable authenticated production endpoints and operator provisioning
belong to deployment preparation; do not treat fixture success as a production rollout.
Debug APKs include a clearly isolated fixture Activity for injecting test payloads and
exercising PendingIntent actions. It is absent from release sources/manifests.

References: [FCM server authorization](https://firebase.google.com/docs/cloud-messaging/auth-server),
[Android receipt](https://firebase.google.com/docs/cloud-messaging/android/receive-messages).
