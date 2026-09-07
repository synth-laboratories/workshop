"""Recover uniquely located prose quotes without accepting changed wording."""
import re
import textwrap


def recover_indentation(text, quote):
    """Only a uniform block indent may differ; tokens and line breaks may not."""
    if not isinstance(quote, str) or not quote.strip():
        return None
    if quote in text:
        return quote
    proposed = quote.strip('\n')
    lines = text.splitlines(keepends=True)
    count = len(proposed.splitlines())
    if not 1 <= count <= 40:
        return None
    matches = []
    for start in range(len(lines) - count + 1):
        candidate = ''.join(lines[start:start + count]).rstrip('\r\n')
        if textwrap.dedent(candidate) == textwrap.dedent(proposed):
            matches.append(candidate)
    return matches[0] if len(matches) == 1 else None


def recover(text, quote):
    if not isinstance(quote, str) or not quote.strip():
        return None
    if quote in text:
        return quote
    # Models sometimes flatten a wrapped bullet, or add its list marker to a
    # continuation sentence. Only prose relevance citations use this helper;
    # code, observations, and defect evidence remain exact-match validated.
    candidate = re.sub(r'^\s*[-*+]\s+', '', quote).strip()
    words = candidate.split()
    if len(words) < 4:
        return None
    pattern = r'\s+'.join(re.escape(word) for word in words)
    matches = list(re.finditer(pattern, text))
    if len(matches) != 1:
        return None
    return matches[0].group(0)
