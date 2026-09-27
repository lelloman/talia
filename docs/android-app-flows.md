# Talìa Android core flows

Design specification, 2026-09-26. Editable screens live in `../android.pen`.
These are proposed native app behaviors, not a claim that the backend or Android
client already implements them. Screen data, check counts and timestamps are illustrative.

## Shared shell

Use LelloDesign Compose components and the copied navigation master. Drawer:
Overview, Reports, Automation, Chats, Settings; account at the foot. Top-level
screens show Menu; detail screens show Back. Back first dismisses the drawer,
keyboard or dialog. Appearance (Light/Dark/System) is in Settings only.
Preserve destination state and chat drafts when navigating. Long content scrolls;
app bar and system insets remain fixed. Honor font scaling and 48 dp touch targets.
Loading uses Talìa’s eye animation from board 10; saved feedback settles once.
Reduced motion shows the complete original icon. Status text remains stationary.

## Connect and sign in (12–15, 28)

12 Continue validates the server address and checks reachability before opening
13. Keep errors beside the address. Require HTTPS in production; never provide
an ignore-certificate-error action. 15 handles unreachable servers and retains
the address. Retry returns to the connection attempt; editing returns to 12.
13 starts sign-in in the system browser; cancellation returns to 13. Success
returns through a verified callback and opens 14, then Overview. Provider failure
shows a retryable message on 13. Do not collect the provider password in the app.
28 handles expiry and returns to the originating screen after same-account
sign-in. Keep drafts scoped to account and server. Switching either requires
confirmation before discarding unsent drafts; never expose another account’s cache.

Implementation dependency: native OIDC enrollment is not implemented today
(`user-access.md`). Define a public native client, authorization-code/PKCE flow,
verified callback, secure session storage and revocation before shipping.

## Report → investigation (16–21, 03, 07)

Reports Run now opens 16 with the saved report definition and time window.
Confirm submits once, disables duplicate taps and opens 17 once a run ID exists.
If submission times out, reconcile that request before offering another run.
Do not invent percentage progress; display completed/in-progress checks only
when the service exposes them. If it does not, show the eye and “Running report”.
Leaving the screen does not cancel the server run. Reopening resumes the same ID.

Completion opens 03: nominal/warning/error verdict, labeled check states,
start/end timestamps with timezone and run ID. An incomplete run has no nominal
verdict. Failure opens 20; Retry goes to 16 and creates a new run while preserving
the failed run. Connection loss goes to 27, not immediately to report failure.

17 Cancel opens confirmation 18. Keep running dismisses it. Confirm requests
cancellation and shows “Cancelling…” until acknowledged, then 19. Disable repeated
cancellation requests. If completion wins the race, show the completed result.
Cancellation is proposed behavior requiring backend support; don’t show the
control until it is supported. Partial check results remain distinguishable.

03/20 Investigate opens 21 with an editable message and visible report attachment.
Starting creates an independent chat with run ID, report window and check results,
then opens 07. Repeated taps must not create duplicate sessions. Draft cancellation
returns to the report. Sending failure preserves the draft and attachment.

## Schedules and alert rules (05, 09, 22–26, 32, 35)

Automation exposes separate Create schedule and Create alert rule actions.
05 collects report, repeat, local time, timezone and completion notification.
Show next run before save. Validate time, timezone and report access inline.
Save is disabled while pending; success opens 22. A server error opens 32 with
the full draft intact. Reconcile an ambiguous save before retrying. Edit reuses
the form with existing values; enabling/disabling is acknowledged by the server.
Delete requires confirmation and never erases past run history.

09 collects check, operator/threshold, duration and severity. Reject missing,
non-numeric or out-of-range thresholds and non-positive duration inline; focus
the first invalid field. Save opens 23. Notify once when firing and once on
recovery, not on every evaluation. Rule edits must not silently create duplicates.
22/23 return to Automation or offer notification setup 24.

24 is the contextual pre-permission screen after choosing notifications.
Enable invokes Android’s actual system permission when applicable; do not draw
or implement a custom imitation of that permission dialog. Allow → 25; deny → 26;
Not now returns without blocking setup. The app also needs a registered delivery
target; permission alone is insufficient. Show registration failures with Retry.
26 opens the app’s Android notification settings. Recheck on resume. Don’t repeatedly
prompt after denial. 35 edits severities, report completion, recoveries and quiet
hours. Save preserves values on failure. Quiet hours use the displayed timezone;
no severity bypasses quiet hours unless the user explicitly configures it.
Notification taps route to the specific run/alert after authentication. A removed
or inaccessible target shows a clear message and a route to its parent list.

## Chats (06–07, 30–31)

30 New chat opens 31. A nonempty message is required; title is optional. Starting
creates a server-owned session, opens it and adds it to 06. Each session has its
own context, draft, running state and unread result count. Switching sessions
never cancels work. Failed sends retain text; retry must not duplicate messages.
Back returns to the session list. Session deletion requires confirmation; stop
work is a separate action with pending/confirmed state if the service supports it.

## Recovery and empty states (27–34)

27 labels cached content with last-sync time. Offline reading is allowed only
for cached account-scoped data. Disable new runs and remote mutations; keep
editable local drafts and explain why Save is unavailable. Reconnect preserves
the destination, refreshes current state and reconciles outstanding requests.
Without cache show “Connect to load your reports”, not an empty-results claim.
28 handles expiry. 29/30/33 are successful empty loads, distinct from loading,
failures and insufficient access. 34 explains view-only permission and offers
only actions that are authorized. Forbidden or missing individual resources do
not reveal their private content. Retry never discards a draft.

## Implementation acceptance

- Native sign-in cancellation, failure, same-account return and account switch.
- Report submission reconciliation, reconnect, completion/cancel race and retry.
- Report attachment reaches only the newly created investigation session.
- Invalid forms preserve entries and show field-level feedback.
- Save retry does not create duplicate schedules, alerts or messages.
- Notification allow/deny, OS settings return, delivery registration and deep links.
- Parallel sessions survive navigation and app backgrounding.
- Empty/offline/forbidden/expired states are distinct; cached data is account-scoped.
- TalkBack labels, focus order, larger fonts, keyboard insets and reduced motion.

Design scope is complete for these core paths. Native auth, report progress/cancel,
chat persistence and Android notification delivery require API capability checks
before implementation. The Pen drawings are static; action destinations are stored
in layer context and described here, not wired prototype interactions.
