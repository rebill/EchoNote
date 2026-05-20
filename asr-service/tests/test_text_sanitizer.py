from __future__ import annotations

import unittest

from echonote_asr.text_sanitizer import sanitize_transcript_text


class TextSanitizerTest(unittest.TestCase):
    def test_collapses_runaway_repetition(self) -> None:
        repeated = "因为这块的，呃，客户通过四点零的资源也可以盘活。"
        text = f"主要是他可以，就有一个优点，就是四点零的资源也可以盘活。{repeated * 20}因为这块的，呃，客户通过"

        sanitized = sanitize_transcript_text(text)

        self.assertEqual(sanitized, f"主要是他可以，就有一个优点，就是四点零的资源也可以盘活。{repeated}")

    def test_preserves_normal_repetition(self) -> None:
        text = "对对对，这个可以。嗯嗯，我们先按五十台测算，然后再看后续需求。"

        self.assertEqual(sanitize_transcript_text(text), text)

    def test_caps_unstructured_runaway_output(self) -> None:
        text = "没有明显循环边界" * 500

        self.assertLessEqual(len(sanitize_transcript_text(text)), 3000)


if __name__ == "__main__":
    unittest.main()
