import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch
import probe
import trigger


class ProbeTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.repo = self.root / 'repo with spaces'
        self.repo.mkdir()
        self.git('init', '-q')
        self.git('config', 'user.email', 'test@example.test')
        self.git('config', 'user.name', 'Test')
        (self.repo / 'tracked').write_text('initial')
        self.git('add', '.')
        self.git('commit', '-qm', 'initial')

    def git(self, *args):
        subprocess.run(['git', '-C', str(self.repo), *args], check=True, capture_output=True)

    def test_dirty_and_linked_worktrees(self):
        linked = self.root / 'linked\nworktree'
        self.git('worktree', 'add', '-qb', 'linked', str(linked))
        self.assertTrue(all(r['state'] == 'clean' for r in probe.check([str(self.repo)])))
        (linked / 'untracked').write_text('new')
        rows = probe.check([str(self.repo), str(linked)])
        self.assertEqual(len(rows), 2)
        self.assertEqual(next(r['state'] for r in rows if r['path'] == str(linked)), 'dirty')
        (self.repo / 'tracked').write_text('changed')
        self.assertEqual(next(r['state'] for r in probe.check([str(self.repo)]) if r['path'] == str(self.repo)), 'dirty')
        self.git('add', 'tracked')
        self.assertEqual(next(r['state'] for r in probe.check([str(self.repo)]) if r['path'] == str(self.repo)), 'dirty')
        (self.repo / '.git/info/exclude').write_text('ignored\n')
        self.git('commit', '-qm', 'change')
        (self.repo / 'ignored').write_text('ignored')
        self.assertEqual(next(r['state'] for r in probe.check([str(self.repo)]) if r['path'] == str(self.repo)), 'clean')

    def test_dirty_submodule(self):
        child = self.root / 'child'
        subprocess.run(['git', 'clone', '-q', str(self.repo), str(child)], check=True)
        self.git('-c', 'protocol.file.allow=always', 'submodule', 'add', str(child), 'sub')
        self.git('commit', '-qm', 'submodule')
        self.assertEqual(probe.check([str(self.repo)])[0]['state'], 'clean')
        (self.repo / 'sub/untracked').write_text('new')
        self.assertEqual(probe.check([str(self.repo)])[0]['state'], 'dirty')

    def test_manual_queue(self):
        config = self.root / 'config.json'
        config.write_text(json.dumps(dict(host='test', repositories=[str(self.repo)])))
        output = self.root / 'git.prom'
        probe.run(config, output, now=100000)
        requests = self.root / 'git-requests'
        requests.mkdir()
        result_path = str(output) + '.json'
        with patch.object(trigger.time, 'time', return_value=100001):
            first = trigger.enqueue(requests, result_path)
            self.assertEqual(trigger.enqueue(requests, result_path), first)
        result = probe.run(config, output, now=100002)
        self.assertEqual(result['checked'], 100002)
        self.assertFalse(result['automatic'])
        with patch.object(probe, 'check', side_effect=AssertionError('must consume once')):
            self.assertEqual(probe.run(config, output, now=100003), result)

    def test_missing_and_timeout(self):
        self.assertEqual(probe.check([str(self.root / 'missing')])[0]['state'], 'error')
        with patch.object(probe, 'git', side_effect=subprocess.TimeoutExpired('git', 30)):
            self.assertEqual(probe.check([str(self.repo)])[0]['state'], 'error')

    def test_daily_persistence(self):
        config = self.root / 'config.json'
        config.write_text(json.dumps(dict(host='test', repositories=[str(self.repo)])))
        output = self.root / 'git.prom'
        first = probe.run(config, output, now=100000)
        self.assertEqual(first['last_clean'], 100000)
        self.assertTrue(first['automatic'])
        (self.repo / 'untracked').write_text('new')
        with patch.object(probe, 'check', side_effect=AssertionError('must not run before due')):
            self.assertEqual(probe.run(config, output, now=100001), first)
        dirty = probe.run(config, output, now=186400)
        self.assertEqual(dirty['last_clean'], 100000)
        self.assertEqual(dirty['worktrees'][0]['state'], 'dirty')
        self.assertIn('talia_git_worktree_dirty', output.read_text())
        config.write_text(json.dumps(dict(host='test', repositories=[str(self.root / 'missing')])))
        failed = probe.run(config, output, now=186401)
        self.assertFalse(failed['success'])
        self.assertEqual(failed['last_clean'], 0)


if __name__ == '__main__':
    unittest.main()
