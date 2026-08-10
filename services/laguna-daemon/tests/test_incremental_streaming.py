from __future__ import annotations

import unittest

from laguna_daemon.responses_api.backends.mlx import _IncrementalReasoningSplitter


def drive(
    chunks: list[str], *, thinking_open: bool = True
) -> list[tuple[str, str]]:
    splitter = _IncrementalReasoningSplitter(thinking_open=thinking_open)
    events: list[tuple[str, str]] = []
    for chunk in chunks:
        events.extend(splitter.feed(chunk))
    events.extend(splitter.flush())
    return events


def merge(events: list[tuple[str, str]]) -> dict[str, str]:
    merged = {"reasoning": "", "answer": ""}
    for kind, text in events:
        merged[kind] += text
    return merged


class IncrementalReasoningSplitterTests(unittest.TestCase):
    """Text must reach the client as it is produced, not in one terminal dump.

    Regression coverage for a measured defect: a 12.7-second generation
    arrived as a single 1280-character frame because the consumer withheld
    every chunk until a `</think>` marker appeared — and this checkpoint often
    emits none, so streaming silently degraded to non-streaming.
    """

    def test_marker_free_output_streams_immediately(self) -> None:
        """The case that was buffering end to end."""
        events = drive(["The ", "answer ", "is ", "42."], thinking_open=False)
        self.assertEqual([kind for kind, _ in events], ["answer"] * 4)
        self.assertEqual(merge(events)["answer"], "The answer is 42.")

    def test_first_chunk_is_not_withheld(self) -> None:
        splitter = _IncrementalReasoningSplitter(thinking_open=False)
        emitted = splitter.feed("Hello there")
        self.assertEqual(emitted, [("answer", "Hello there")])

    def test_reasoning_streams_before_the_span_closes(self) -> None:
        splitter = _IncrementalReasoningSplitter(thinking_open=True)
        self.assertEqual(splitter.feed("<think>"), [])
        # Thinking must be visible while it is happening.
        first = splitter.feed("Let me work through this carefully")
        self.assertTrue(first)
        self.assertEqual(first[0][0], "reasoning")

    def test_reasoning_then_answer_are_classified_correctly(self) -> None:
        events = drive(["<think>", "step one ", "step two", "</think>", "Final answer."])
        merged = merge(events)
        self.assertEqual(merged["reasoning"], "step one step two")
        self.assertEqual(merged["answer"], "Final answer.")

    def test_template_opened_span_is_reasoning_without_an_opening_marker(self) -> None:
        """The checkpoint normally emits no <think>, only </think>."""
        events = drive(["Let me work ", "through it", "</think>", "The answer is 42."])
        merged = merge(events)
        self.assertEqual(merged["reasoning"], "Let me work through it")
        self.assertEqual(merged["answer"], "The answer is 42.")
        self.assertNotIn("</think>", merged["answer"])

    def test_tool_envelope_can_be_buffered_without_buffering_reasoning(self) -> None:
        splitter = _IncrementalReasoningSplitter(thinking_open=True)
        reasoning_events = splitter.feed("Inspect the workspace first. ")
        self.assertTrue(reasoning_events)
        self.assertEqual(reasoning_events[0][0], "reasoning")
        remainder = splitter.feed("</think><tool_call>list_files</tool_call>")
        merged = merge(reasoning_events + remainder + splitter.flush())
        self.assertEqual(merged["reasoning"], "Inspect the workspace first. ")
        self.assertEqual(merged["answer"], "<tool_call>list_files</tool_call>")

    def test_thinking_disabled_streams_everything_as_answer(self) -> None:
        events = drive(["Hello ", "world"], thinking_open=False)
        self.assertEqual([kind for kind, _ in events], ["answer", "answer"])

    def test_closing_marker_split_across_chunks(self) -> None:
        """A marker straddling a chunk boundary must not leak into the answer."""
        events = drive(["<think>", "thinking", "</thi", "nk>", "Answer."])
        merged = merge(events)
        self.assertEqual(merged["reasoning"], "thinking")
        self.assertEqual(merged["answer"], "Answer.")
        self.assertNotIn("</think>", merged["answer"])
        self.assertNotIn("</think>", merged["reasoning"])

    def test_opening_marker_split_across_chunks(self) -> None:
        events = drive(["<th", "ink>", "reasoning here", "</think>", "done"])
        merged = merge(events)
        self.assertEqual(merged["reasoning"], "reasoning here")
        self.assertEqual(merged["answer"], "done")

    def test_single_character_chunks(self) -> None:
        """Token-by-token delivery is the real shape of a generation."""
        text = "<think>abc</think>xyz"
        events = drive(list(text))
        merged = merge(events)
        self.assertEqual(merged["reasoning"], "abc")
        self.assertEqual(merged["answer"], "xyz")

    def test_no_marker_text_is_never_lost(self) -> None:
        for chunks in (["a"], ["<"], ["<th"], ["<think"], [""]):
            with self.subTest(chunks=chunks):
                merged = merge(drive(chunks, thinking_open=False))
                joined = merged["reasoning"] + merged["answer"]
                expected = "".join(chunks)
                self.assertEqual(joined, expected)

    def test_interleaved_think_marker_reenters_reasoning(self) -> None:
        """Laguna interleaves thinking; a mid-turn <think> is a real marker.

        The marker itself is consumed (markers never reach clients), and the
        text that follows classifies as reasoning rather than leaking into
        the assistant answer.
        """
        merged = merge(
            drive(["Answer. ", "<think>", "more planning"], thinking_open=False)
        )
        self.assertEqual(merged["answer"], "Answer. ")
        self.assertEqual(merged["reasoning"], "more planning")

    def test_unclosed_thinking_span_is_flushed_as_reasoning(self) -> None:
        """A truncated turn must still deliver what the model produced."""
        merged = merge(drive(["<think>", "half a thought"]))
        self.assertEqual(merged["reasoning"], "half a thought")
        self.assertEqual(merged["answer"], "")

    def test_answer_text_containing_angle_brackets_is_untouched(self) -> None:
        merged = merge(
            drive(["Use ", "<div> ", "and ", "</div> ", "tags."], thinking_open=False)
        )
        self.assertEqual(merged["answer"], "Use <div> and </div> tags.")
        self.assertEqual(merged["reasoning"], "")

    def test_held_back_window_is_bounded(self) -> None:
        """While thinking, at most a partial marker may be withheld."""
        splitter = _IncrementalReasoningSplitter(thinking_open=True)
        splitter.feed("<think>")
        long_chunk = "x" * 5000
        emitted = splitter.feed(long_chunk)
        streamed = sum(len(text) for _, text in emitted)
        self.assertGreaterEqual(streamed, len(long_chunk) - len("</think>"))


if __name__ == "__main__":
    unittest.main()
