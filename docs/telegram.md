# Telegram report and alert delivery

Administrators configure a dedicated bot and approve destinations in the web UI.
Telegram cannot edit Talìa. Investigations and chat compaction are currently
unavailable while the external agent integration is being replaced.

## Setup in the web interface

1. Create a dedicated bot with BotFather. No other application should poll it or
   install a webhook. Talìa rejects bots with an existing webhook.
2. Open **Settings → Telegram** as an administrator. Paste the token and connect.
   The field clears after submission; status never returns the token.
3. Create a pairing code. Within ten minutes, send `/pair CODE` in the private
   chat, group or channel. Add the bot to a group first; give it posting permission
   in a channel. Refresh and inspect the detected numeric IDs before approving.
4. Approve delivery, then use **Send test message**. The destination
   `telegram-CHAT_ID` is usable in alert policies and report definitions.

The bot can be paused and its token rotated without restarting. Rotation must
retain the same bot identity because permissions and message IDs belong to it.
Reports can use email and managed Telegram destinations together.

## Existing installations

Migration 15 removes the observer profile, probe-source configuration and dedicated
observer MCP credential. No runner profiles, provider maps or service credentials
are needed. User-issued temporary authoring MCP keys continue to work as before.

Pending investigations are cancelled and pending chat replies suppressed. Existing
bot credentials, paired destinations, approved accounts, conversation history,
compaction summaries and execution evidence are retained. Previously approved
accounts remain visible and revocable, but no new investigation access can be
granted. Authorized incoming questions receive an unavailable notice; no LLM job
is created. Reports remain separate from conversation history.

The next implementation will use simple-ai completions through Talìa's own harness.
It must preserve read-only investigation permissions, selected report context and
conversation compaction. This release does not yet provide that harness.

## Persistence and delivery

Migration 14 stores bot configuration, pairing candidates, permissions, jobs,
conversation history/summaries, historical execution evidence and outgoing messages. Bot
tokens are encrypted with AES-256-GCM. A random 32-byte key is created with mode
0600 beside the database as `DATABASE.telegram-key`, or at the path configured by
`TALIA_TELEGRAM_KEY_FILE`. Back up that key with the database, protect the data
directory, and ensure the service UID can create/read the file. Losing the key
requires reconnecting with a new token. No additional secret mount is necessary
for the normal `/data/talia.sqlite3` deployment.

Inbound update IDs and pairing admissions commit together before polling advances
the offset. One poll/dispatch loop runs per service. No external agent is contacted.

Outgoing text is plain text, split into at most 1,800 Unicode scalar values per
message (also below Telegram's UTF-16 limit). Every part has its own durable
delivery state and report reference. A restart or uncertain response during send
becomes `unknown`, with no automatic retry or duplicate send. Report delivery is
complete only when all parts are confirmed. Telegram acceptance is not a read
receipt. The settings page displays polling errors, delivery counts and recent
investigation failures/session IDs.

Bounds: 100 paired chats, ten outstanding pairing codes, 10,000 outgoing parts. Capacity exhaustion
is explicit; this increment does not automatically delete history. Plan operator
retention before reaching these limits. Text only: attachments, investigations,
Telegram editing commands and automatic alert acknowledgement are not supported.

## Qualification

Local fixtures cover encrypted tokens, pairing and numeric permissions, rejected
retired configuration/credentials, unavailable investigations, Unicode delivery,
unknown-send recovery and report delivery through the managed bot. Migration tests
verify preserved bot setup/history and cancelled pending investigations. Browser
checks cover delivery settings, viewer denial and rejection of retired MCP keys.
No live Telegram messages are sent by these tests.
