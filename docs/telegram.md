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

Each request has a fifteen-minute total deadline from acceptance, including queue
time, compaction and investigation. Expired queued requests never start inference.
On timeout Talìa stops waiting, stops typing, retains failure evidence and queues
a clear timeout reply; late results are discarded. Sending a new message starts
a new request. Provider/network failures may still end a request earlier.

Conversation state is per chat/account. Each ordinary question first goes through a
tool-free session classifier using the configured simple-ai model. A new topic
advances a durable request-ID cutoff; older chat, summaries and diagnostic
evidence are excluded. Follow-ups continue the active session. Ambiguous boundaries
produce a short clarification before diagnostics; the unresolved question is
retained for the next reply. Classification runs when the request reaches the head
of its queue, so earlier accepted work can finish. `/new` remains an immediate
reset that cancels older local work and pending replies.

Classifier requests have a hard **10,000-character** limit on the complete
serialized HTTP body, including model metadata, system instructions, the latest
message, pending clarification, history and JSON escaping. Characters are Unicode
scalar values, not UTF-8 bytes. This is checked immediately before sending.
Oversized requests are not sent. If the required history is incomplete or the
request cannot fit, Talìa preserves the current cutoff and proceeds through normal
compaction and answering with the existing bounded history. It does not silently
truncate classifier evidence to justify a cutoff. This cap applies to the current
chat-based classifier; the experimental JEV cutoff search is not integrated yet.

Original history is immutable. A separate working context references exact chat
messages, stores selective summaries with source coverage, and retains bounded
diagnostic evidence with AI run/tool IDs, the query, run creation time and evidence
capture time. Capture time is not a fresh measurement: timestamps inside the
original result remain authoritative. Assistant reasoning is not copied.
Each tool result excerpt is limited to 4 KiB and its query to 2 KiB, with explicit
truncation markers. Copies remain usable after the original AI run is pruned.

Selective compaction runs after classification when active history exceeds
16 KiB or 16 unprocessed chat messages. It processes bounded batches, keeping
useful exact details and condensing repetition; previously kept messages can be
reconsidered. Source coverage is checked for unknown IDs, duplicates and dropped
entries. Summary coverage can reference an earlier summary, preserving its
provenance without loading all original messages again. `/compact` requests one
selective batch without changing the cutoff. Original messages are never deleted.

Automatic maintenance allows at most two 32-KiB batches in a shared 180-second
budget, with 8,192 output tokens per completion. Classification allows 120 seconds
and 2,048 output tokens. Both use the existing model and stay within the request's
fifteen-minute deadline. Compaction aims for 12 KiB per replacement batch; answer
history is capped at 24 KiB, prioritizing the latest turn and summaries. At most
512 active entries are considered per pass. Any omitted context is explicitly
marked; the incoming question and explicitly selected report are included
separately within the AI input-size limit.

Classification failure leaves the cutoff unchanged and answers with only the
current question and selected report, asking for clarification if that is
insufficient. Compaction failure preserves the last valid working context and
continues with bounded history. Incomplete model output is never accepted as a
summary. Failed manual compaction reports that the context was unchanged.
No automatic retry repeats interrupted inference; completed phase results are
reused after restart.

Schema 18 adds request ownership and working-context records. Existing chats begin
fresh context on their next newly admitted request; old history and summaries
remain available for inspection. Jobs admitted before the upgrade finish using
the legacy conversation path. Back up SQLite before upgrading. Older binaries
reject schema 18; binary-only rollback is not supported.

Reports are not automatically appended to chat or compaction inputs. Replying to
a stored report selects that report explicitly; a question retains its report ID,
and a diagnostic report read may supply a bounded historical evidence excerpt.

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

Outgoing chat replies, reports and alerts render Markdown as Telegram text and
formatting entities: bold, italics, strikethrough, links, inline code and fenced
code blocks. Headings become bold and lists remain readable text. Raw HTML stays
literal; unsupported formatting falls back to readable text. Long outbox messages
are split at 3,600 UTF-16 units, preserving formatting across parts and emoji
boundaries. Every part has its own durable text, entities, delivery state and
report reference. Existing queued messages retain their original plain text.
A restart or uncertain response during send becomes `unknown`, with no automatic retry or duplicate send. Report delivery is
complete only when all parts are confirmed. Telegram acceptance is not a read
receipt. The settings page displays polling errors, delivery counts and recent
investigation failures and local AI run IDs. Job status also exposes the execution
phase, session cutoff, context revision, maintenance error and fallback flag.

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

## Timeout update — 2026-09-26

Telegram investigations now allow 15 minutes from acceptance. The user-facing
expiry notice and queued/active deadline tests use the same limit. Existing jobs
retain their stored deadlines; new requests receive the increased budget.

Deployed image: `registry.homelab:5000/talia:telegram-timeout-15-20260926`, digest
`sha256:a075427b1df1dbd1bfd468acc2e145c0c56665f2f0c73c57ae8dad53276749e9`.
The working-tree build includes the compact report layout. Deployment uses
`/tmp/talia-telegram-timeout-15-20260926.yml` on homelab; future base-Compose
`latest` deployments must include these changes. Both pre-deployment SQLite
backups passed integrity checks; all eight Telegram tests passed.

Schema version 21 adds persisted formatting entities to the Telegram outbox.
Back up the engine database before upgrading; older binaries reject this newer
schema, so rolling back requires a compatible binary or the pre-upgrade backup.
