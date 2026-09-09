/** Word-level LCS diff for parent-vs-candidate prompt levers. */

export type DiffSegment = { type: "same" | "add" | "del"; text: string };

function tokenize(text: string): string[] {
  return text.split(/(\s+)/).filter((token) => token.length > 0);
}

export function wordDiff(before: string, after: string): DiffSegment[] {
  const a = tokenize(before);
  const b = tokenize(after);
  // LCS table; prompts are a few hundred tokens, so O(n·m) is fine.
  const rows = a.length + 1;
  const cols = b.length + 1;
  const table = new Uint32Array(rows * cols);
  for (let i = a.length - 1; i >= 0; i -= 1) {
    for (let j = b.length - 1; j >= 0; j -= 1) {
      table[i * cols + j] = a[i] === b[j]
        ? table[(i + 1) * cols + j + 1] + 1
        : Math.max(table[(i + 1) * cols + j], table[i * cols + j + 1]);
    }
  }
  const segments: DiffSegment[] = [];
  const push = (type: DiffSegment["type"], text: string) => {
    const last = segments.at(-1);
    if (last && last.type === type) last.text += text;
    else segments.push({ type, text });
  };
  let i = 0;
  let j = 0;
  while (i < a.length && j < b.length) {
    if (a[i] === b[j]) {
      push("same", a[i]);
      i += 1;
      j += 1;
    } else if (table[(i + 1) * cols + j] >= table[i * cols + j + 1]) {
      push("del", a[i]);
      i += 1;
    } else {
      push("add", b[j]);
      j += 1;
    }
  }
  while (i < a.length) { push("del", a[i]); i += 1; }
  while (j < b.length) { push("add", b[j]); j += 1; }
  return segments;
}
