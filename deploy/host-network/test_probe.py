import pathlib, tempfile, unittest
from probe import collect, publish

class NetworkProbeTests(unittest.TestCase):
    def test_counters_and_missing_interface(self):
        with tempfile.TemporaryDirectory() as directory:
            root=pathlib.Path(directory); stats=root/'enp3s0'/'statistics';stats.mkdir(parents=True)
            (stats/'rx_bytes').write_text('12345');(stats/'tx_bytes').write_text('6789')
            self.assertIn('receive_bytes_total{device="enp3s0"} 12345',collect('enp3s0',root,lambda:123))
            output=root/'network.prom';publish(output,'enp3s0',root)
            self.assertIn('transmit_bytes_total{device="enp3s0"} 6789',output.read_text())
            (stats/'rx_bytes').write_text('12')  # A reset is published as the actual new counter.
            publish(output,'enp3s0',root);self.assertIn('} 12\n',output.read_text())
            (stats/'rx_bytes').unlink()
            with self.assertRaises(OSError):publish(output,'enp3s0',root)
            self.assertFalse(output.exists())
            self.assertEqual(list(root.glob('.host-network-*')),[])
    def test_interface_validation(self):
        with self.assertRaises(ValueError):collect('../other')

if __name__=='__main__':unittest.main()
