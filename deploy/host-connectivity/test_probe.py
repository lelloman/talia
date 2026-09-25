import importlib.util
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch
spec = importlib.util.spec_from_file_location('probe', Path(__file__).with_name('probe.py'))
probe = importlib.util.module_from_spec(spec)
spec.loader.exec_module(probe)

class ProbeTests(unittest.TestCase):
    def test_failures_and_real_result(self):
        with patch.object(probe.subprocess, 'run', return_value=subprocess.CompletedProcess([], 0, b'1.2.3.4 host')):
            self.assertTrue(probe.check('dns', 'google'))
        with patch.object(probe.subprocess, 'run', return_value=subprocess.CompletedProcess([], 0, b'')):
            self.assertFalse(probe.check('dns', 'google'))
            self.assertTrue(probe.check('https', 'google'))
        for error in [FileNotFoundError(), subprocess.TimeoutExpired('curl', 10)]:
            with patch.object(probe.subprocess, 'run', side_effect=error):
                self.assertFalse(probe.check('https', 'google'))
    def test_aggregate_and_atomic_replace(self):
        text = probe.render('VPS-EU', {'dns': {'google': False, 'cloudflare': True}, 'https': {'google': False, 'cloudflare': False}}, 1234)
        self.assertIn('talia_report_probe_success{host="VPS-EU",kind="dns"} 1', text)
        self.assertIn('talia_report_probe_success{host="VPS-EU",kind="https"} 0', text)
        self.assertIn('checked_timestamp_seconds{host="VPS-EU",kind="dns"} 1234.000', text)
        with tempfile.TemporaryDirectory() as d:
            p = Path(d) / 'connectivity.prom'
            probe.atomic_write(p, 'old success')
            probe.atomic_write(p, text)
            self.assertEqual(p.read_text(), text)
            self.assertEqual(list(Path(d).iterdir()), [p])
if __name__ == '__main__': unittest.main()
