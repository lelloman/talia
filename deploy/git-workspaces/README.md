# Daily Git workspace checks

Homelab's dashboard shows a green check when every configured repository's
worktrees are clean, or a warning with a short list of dirty/unreadable paths.
It shows the latest check time and retains the last successful all-clean time.
Staged changes, unstaged changes, untracked files and dirty submodules count;
ignored files do not. Linked worktrees are discovered with `git worktree list`
on every run, deduplicated and checked too. Missing repositories, inaccessible
worktrees and Git timeouts are failures, never clean results.

The explicit per-host repository list lives in JSON; the initial Homelab list is
[homelab.json](homelab.json). It includes all 11 repositories discovered beneath
`/home/lelloman`, `/opt` and `/srv` on 2026-10-04, including plugin checkouts.
New independent repositories must be added to this list. No automatic repository
discovery runs in the scheduled probe.

## Collection and installation

`probe.py` runs as the repository owner, with optional Git index locks disabled,
no inherited `GIT_*` overrides, and a 30-second timeout per Git invocation.
It changes no Git files. The installer uses an existing node_exporter textfile
directory; there is no new network listener and no service restart.

Copy `probe.py`, `install.py` and the host's JSON configuration into one directory
on that host, then run as the repository owner:

```sh
python3 install.py --config homelab.json --directory /home/lelloman/homelab-data/report-probes
```

The installer backs up the crontab and previous configuration and preserves other
cron jobs. The installed files on Homelab are:

- `~/.local/bin/talia-git-workspaces`: probe script.
- `~/homelab-data/report-probes/git-workspaces-config.json`: editable repository list.
- `~/homelab-data/report-probes/git-workspaces.prom.json`: retained results and last all-clean timestamp.
- `~/homelab-data/report-probes/git-workspaces.prom`: atomically replaced metrics.

Cron checks the persisted deadline every minute under `flock`; it actually runs
Git once every elapsed 24 hours, including across restarts. A changed repository
list triggers a fresh check on the next minute and resets the historical clean
timestamp for the old list. Missed runs execute once when cron resumes. Results
older than 24 hours plus five minutes of collection grace warn as overdue.
Invalid configuration or an interrupted probe leaves the previous timestamp,
which then becomes overdue. Never delete the result JSON to refresh the check; use the dashboard’s **Check now**
button, or `--force` with the same lock from a host shell.

## Dashboard configuration

The existing `host-collect` pipeline reads `talia_git_*` metrics filtered by the
host's exact Prometheus job, instance and host label. Only host instances with
`gitWorkspaces: true` enable this query/card. The initial configuration enables
Homelab alone. Collection failures are isolated from CPU, memory and disk data.
Stale host samples, unreachable monitoring, incomplete Git results and overdue
probes cannot render a green check.

The per-host list is managed in the collector JSON, not through an in-app editor.
To add another host, install the collector with its own list and host name, ensure
its node_exporter scrapes that textfile directory, then set `gitWorkspaces: true`
on its `host-collect` instance. No broader dashboard permissions are needed.

Shared sources are `dashboard/examples/host/{collect.js,present.js,layout.ui}`;
`deploy/host-dashboards.mjs` captures the initial configuration. Follow
[host-dashboard publishing](../host-dashboards.md) to preserve current definitions
and grants when updating. Open dashboards adopt the new package on Reload.
Daily Telegram report definitions are unchanged.

## Verification and deployment

```sh
python3 deploy/git-workspaces/test_probe.py
node dashboard/tests/git-workspaces.mjs
node dashboard/tests/host-template.mjs
node dashboard/tests/host-browser.mjs
```

Tests exercise actual Git repositories, staged/unstaged/untracked changes,
linked worktrees with unusual paths, missing repositories, timeouts, persistent
24-hour scheduling, clean timestamp retention, stale/failed results and responsive
host dashboards.

Published on 2026-10-04 at catalog revision 31, with `host-collect` version 6.
The first live collection inspected all 11 configured repositories/worktrees;
`crumbles`, `homelab` and `pezzottify` were dirty. Prometheus ingestion and the
live `host-homelab` sample were verified. Private pre-change definitions and
verification evidence are retained in `.local/git-workspaces/`.

To disable, remove only the cron line ending `# talia-git-workspaces` and set
`gitWorkspaces: false` on that host's collection instance. Retain the result JSON
if preserving the last clean timestamp matters. To revert the shared UI, save the
backed-up definitions against the current catalog revision, preserving newer edits.

## Manual checks from the dashboard

Homelab's repository card has **Check now**. It starts the unscheduled
`git-recheck-homelab` pipeline, disables the button while the server run is active,
and shows completion or failure. Active runs coalesce across clients; their status
survives a dashboard reload. Cron picks up a queued request within one minute;
Prometheus and the dashboard normally show the result within another 30 seconds.
An actual manual check also resets the next automatic check to 24 hours later.

`trigger.py` exposes only `POST /check` (no body) and `GET /status`. It cannot accept
commands, repository paths or configuration. It writes a coalesced request to
`git-requests/request.json`; the host's existing cron probe consumes that request
under its existing `flock`. The trigger has read-only access to results and write
access only to the request directory. The pipeline waits up to 170 seconds for a
fresh completed result, then reports success or failure independently of whether
the repositories are dirty. Dirty results remain a warning on the card.

The trigger runs as an unprivileged container, without a published port, on the
internal `talia-git-probe` Docker network shared only with Talìa. It has no
repository mounts, Docker socket or shell-execution API. Install the supplied
Compose file and `probe.py`/`trigger.py` together, create the request directory
owned by the repository user, and create the network with
`docker network create --internal talia-git-probe`. Match the Compose UID/GID and
paths to the host. Attach Talìa to that network and persist it in Talìa's Compose
configuration before starting the trigger. No bearer token is required on this
isolated two-container network; do not attach it to a public/shared network.

The dashboard has exactly two extra permissions: read
`monitor.git-recheck-homelab` and run `git-recheck-homelab`. Add those exact grants
to the deployment ceiling with offline `talia-agent --policy-only`, preserving all
current grants and policy versions, before saving the catalog changes generated
by `deploy/host-dashboards.mjs`. Back up both databases, check for active work and
briefly stop/start Talìa for offline policy provisioning. No engine or client
binary changes are required.

Deployed 2026-10-04 at catalog revision 33 and operator policy version 4. Both
SQLite backups passed integrity checks; Talìa recovered healthy after the permission
update. Deployment files are in `~/homelab-data/git-probe-trigger` on Homelab.
Local before/after evidence is in `.local/git-recheck/`. Unit tests cover queue
coalescing, one-time consumption, button dispatch and run outcomes; browser tests
exercise the button at mobile and desktop sizes.

To remove the button, remove the dashboard's action parameter and extra grants,
then the dedicated pipeline instance/definition and source. Remove the trigger
container/network and the matching deployment-ceiling entries after grants no
longer refer to them. The daily cron probe continues to work without the trigger.
