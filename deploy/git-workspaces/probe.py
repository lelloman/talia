#!/usr/bin/env python3
"""Read-only daily checks of explicitly configured repositories and their worktrees."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import tempfile
import time

INTERVAL = 86400


def git(path, *args):
    env = {k: v for k, v in os.environ.items() if not k.startswith('GIT_')}
    env['GIT_OPTIONAL_LOCKS'] = '0'
    return subprocess.run(['git', '-C', str(path), *args], env=env,
                          stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                          check=True, timeout=30).stdout


def check(repositories):
    results = {}
    for repository in repositories:
        try:
            records = git(repository, 'worktree', 'list', '--porcelain', '-z').split(b'\0\0')
            paths = []
            for record in records:
                fields = record.split(b'\0')
                # Bare repositories have no workspace; linked worktrees still count.
                if b'bare' in fields:
                    continue
                paths.extend(os.fsdecode(f[9:]) for f in fields if f.startswith(b'worktree '))
            if not paths:
                raise ValueError('No worktrees')
            for path in paths:
                if path in results:
                    continue
                try:
                    # All untracked files and submodule modifications count; ignored files do not.
                    status = git(path, 'status', '--porcelain=v1', '-z', '--untracked-files=all', '--ignore-submodules=none')
                    results[path] = {'path': path, 'state': 'dirty' if status else 'clean'}
                except (subprocess.SubprocessError, OSError):
                    results[path] = {'path': path, 'state': 'error'}
        except (subprocess.SubprocessError, OSError, ValueError):
            results[repository] = {'path': repository, 'state': 'error'}
    return sorted(results.values(), key=lambda r: r['path'])


def atomic(path, text):
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    fd, name = tempfile.mkstemp(dir=path.parent, prefix='.' + path.name)
    try:
        with os.fdopen(fd, 'w') as f:
            f.write(text)
        os.chmod(name, 0o644)
        os.replace(name, path)
    finally:
        if os.path.exists(name):
            os.unlink(name)


def metrics(result):
    def label(value):
        # Prometheus accepts UTF-8 and only backslash, quote and newline escapes.
        return '"' + value.replace('\\', '\\\\').replace('"', '\\"').replace('\n', '\\n') + '"'
    host = 'host=' + label(result['host'])
    lines = []
    for name, value in [('checked_timestamp_seconds', result['checked']),
                        ('last_clean_timestamp_seconds', result['last_clean']),
                        ('success', int(result['success'])),
                        ('repositories', len(result['repositories']))]:
        lines.append('talia_git_' + name + '{' + host + '} ' + str(value))
    for row in result['worktrees']:
        labels = host + ',path=' + label(row['path'])
        lines.append('talia_git_worktree_dirty{' + labels + '} ' + str(int(row['state'] == 'dirty')))
        lines.append('talia_git_worktree_error{' + labels + '} ' + str(int(row['state'] == 'error')))
    return '\n'.join(lines) + '\n'


def run(config_path, output, force=False, now=None):
    config = json.loads(Path(config_path).read_text())
    repositories = config['repositories']
    if not isinstance(config['host'], str) or not isinstance(repositories, list) or not repositories or not all(isinstance(p, str) and p.startswith('/') for p in repositories):
        raise ValueError('Configure a host and a nonempty list of absolute repository paths')
    if len(repositories) > 256 or len(set(repositories)) != len(repositories):
        raise ValueError('Repository list must be unique and contain at most 256 paths')
    state_path = Path(str(output) + '.json')
    try:
        previous = json.loads(state_path.read_text())
    except (OSError, ValueError):
        previous = {}
    now = time.time() if now is None else now
    try:
        request = json.loads((Path(output).parent / 'git-requests/request.json').read_text())
        requested = request.get('requested', 0)
        force = force or (isinstance(requested, (int, float)) and previous.get('checked', 0) < requested <= now)
    except (OSError, ValueError):
        pass
    same = previous.get('host') == config['host'] and previous.get('repositories') == repositories
    if not force and same and 0 <= now - previous.get('checked', 0) < INTERVAL:
        # Repair a missing metrics file without running Git again.
        atomic(output, metrics(previous))
        return previous
    worktrees = check(repositories)
    success = bool(worktrees) and all(r['state'] != 'error' for r in worktrees)
    clean = success and all(r['state'] == 'clean' for r in worktrees)
    result = dict(host=config['host'], repositories=repositories, checked=now,
                  last_clean=now if clean else previous.get('last_clean', 0) if same else 0,
                  success=success, worktrees=worktrees)
    atomic(state_path, json.dumps(result, indent=2) + '\n')
    atomic(output, metrics(result))
    return result


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--config', required=True)
    parser.add_argument('--output', required=True)
    parser.add_argument('--force', action='store_true')
    args = parser.parse_args()
    run(args.config, args.output, args.force)
