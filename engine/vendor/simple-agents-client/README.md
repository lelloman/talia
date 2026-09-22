# Simple Agents Rust client

`simple-agents-client` uses the public `simple-agents-protocol` types and depends
on neither the service nor Crumbles. The client supports submission, lookup by
key, session listing/lookup, event replay, commands and receipt reconciliation,
results and artifact listing/download. It can be used while the service is
running elsewhere.

Add a dependency from a pinned revision of the Simple Agents repository:

```toml
[dependencies]
simple-agents-client = { git = "https://fucina.homelab/lelloman/simple-agents", rev = "<reviewed-full-commit>" }
```

Alternatively build `python3 scripts/build-client.py /tmp/simple-agents-client.tar.gz`
from a reviewed checkout. The deterministic source archive includes only the
client/protocol crates, contract fixtures, license, and their locked dependency
closure. It contains `source.json` (commit and dirty-checkout marker) and
`SHA256SUMS`. Verify the archive checksum from your trusted delivery channel and
then its inventory with `sha256sum -c SHA256SUMS` after extraction. A release
should report `dirty: false`. Build/test the extracted workspace with
`cargo test --locked --workspace`. It requires Rust and access to the locked
registry dependencies (or a populated Cargo cache), but no service checkout.
This is a source distribution; it does not publish either crate to crates.io.
The bundler requires Python 3.11+, Cargo and a populated dependency cache. It
normalizes the standalone lockfile offline and rejects any package version or
checksum absent from the repository lockfile.

In another workspace, use a path dependency on the extracted
`crates/simple-agents-client` directory. The adjacent protocol crate and root
manifest must stay together for Cargo workspace inheritance.

```rust,no_run
use simple_agents_client::{Client, Options, protocol::{Id, SubmitSession, MAX_PAGE_SIZE}};

async fn example(origin: &str, token: &str, work: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
    let client = Client::new(origin, token, Id::new("my-caller")?, Options::default())?;
    let request = SubmitSession::decode(work)?;
    let submitted = client.submit(&request).await?;
    let page = client.events(&submitted.session.session_id, 0, MAX_PAGE_SIZE).await?;
    // Apply the events and persist page.next_after in the same transaction.
    // On restart, pass that durable cursor to events() again.
    Ok(())
}
```

Credentials are supplied by the application from its secret store. HTTPS is
required, except for numeric loopback HTTP addresses. Redirects and automatic
HTTP retries are disabled. Client debug output and errors exclude tokens,
request text and response bodies. Reconstruct the client when rotating a token.
The service revalidates that credential on every call.

Mutations make one HTTP attempt with the key and resource version provided by
the application. A transport error, malformed response, or interrupted future
can leave a mutation's outcome unknown. For submission, call `by_key` with the
original request; if necessary, submit the exact original request again. The
lookup checks caller, source, profile, capabilities, bindings and budget, but
cannot compare work text because a session response does not expose that text.
Exact submission replay lets the service check the complete fingerprint.
For controls, query a known command ID or replay the exact original command
with its original key and version. Never generate a replacement key as an
automatic retry. `retryable` on an HTTP error is a hint for reconciliation, not
permission to repeat external effects.

All page methods accept limits from 1 through `MAX_PAGE_SIZE` (100), matching
the service. Session and artifact lists use an exclusive `after` ID and expose
`next_cursor`; they are live authorized lists, not stable snapshots. The client
checks ordering, duplicate IDs and continuation. Event pages enforce consecutive
sequences and the requested session/version. A rejected page never advances a
client cursor. The caller owns durable cursor storage and deduplication if a
transaction is replayed. This initial client uses HTTP event pages for recovery;
it does not wrap the SSE transport or administrative endpoints.

Response bodies are bounded as they arrive, including chunked responses.
Defaults are 16 MiB for JSON and 8 MiB for artifact downloads; `Options` allows
smaller or larger application-specific ceilings and request timeouts. Oversized
JSON pages can be retried as reads with a smaller page limit. Artifact downloads
check the exact size and SHA-256 against supplied session metadata before
returning bytes. They never write files. Artifact bytes are served as downloads
with `application/octet-stream`; use the metadata's media type for interpretation.

`result()` returns the typed result, exact bytes and SHA-256. Applications still
own terminal-event/result comparison, external-effect reconciliation and any
domain handoff. An uncertain effect receipt must not be treated as success.

The service's `public_client` test drives this crate over a real HTTP listener
through concurrent submission, replay, paging, cancellation, restart and
revocation. The client's `boundaries` suite probes malformed responses,
redirects, size limits, sequence gaps, artifact corruption and mutation retries.
The `faults` suite uses raw loopback TCP peers to drop mutation responses, stall
headers and bodies, truncate fixed-length and chunked downloads, and exceed
streaming bounds without a Content-Length or end marker. It verifies single
mutation attempts with unchanged keys/versions and explicit read recovery from
the original event cursor. These are client transport tests, not proof of
server-side exactly-once effects. Run them with
`cargo test --locked -p simple-agents-client --test faults`.
The service's `public_client` suite also drops confirmed successful upstream
submission and cancellation responses before delivery to the client. It checks
by-key reconciliation and concurrent exact replay, immediately and after a
database reopen: the same session/command IDs, no additional durable rows or
events, and unchanged terminal results. Cancellation uses a queued session;
this does not establish exactly-once behavior for external worker effects.
