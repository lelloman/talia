# Talìa AI harness

Talìa calls simple-ai's `/v1/chat/completions` endpoint directly. Talìa owns the
bounded model/tool loop, permissions, context and durable outcomes. There are no
runner profiles, external agent sessions or observer MCP credentials.

## One connection and model

Administrators configure the installation in **Settings → simple-ai**:

1. Enter the simple-ai server origin and model or model class.
2. Choose **Connect LelloAuth account**. Open the authorization link and sign
   in as the dedicated Talìa account. The separate account-linking sign-in preserves
   your existing LelloAuth and Talìa browser sessions, including when using MFA.
3. Return to Talìa, check the displayed identity, then choose **Use this account**.

Talìa discovers the issuer and public client from `/.well-known/simple-ai` and uses
LelloAuth's device authorization flow with its advertised
`device_account_link_endpoint` extension. Older providers must be upgraded; Talìa
does not silently fall back to authorizing the current SSO identity. Enable **device flow** on that advertised
public client in LelloAuth (simple-ai advertises its Android/public client). Its
client ID must be accepted by simple-ai; the browser's Talìa OIDC client is a
separate application and is not reused. Give the dedicated account the appropriate
simple-ai per-app model/class roles. Specific model selection requires
`model:specific`; class selection uses its class permissions. Choose a model that
supports function tool calls for investigations. Connecting verifies the provider
identity, not model availability or inference permissions; inference failures are
visible on the run.

LelloAuth does not support password grants. Talìa saves the authorized account's
renewable session, not its password. This is separate from the operator's browser
login. Access/refresh tokens and pending device credentials are AES-256-GCM
encrypted in SQLite. Back up the database **and** its `DATABASE.ai-key` sidecar
(mode 0600); `TALIA_AI_KEY_FILE` can override the key path. No new mount is needed
when the database directory is already persistent and writable by the service.
Tokens never reach the web UI or MCP. Schema 17 adds this installation account.

Account changes apply without restart. Reconnect to change account, origin or
model. Starting a replacement connection stops use of the previous connection.
A pending connection expires and requires the initiating administrator's explicit
identity confirmation. Refresh is serialized across runs, rotates credentials
durably, and checks that the provider identity stays the same. Tokens are saved
before safe userinfo validation can be retried. An interrupted/ambiguous token
exchange requires reconnecting; Talìa never blindly reuses an old refresh token.
Restart recovery retains confirmed connections and fails interrupted exchanges.

Disconnect immediately forgets local credentials and invalidates in-flight AI
continuations, then attempts provider session revocation when advertised. A
provider revocation failure is shown separately; local disconnection still holds.
An already-sent model request or completed tool read cannot be undone. Changing
connections also discards late results. Old provider sessions from replacement or
ambiguous exchanges can be revoked through LelloAuth.

### Optional file-based API key

Existing installations can continue using this alternative **until a managed
account is configured or explicitly disconnected**. A disconnected or broken
managed account never silently falls back to a file credential.

Set `TALIA_AI_CONFIG=/run/talia/ai.json` and mount this private file:

```json
{
  "origin": "https://simple-ai.example.com",
  "model": "YOUR_MODEL_OR_CLASS",
  "token_file": "/run/talia/simple-ai.key"
}
```

Create a dedicated simple-ai API key (`sk-…`) with inference access to that model
or class, and put only the key in `simple-ai.key`. The local simple-ai gateway
accepts these keys as Bearer credentials. Specific model selection requires its
`model:specific` permission; alternatively use a model class permitted by the key.
Select a model supporting function tool calls for Telegram investigations.

Both files must be readable by the Talìa service UID (65532 in the container),
with access restricted to the service/operator. The origin must be HTTPS, without
a path, query or embedded credentials. Numeric loopback HTTP is allowed for tests.
There is no automatic anonymous/LAN authentication fallback. Redirects are refused.
Adding the environment variable or mount requires container recreation; replacing
file contents atomically affects subsequent runs without a service restart.
A file-based run pins the connection/model/token it loaded until it finishes;
creating a managed connection invalidates that run.

Telegram settings show the configured model or connection error. For file setup,
“configured” means the file is valid, not that inference credentials or model availability have
been checked remotely. Inference errors are retained with the run. The token,
headers, private file paths and upstream HTTP error bodies are never returned in
run results. Report/Telegram data sent to simple-ai may be retained by its own
request logging; configure that deployment accordingly.

## Reports

An `analysis` step has `instructions` and `inputs` naming previous steps. It sends
only those selected outcomes and the reporting period to the model, with no tools.
Its output is `{run_id,summary,model,turns}`; compose from `value.summary`. Collection,
probes and JavaScript transformations remain explicit report steps.

See [morning homelab](../reports/examples/morning-homelab.json). Scheduling starts
disabled so it can be previewed. `send:false` still performs inference.

## Read-only Telegram tools

An administrator enables investigations and approves both a numeric account and
its chat in Settings → Telegram. This grants broad monitoring read access;
it is independent of dashboard viewer grants. Telegram never exposes authoring,
engine writes, getters, setters, arbitrary scripts, pipelines or alert controls.
The tools are internal Rust dispatch, not another MCP connection:

- `monitoring_snapshot`: cached variables, alerts, source IDs/kinds and recent
  report metadata. Full reports are not added automatically.
- `monitoring_read`: a cached variable, retained history, or a specified report's
  composed content. It does not invoke computed getters.
- `monitoring_probe`: queries against explicitly approved diagnostic DataSource IDs.
  Prometheus query/range and HTTP GET/HEAD only, using the existing confined source
  adapter, credentials, size and timeout limits. Approve endpoints known to be
  safe to read; HTTP method alone cannot establish absence of external effects.

Unknown/mutating tool names fail the run. The complete call batch is validated
before dispatch. Expected probe/read failures become bounded tool error results
that the model can explain. Permissions are checked before and after awaited I/O;
revocation, settings changes or `/new` suppress delayed results and subsequent work.
Already completed external reads are not undone.

## Execution and persistence

Schema 16 adds `ai_runs`. A report step or Telegram job/phase supplies a stable
local run ID. Messages, tool outcomes, requested model, usage counters, turns,
summary and errors are persisted; no provider credentials are stored there.
Global administrators can inspect a run with `reports_analysis_get({id})`.

A completed local result is reused if its owning workflow resumes after a crash.
A run that was in flight at restart becomes failed, without automatically repeating
the request or its tools. Lost/failed HTTP responses are also not automatically
retried. Ask a new question or start a new report run to try again. An HTTP failure
cannot prove the remote model did no computation; Talìa does not claim to cancel
simple-ai inference remotely.

Limits per run: six model requests, at most four tool calls per response, 2,048
requested output tokens per completion, 120 seconds per HTTP request and at most
15 minutes overall (also bounded by the owner deadline). Inputs allow 16 KiB of
instructions and 64 KiB of context; accumulated messages and HTTP responses each
allow 128 KiB; one tool result allows 32 KiB; final text allows 24,000 bytes. A
truncated/malformed completion fails explicitly. A record is at most 256 KiB.

Up to four report steps and one Telegram conversation can await inference at once.
Telegram polling/delivery runs separately; database transactions never span I/O.
There is no autonomous background investigation beyond admitted work.

At most 10,000 AI runs are retained. `reports_prune` also removes older terminal
AI runs whose owning report/job is no longer active, returning `ai_removed`.
Conversation history and Telegram delivery/job retention are separate. No automatic
pruning is performed.

## Qualification

Local fixtures exercise report input selection, actual tool-call conversations,
read-only probes, forbidden calls, turn bounds, truncated/malformed responses,
credential redaction, redirects, revocation during inference, restart recovery,
report context selection, manual/automatic compaction and independent Telegram
delivery. The browser suite checks settings and administrator boundaries.
These checks do not perform live inference, send live messages or deploy Talìa.
