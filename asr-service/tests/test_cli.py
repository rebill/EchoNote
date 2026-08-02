from __future__ import annotations

import contextlib
import io
import unittest

from echonote_asr.__main__ import LOCAL_HOST, parse_args


class LocalCliTest(unittest.TestCase):
    def test_defaults_to_ipv4_loopback(self) -> None:
        args = parse_args([])

        self.assertEqual(args.host, LOCAL_HOST)
        self.assertEqual(args.cpu_threads, 0)

    def test_accepts_the_windows_cpu_backend(self) -> None:
        args = parse_args(
            [
                "--backend",
                "faster-whisper",
                "--model",
                r"C:\EchoNote\models\faster-whisper-small",
                "--cpu-threads",
                "8",
            ]
        )

        self.assertEqual(args.backend, "faster-whisper")
        self.assertEqual(args.cpu_threads, 8)

    def test_rejects_non_loopback_bind_addresses(self) -> None:
        with contextlib.redirect_stderr(io.StringIO()):
            with self.assertRaises(SystemExit):
                parse_args(["--host", "0.0.0.0"])

    def test_rejects_negative_cpu_threads(self) -> None:
        with contextlib.redirect_stderr(io.StringIO()):
            with self.assertRaises(SystemExit):
                parse_args(["--cpu-threads", "-1"])


if __name__ == "__main__":
    unittest.main()
