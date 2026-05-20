from __future__ import annotations

MIN_REPEATED_UNIT_CHARS = 8
MAX_REPEATED_UNIT_CHARS = 120
MIN_REPETITIONS = 4
MAX_SEGMENT_CHARS = 3000


def sanitize_transcript_text(text: str) -> str:
    normalized = text.strip()
    if not normalized:
        return ""

    collapsed = _collapse_repeated_runs(_collapse_repeated_sentences(normalized)).strip()
    if len(collapsed) <= MAX_SEGMENT_CHARS:
        return collapsed

    return _trim_at_boundary(collapsed, MAX_SEGMENT_CHARS)


def _collapse_repeated_sentences(text: str) -> str:
    sentences = _split_sentences(text)
    if len(sentences) < MIN_REPETITIONS:
        return text

    output: list[str] = []
    index = 0
    while index < len(sentences):
        sentence = sentences[index]
        repetitions = 1
        while index + repetitions < len(sentences) and sentences[index + repetitions] == sentence:
            repetitions += 1

        index += repetitions
        if repetitions >= MIN_REPETITIONS:
            output.append(sentence)
            if index < len(sentences) and sentence.startswith(sentences[index]):
                index += 1
            continue

        output.extend([sentence] * repetitions)

    return "".join(output)


def _split_sentences(text: str) -> list[str]:
    sentences: list[str] = []
    start = 0
    for index, char in enumerate(text):
        if char not in "。！？.!?":
            continue
        sentences.append(text[start : index + 1])
        start = index + 1
    if start < len(text):
        sentences.append(text[start:])
    return sentences


def _collapse_repeated_runs(text: str) -> str:
    output: list[str] = []
    index = 0
    while index < len(text):
        match = _find_repeated_run(text, index)
        if match is None:
            output.append(text[index])
            index += 1
            continue

        unit_length, repetitions = match
        output.append(text[index : index + unit_length])
        index += unit_length * repetitions

    return "".join(output)


def _find_repeated_run(text: str, index: int) -> tuple[int, int] | None:
    remaining = len(text) - index
    max_unit_length = min(MAX_REPEATED_UNIT_CHARS, remaining // MIN_REPETITIONS)

    for unit_length in range(MIN_REPEATED_UNIT_CHARS, max_unit_length + 1):
        unit = text[index : index + unit_length]
        if not unit.strip():
            continue

        repetitions = 1
        cursor = index + unit_length
        while text.startswith(unit, cursor):
            repetitions += 1
            cursor += unit_length

        if repetitions < MIN_REPETITIONS:
            continue

        return unit_length, repetitions

    return None


def _trim_at_boundary(text: str, max_chars: int) -> str:
    candidate = text[:max_chars].rstrip()
    for boundary in ("。", "！", "？", ".", "!", "?"):
        position = candidate.rfind(boundary)
        if position >= max_chars // 2:
            return candidate[: position + 1]
    return candidate
