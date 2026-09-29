# Store notifications

Talìa can send report results and incident transitions through the self-hosted
LelloStore broker. This integration is disabled unless
`TALIA_STORE_NOTIFICATIONS_FILE` names a private writable JSON configuration:

```json
{"url":"https://store.lelloman.com","invitation":"ONE_TIME_ADMIN_INVITATION","applications":["com.lelloman.talia"]}
```

Create the invitation in Store's Notifications administrator page, approving
Talìa's exact package and release signing certificate. The worker saves its
random sender credential before redemption. Protect the configuration directory
and file; never put this credential into Android or browser code. Sender
credential rotation requires updating the file and restarting Talìa within the
24-hour overlap.

Migration 019 writes notifications transactionally with report/incident state.
Report previews do not publish. Reports queue until receipt; incident state uses
occurrence/revision replacement. The backend renews subscription authorization
only while the native session and administrator access remain valid. Native
apps enroll through authenticated `/native/notifications` and verified local
Binder calls. Both the self-hosted Store server and Android Store may read the
payloads. Transport uses TLS.

Build Store's `notification-client` and `notification-protocol` release artifacts
into its local build repository first. Supply `LELLOSTORE_SIGNING_CERTIFICATES`
when building Talìa, using comma-separated lowercase SHA-256 Store signing pins.
An unconfigured build cannot enroll. Enable notifications in Talìa's Settings
and grant its notification permission; Store must have Shared notifications and
unrestricted battery use enabled.

APK 4 introduces the receiver service. Build a new shell using
`-PparavoidNewShell=true`, export/review its baseline, and retain that accepted
baseline in `android/paravoid-baselines/apk-4` before ordinary payload builds.
Do not publish this as a payload-only update to APK 3. Building the new shell
option does not authorize publishing or accepting a baseline automatically.

See the [Store protocol and qualification guide](../../lellostore/docs/SHARED_ANDROID_NOTIFICATIONS.md)
for wire contracts, limits, failure behavior and device/battery qualification.
