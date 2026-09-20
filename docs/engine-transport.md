# Durable engine transport v1

Run `cargo run --manifest-path engine/Cargo.toml -- DATABASE PORT --seed`.
The service binds loopback only; authentication and deployment are later stories.
An exclusive OS file lock prevents two servers from owning the same database.
`POST /engine` accepts `{version:1,client,epoch,incarnation,op,args}`.
The `hello` operation obtains the new server incarnation and a consistent snapshot.
All other operations require that incarnation; client epochs increase on runtime
replacement. Responses echo both so clients can reject obsolete completions.

Operations are hello, snapshot, read, subscribe, poll, unsubscribe, write, set,
status, define, create, parameters, remove, removeDefinition, invalidate and history.
Writes require expected instance revision, tagged value and stable actionId.
Set invokes the computed setter. Status never dispatches work. A duplicate action
with identical arguments returns its recorded outcome; changed arguments fail.
Accepted work is detached from HTTP connection lifetime. Crash between an external
effect and outcome persistence leaves unknown; no automatic replay is attempted.

Subscriptions use client/instance leases renewed by poll and expire after 60 seconds.
A snapshot is read synchronously between engine commits. Polling coalesces updates;
this is state delivery, not an event log. Clients replace their server snapshot on
incarnation changes and only compare revisions within that incarnation. After
reconnect, restore subscriptions and reconcile action IDs before indicating recovery.
Running JS heaps/subscriptions are recreated; stored values retain original age.
The bounded development service allows 256 client identities, 1024 leases and 128
active requests. Production identity/authentication remains a separate boundary.
