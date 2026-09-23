# Durable engine storage

The P2 engine uses SQLite with WAL, synchronous=FULL and foreign keys enabled.
Schema versions 2–4 add monitoring configuration, state, runs, request identities
and ordered observations transactionally to version 1. Existing P2 data is preserved;
newer unsupported schemas are rejected.
A synchronous storage call returns only after commit. It never holds a transaction
across a script await, network request or external effect. OS/filesystem durability
still depends on the storage device honoring synchronization.

Definitions, instance parameters, private state, latest results and original
measurement timestamps/quality persist. History requires both a positive count
and age bound; zero disables retention. Reads filter expired history, and writes
prune expired/excess samples. Idle expired rows may remain physically until the
next write; no expired samples are returned. History stores only measurement values and metadata, not private state.

The online backup uses SQLite's backup API into a new destination. To restore,
stop the engine and open the backup as its database; never copy a live main file
without its WAL. Backup overwrite is rejected. Process-kill tests exercise both
uncommitted rollback and committed results whose reply was never observed.

Accepted actions have durable identities. A restart marks unfinished accepted
actions unknown. The engine never interprets that state as permission to retry an
external effect. Action tombstones are retained without automatic expiry in v1;
this prevents an expired identity from accidentally dispatching again.

The installation AI account (schema 17) stores only encrypted OAuth credentials.
Back up `DATABASE.ai-key` (or `TALIA_AI_KEY_FILE`) along with SQLite. Restoring an
older database may restore already-rotated refresh tokens; reconnect the dedicated
account after such a restore. Never run two installations from the same account
session backup. See [AI account setup](ai.md).
