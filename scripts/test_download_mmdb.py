"""Offline installer checks: failures must preserve the installed database."""
import gzip
import hashlib
import importlib.util
import io
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("download_mmdb", Path(__file__).with_name("download-mmdb.py"))
installer = importlib.util.module_from_spec(spec)
spec.loader.exec_module(installer)


class InstallTests(unittest.TestCase):
    def check_install(self, archive, expected=None, limit=installer.MAX_BYTES, fails=False):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "current.mmdb"
            output.write_bytes(b"previous database")
            with patch.object(installer.urllib.request, "urlopen", return_value=io.BytesIO(archive)):
                args = ("2026-11", expected or hashlib.sha256(archive).hexdigest(), output, limit)
                if fails:
                    with self.assertRaises((ValueError, OSError, EOFError)):
                        installer.install(*args)
                    self.assertEqual(output.read_bytes(), b"previous database")
                else:
                    installer.install(*args)
                    self.assertEqual(output.read_bytes(), gzip.decompress(archive))
            self.assertEqual(list(Path(directory).iterdir()), [output])

    def test_installs_verified_fixture(self):
        data = (Path(__file__).resolve().parent.parent / "tests/fixtures/GeoIP2-City-Test.mmdb").read_bytes()
        self.check_install(gzip.compress(data))

    def test_checksum_failure_preserves_previous_file(self):
        self.check_install(gzip.compress(b"data"), expected="0" * 64, fails=True)

    def test_truncated_gzip_preserves_previous_file(self):
        self.check_install(gzip.compress(b"data")[:-4], fails=True)

    def test_invalid_mmdb_preserves_previous_file(self):
        self.check_install(gzip.compress(b"not an mmdb"), fails=True)

    def test_compressed_size_limit(self):
        self.check_install(gzip.compress(b"data"), limit=8, fails=True)

    def test_decompressed_size_limit(self):
        self.check_install(gzip.compress(b"x" * 4096), limit=128, fails=True)

    def test_invalid_inputs_do_not_download(self):
        with patch.object(installer.urllib.request, "urlopen") as download:
            for release, digest in [("../../other", "0" * 64), ("2026-13", "0" * 64), ("2026-11", "invalid")]:
                with self.assertRaises(ValueError):
                    installer.install(release, digest, "unused.mmdb")
            download.assert_not_called()


if __name__ == "__main__":
    unittest.main()
