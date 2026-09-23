# Telegram report and alert delivery

Administrators configure a dedicated bot and approve destinations in the web UI.
Telegram cannot edit Talìa. Read-only investigations and chat compaction use the
[Talìa AI harness](ai.md) with a single simple-ai connection.

## Setup in the web interface

1. Create a dedicated bot with BotFather. No other application should poll it or
   install a webhook. Talìa rejects bots with an existing webhook.
2. Open **Settings → Telegram** as an administrator. Paste the token and connect.
   The field clears after submission; status never returns the token.
3. Create a pairing code. Within ten minutes, send `/pair CODE` in the private
   chat, group or channel. Add the bot to a group first; give it posting permission
   in a channel. Refresh and inspect the detected numeric IDs before approving.
4. Approve delivery, investigations, or both. Investigation access requires an
   identified personal account and an approved chat; channels are delivery-only.
   Use **Send test message**. The destination
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
accounts remain visible and revocable. Investigations default to disabled after
migration; enable them explicitly after configuring simple-ai. Reports remain
separate from conversation history.

## Investigation setup and conversation

Configure the [single simple-ai connection](ai.md#one-connection-and-model), then
select **Enable read-only investigations** in Settings → Telegram. Optionally
enter approved diagnostic DataSource IDs. This grants approved chat accounts
broad monitoring reads, not just access to one dashboard. Group answers are visible
to all group members, even those who cannot ask questions.

Private chats accept ordinary text. In groups use `/ask QUESTION` or reply to a
recorded bot message. Replying to a report part selects only that report as context
(up to 8 KB with an explicit truncation marker); other reports are not included.
Forwarded/quoted text is not trusted as a stored report reference. Full report
content can be read through an explicit internal tool call.

Accepted questions and `/compact` requests receive a short acknowledgement on
the next bot poll (normally within two seconds), prioritized ahead of queued
reports. Queued requests are identified as such. During active processing Talìa
refreshes Telegram’s typing indicator every four seconds; it stops on completion,
failure, cancellation or revoked access. Typing is best-effort: Telegram failures
do not interrupt an investigation. Acknowledgements use the durable outgoing queue
and its existing uncertain-delivery policy, and are never added to AI history.

Each request has a five-minute total deadline from acceptance, including queue
time, compaction and investigation. Expired queued requests never start inference.
On timeout Talìa stops waiting, stops typing, retains failure evidence and queues
a clear timeout reply; late results are discarded. Sending a new message starts
a new request. Provider/network failures may still end a request earlier.

Conversation state is per chat/account. `/new` starts a fresh epoch and cancels
older local work/replies. `/compact` requests a summary; automatic compaction runs
before an answer at 16 messages or 16 KB of recent context. Summaries keep at most
4,000 characters. Large histories compact in bounded batches with durable progress;
exceptionally large old individual messages are marked as truncated. Original
history stays in SQLite. Reports are not appended to history or compaction inputs;
a user's question retains only its report ID reference, while the assistant's
answer is ordinary conversation history.

Failed and timed-out requests also retain their question (including any report ID)
and a bounded outcome notice in conversation history, so follow-ups such as “try
again” and later compaction retain the intent. Raw provider errors and incomplete
tool transcripts are not added to chat context. This applies to failures recorded
from this release onward; older failed jobs remain in execution records. `/new`
still isolates the new conversation, and revoked/cancelled work adds no late result.
Observer instructions request brief, relevant replies and explain that retention
limits are not sample counts, internal state is independent of the exposed value,
and an empty alert list is not proof of overall health.

Compaction has no tools. Investigations only have the cached reads and approved
probes described in [the harness contract](ai.md#read-only-telegram-tools).
Revoking a user/chat, changing settings or starting a new conversation prevents
later results from being used. Settings revisions cancel older jobs, so saving
settings during a run may cancel that investigation.

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
investigation failures and local AI run IDs.

Bounds: 100 paired chats, ten outstanding pairing codes, 100 active jobs, four per
chat/account, 10,000 retained jobs and 10,000 outgoing parts. Capacity exhaustion
is explicit; this increment does not automatically delete history. Plan operator
retention before reaching these limits. Text only: attachments, streaming answers,
Telegram editing commands and automatic alert acknowledgement are not supported.

## Qualification

Local fixtures cover encrypted tokens, pairing and numeric permissions, rejected
retired configuration/credentials, read-only investigations, Unicode delivery,
unknown-send recovery and report delivery through the managed bot. Migration tests
verify preserved bot setup/history and cancelled pending investigations. Browser
checks cover delivery settings, viewer denial and rejection of retired MCP keys.
Conversation fixtures also cover report context selection, manual/automatic
compaction, revocation during inference and bot delivery while inference waits.
No live Telegram messages are sent by these tests.
