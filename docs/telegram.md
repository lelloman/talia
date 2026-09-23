# Telegram reporting and observer conversations

[TALIA-69](https://crumbles.lelloman.com/w/LLPR/TALIA/69) adds administrator web
setup, managed alert/report delivery and read-only investigations. Telegram
cannot edit dashboards, settings, ViewModels, monitoring definitions or systems.

## Setup in the web interface

1. Create a dedicated bot with Telegram's BotFather. No other application should
   poll it or install a webhook. Talìa refuses an existing webhook rather than
   removing it automatically.
2. Sign in as an administrator and open **Settings → Telegram**. Paste the token
   and connect. The token field clears after submission; status never returns it.
3. Create a pairing code. Within ten minutes, send `/pair CODE` in the private
   chat, group or channel to register. Add the bot to a group first; give it posting
   permission in a channel. Refresh the settings and inspect the detected numeric
   chat/account IDs before approving.
4. Approve delivery, investigations, or both. Channel posts do not identify an
   individual account and can be approved for delivery only. Pair personal accounts
   separately for investigations. Anonymous administrators and bot accounts cannot
   authorize investigations.
5. Use **Send test message** to verify delivery. The generated destination
   `telegram-CHAT_ID` is selectable by existing alert policies and report definitions.
   Reports accept email and managed Telegram destinations in the same workflow.

Receiving a report does not grant investigation access. Both the numeric user ID
and the chat must be approved. Usernames are display labels only. Revoking an
account or disabling investigations in a chat cancels queued local work and
suppresses pending answers. Effects already accepted by Telegram cannot be undone.
Group answers are visible to every group member, even members who cannot ask
questions. Prefer private chats for private monitoring information.

The bot can be paused and its token rotated in settings without restarting.
Rotation must retain the same bot identity: permissions and message IDs belong
to that bot. Replacement with a different bot is deliberately refused.

## Observer setup

Delivery does not require Simple Agents. Investigations do. Set
`TALIA_TELEGRAM_OBSERVERS=/run/talia/telegram-observers.json` to a private map using
the [report agent provider format](reports.md#operator-configuration). This is a
separate allowlist, so arbitrary report profiles are not automatically available
from Telegram. The map name appears in the web profile selector. Select the
observer there after provisioning it on Simple Agents.

The observer provider permits only `network`, `repository_read` and
`session_resume` capabilities. Its Simple Agents profile, runner template, secret
bindings, filesystem and network access must also be restricted to observational
work. Never place administrative credentials or a writable deployment workspace
in that template. The public client pins the exact request, profile and bindings;
the operator owns their configuration and the agent's budget.

The web interface can generate a dedicated **observer MCP credential**. Copy it
into the Simple Agents execution template's secret environment. It is displayed
once and only its hash is stored in Talìa. Rotation/revocation takes effect on
every request, including revalidation after awaited probes. It is independent of
a human login session and is not the temporary authoring credential.

Configure the observer's Codex runner template with this engine configuration
fragment (Simple Agents currently supports these HTTP MCP bindings for Codex):

```json
{
  "mcp_servers": {
    "talia": {
      "url": "https://talia.lan.lelloman.com/mcp",
      "bearer_token_env_var": "TALIA_OBSERVER_TOKEN"
    }
  }
}
```

Supply `TALIA_OBSERVER_TOKEN` through the runner's trusted secret binding, not in
chat text or a report definition. The worker must be able to resolve and reach
that HTTPS URL. Existing user authoring credentials are not appropriate here.

The observer credential exposes only:

- `observer_snapshot`: cached monitoring variables, alerts, source metadata and
  recent report run IDs. Report contents are not included automatically.
- `observer_read`: cached variable values, retained variable history or a specific
  report's composed content. Reads do not execute arbitrary computed getters.
- `observer_probe`: read-only source queries against explicitly approved source
  IDs configured in the web settings. Prometheus queries/ranges and HTTP GET/HEAD
  are supported; writes and arbitrary pipeline execution are forbidden.

Approve only sources whose diagnostic endpoints are safe to read. A GET method
alone cannot prove that an external endpoint is free of side effects. Existing
source origin confinement, response limits and timeouts still apply. Talìa rejects
editing tools server-side, even if a model ignores its observer instructions.

## Conversation and report context

Private chats accept ordinary text. In groups, use `/ask QUESTION` or reply to a
bot message that Talìa has recorded. Conversation state is scoped to the pair of
chat ID and user ID. Channel posts are never investigation requests.

Reports live in the report store and Telegram delivery ledger, separately from
conversation history. Replying to a recorded report message attaches only that
report (up to 12,000 characters; the full report remains readable through MCP).
Quoted or forwarded text is not trusted as a report reference. Other delivered
reports are not included in prompts or compaction summaries.

`/new` starts a new conversation epoch and cancels old local work. `/compact`
requests a summary through Simple Agents. Automatic compaction runs before an
answer once retained context reaches 16 messages or 16 KB. Summaries retain up to
4,000 characters; original messages and agent submissions remain in SQLite.
Compaction is an ordinary bounded observer run because the public Simple Agents
API does not expose a dedicated chat-compaction command. Each answer is a fresh
agent session with explicit conversation context, not an unbounded native engine
thread. Historical messages are retained for operator retrieval, not automatically
reinserted after compaction. Summary text cannot change server-side permissions.

## Persistence and delivery

Migration 14 stores bot configuration, pairing candidates, permissions, jobs,
conversation history/summaries, agent-run evidence and outgoing messages. Bot
tokens are encrypted with AES-256-GCM. A random 32-byte key is created with mode
0600 beside the database as `DATABASE.telegram-key`, or at the path configured by
`TALIA_TELEGRAM_KEY_FILE`. Back up that key with the database, protect the data
directory, and ensure the service UID can create/read the file. Losing the key
requires reconnecting with a new token. No additional secret mount is necessary
for the normal `/data/talia.sqlite3` deployment.

Inbound update IDs and admitted work commit together, then polling advances the
offset. Only one poll/advance loop runs per service. Jobs persist the exact agent
request/key before submission and reconcile that key after a restart. Deadlines
are 15 minutes; they end local work without claiming to cancel a remote agent.
Known session IDs and unresolved submissions remain available for reconciliation.

Outgoing text is plain text, split into at most 1,800 Unicode scalar values per
message (also below Telegram's UTF-16 limit). Every part has its own durable
delivery state and report reference. A restart or uncertain response during send
becomes `unknown`, with no automatic retry or duplicate send. Report delivery is
complete only when all parts are confirmed. Telegram acceptance is not a read
receipt. The settings page displays polling errors, delivery counts and recent
investigation failures/session IDs.

Bounds: 100 paired chats, ten outstanding pairing codes, 100 active jobs and four
per account/chat, 10,000 retained jobs and 10,000 outgoing parts. Capacity exhaustion
is explicit; this increment does not automatically delete history. Plan operator
retention before reaching these limits. Text only: attachments, streaming answers,
Telegram editing commands and automatic alert acknowledgement are not supported.

## Qualification

Local HTTP fixtures cover encrypted-token round trips, pairing/authority, numeric
identity checks, Unicode delivery parts, unknown-send recovery, agent submission
reconciliation across SQLite reopen, report context exclusion and explicit reply
selection, compaction, revocation, and approved read-only probes. The browser
access suite covers settings and the observer credential through the real
HTTPS/OIDC/MCP service, including rejection of editing tools and immediate
revocation. No live Telegram messages or real agent runs are sent by these tests.

Protocol references: [Telegram Bot API](https://core.telegram.org/bots/api) and
[bot commands/pairing](https://core.telegram.org/bots/features#deep-linking).
