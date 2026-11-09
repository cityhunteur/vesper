"""Reject malformed microVM requests before creating a session or invoking a runtime."""
import json
from pathlib import Path
import subprocess
import unittest

SCRIPT = Path(__file__).resolve().parents[2] / "systemvm/overlay/usr/local/bin/vesper-guest-session"


class GuestSessionTests(unittest.TestCase):
    def test_invalid_requests(self):
        for request in (
            "RUN nope 1 0", "RUN 0 1 0", "RUN 1029 1 0", "RUN 65532 1 0",
            "RUN 1028 1048577 0", "RUN 1028 -1 0", "RUN 1028 1 2",
            "OTHER 1028 1 0", "SIGNAL 1028 bad 0", "SIGNAL 1028 65 0",
        ):
            with self.subTest(request=request):
                result = subprocess.run(["sh", str(SCRIPT)], input=request + "\n", text=True, capture_output=True)
                self.assertNotEqual(result.returncode, 0)
                self.assertTrue(result.stdout.startswith("ERROR "), result)
                error = json.loads(result.stdout.removeprefix("ERROR "))
                self.assertIn("stage", error)
                self.assertIn("message", error)

    def test_shell_syntax(self):
        for name in ("vesper-microvm", "vesper-microvm-init", "vesper-guest-session", "vesper-ctr", "vesper-ptyrun"):
            subprocess.run(["sh", "-n", str(SCRIPT.with_name(name))], check=True)
