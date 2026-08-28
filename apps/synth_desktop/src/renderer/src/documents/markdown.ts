/**
 * Markdown → node tree. No dependency, and no HTML.
 *
 * Two reasons this is written rather than installed. The build cannot add a
 * package right now, so an unverifiable dependency would be a claim rather
 * than a fact. And every renderer worth installing hands back an HTML string,
 * which in this pane would mean `dangerouslySetInnerHTML` over bytes read from
 * a file an agent chose — the exact shape the visuals boundary exists to
 * refuse. This produces a typed tree the pane renders as React elements, so a
 * `<script>` in a README is text, structurally, and not by sanitizer.
 *
 * The subset is the one technical documents actually use: ATX and setext
 * headings, fenced code with an info string, block quotes, ordered and
 * unordered lists with task boxes and one level of nesting, GFM tables,
 * thematic breaks, and inline code / strong / emphasis / strikethrough /
 * links / images / autolinks. Anything unrecognized stays a paragraph of
 * literal text rather than disappearing.
 */

export type InlineNode =
	| { type: "text"; value: string }
	| { type: "code"; value: string }
	| { type: "strong"; children: InlineNode[] }
	| { type: "em"; children: InlineNode[] }
	| { type: "strike"; children: InlineNode[] }
	| { type: "link"; href: string; children: InlineNode[] }
	| { type: "image"; src: string; alt: string };

export type TableAlign = "left" | "center" | "right" | null;

export type ListItem = {
	/** `null` when the item is not a task list item. */
	checked: boolean | null;
	blocks: Block[];
};

export type Block =
	| { type: "heading"; depth: 1 | 2 | 3 | 4 | 5 | 6; slug: string; children: InlineNode[] }
	| { type: "paragraph"; children: InlineNode[] }
	| { type: "code"; language: string; value: string }
	| { type: "quote"; blocks: Block[] }
	| { type: "list"; ordered: boolean; start: number; items: ListItem[] }
	| { type: "table"; head: InlineNode[][]; align: TableAlign[]; rows: InlineNode[][][] }
	| { type: "rule" };

const HEADING = /^(#{1,6})\s+(.*?)\s*#*\s*$/;
const FENCE = /^(```+|~~~+)\s*([^\s`]*)/;
const RULE = /^ {0,3}([-*_])(?:\s*\1){2,}\s*$/;
const UNORDERED = /^(\s*)[-*+]\s+(.*)$/;
const ORDERED = /^(\s*)(\d{1,9})[.)]\s+(.*)$/;
const QUOTE = /^ {0,3}>\s?(.*)$/;
const TASK = /^\[([ xX])\]\s+(.*)$/;
const SETEXT = /^ {0,3}(=+|-+)\s*$/;
const TABLE_DIVIDER = /^\s*\|?\s*:?-{1,}:?\s*(\|\s*:?-{1,}:?\s*)*\|?\s*$/;

/** Stable anchor id for a heading, so a table of contents can address it. */
export function slugify(text: string): string {
	return text
		.toLowerCase()
		.replace(/[^\w\s-]/g, "")
		.trim()
		.replace(/\s+/g, "-")
		.slice(0, 80);
}

function inlineText(nodes: InlineNode[]): string {
	return nodes
		.map((node) => {
			switch (node.type) {
				case "text":
				case "code":
					return node.value;
				case "image":
					return node.alt;
				default:
					return inlineText(node.children);
			}
		})
		.join("");
}

function splitRow(line: string): string[] {
	const trimmed = line.trim().replace(/^\|/, "").replace(/\|$/, "");
	const cells: string[] = [];
	let current = "";
	let escaped = false;
	for (const character of trimmed) {
		if (escaped) {
			current += character;
			escaped = false;
			continue;
		}
		if (character === "\\") {
			escaped = true;
			continue;
		}
		if (character === "|") {
			cells.push(current.trim());
			current = "";
			continue;
		}
		current += character;
	}
	cells.push(current.trim());
	return cells;
}

function alignments(divider: string): TableAlign[] {
	return splitRow(divider).map((cell) => {
		const left = cell.startsWith(":");
		const right = cell.endsWith(":");
		if (left && right) return "center";
		if (right) return "right";
		if (left) return "left";
		return null;
	});
}

export function parseMarkdown(source: string): Block[] {
	const lines = source.replace(/\r\n?/g, "\n").split("\n");
	return parseBlocks(lines);
}

function parseBlocks(lines: string[]): Block[] {
	const blocks: Block[] = [];
	let index = 0;

	while (index < lines.length) {
		const line = lines[index];

		if (!line.trim()) {
			index += 1;
			continue;
		}

		const fence = FENCE.exec(line.trim());
		if (fence) {
			const marker = fence[1][0].repeat(3);
			const language = fence[2] ?? "";
			const body: string[] = [];
			index += 1;
			// An unterminated fence runs to the end of the file rather than
			// swallowing the document into a parse error.
			while (index < lines.length && !lines[index].trim().startsWith(marker)) {
				body.push(lines[index]);
				index += 1;
			}
			const terminated = index < lines.length;
			if (terminated) index += 1;
			// An unterminated fence swallowed the file's trailing blank line;
			// a terminated one never had it. Same block either way.
			else while (body.length && !body[body.length - 1].trim()) body.pop();
			blocks.push({ type: "code", language: language.toLowerCase(), value: body.join("\n") });
			continue;
		}

		if (RULE.test(line)) {
			blocks.push({ type: "rule" });
			index += 1;
			continue;
		}

		const heading = HEADING.exec(line);
		if (heading) {
			const depth = heading[1].length as 1 | 2 | 3 | 4 | 5 | 6;
			const children = parseInline(heading[2]);
			blocks.push({ type: "heading", depth, slug: slugify(inlineText(children)), children });
			index += 1;
			continue;
		}

		const quote = QUOTE.exec(line);
		if (quote) {
			const body: string[] = [quote[1]];
			index += 1;
			while (index < lines.length) {
				const next = QUOTE.exec(lines[index]);
				if (next) {
					body.push(next[1]);
					index += 1;
					continue;
				}
				// Lazy continuation: a bare line under a quote stays in it.
				if (lines[index].trim() && !HEADING.test(lines[index]) && !RULE.test(lines[index])) {
					body.push(lines[index]);
					index += 1;
					continue;
				}
				break;
			}
			blocks.push({ type: "quote", blocks: parseBlocks(body) });
			continue;
		}

		if (UNORDERED.test(line) || ORDERED.test(line)) {
			const [list, next] = parseList(lines, index);
			blocks.push(list);
			index = next;
			continue;
		}

		if (
			line.includes("|")
			&& index + 1 < lines.length
			&& TABLE_DIVIDER.test(lines[index + 1])
			&& lines[index + 1].includes("-")
		) {
			const head = splitRow(line).map(parseInline);
			const align = alignments(lines[index + 1]);
			index += 2;
			const rows: InlineNode[][][] = [];
			while (index < lines.length && lines[index].trim() && lines[index].includes("|")) {
				rows.push(splitRow(lines[index]).map(parseInline));
				index += 1;
			}
			blocks.push({ type: "table", head, align, rows });
			continue;
		}

		// Paragraph, with setext heading and hard-break handling.
		const paragraph: string[] = [line];
		index += 1;
		while (index < lines.length) {
			const next = lines[index];
			if (!next.trim()) break;
			if (SETEXT.test(next) && !RULE.test(next)) {
				const depth = next.trim().startsWith("=") ? 1 : 2;
				const children = parseInline(paragraph.join(" "));
				blocks.push({ type: "heading", depth, slug: slugify(inlineText(children)), children });
				index += 1;
				paragraph.length = 0;
				break;
			}
			if (
				HEADING.test(next)
				|| FENCE.test(next.trim())
				|| RULE.test(next)
				|| QUOTE.test(next)
				|| UNORDERED.test(next)
				|| ORDERED.test(next)
			) break;
			paragraph.push(next);
			index += 1;
		}
		if (paragraph.length) blocks.push({ type: "paragraph", children: parseInline(paragraph.join("\n")) });
	}

	return blocks;
}

function indentOf(value: string): number {
	return value.replace(/\t/g, "    ").length;
}

function parseList(lines: string[], start: number): [Block, number] {
	const first = ORDERED.exec(lines[start]);
	const ordered = Boolean(first);
	const baseIndent = indentOf(ordered ? first![1] : UNORDERED.exec(lines[start])![1]);
	const items: ListItem[] = [];
	let index = start;
	let current: string[] | null = null;
	let currentChecked: boolean | null = null;

	const flush = () => {
		if (!current) return;
		items.push({ checked: currentChecked, blocks: parseBlocks(current) });
		current = null;
		currentChecked = null;
	};

	while (index < lines.length) {
		const line = lines[index];
		if (!line.trim()) {
			// A blank line inside a list is only a break when the next line
			// leaves the list; otherwise it is a loose-list separator.
			const next = lines[index + 1];
			if (!next || (!UNORDERED.test(next) && !ORDERED.test(next) && indentOf(next.match(/^\s*/)![0]) <= baseIndent)) break;
			if (current) current.push("");
			index += 1;
			continue;
		}
		const unordered = UNORDERED.exec(line);
		const numbered = ORDERED.exec(line);
		const marker = unordered ?? numbered;
		if (marker && indentOf(marker[1]) <= baseIndent) {
			flush();
			const body = unordered ? unordered[2] : numbered![3];
			const task = TASK.exec(body);
			currentChecked = task ? task[1].toLowerCase() === "x" : null;
			current = [task ? task[2] : body];
			index += 1;
			continue;
		}
		if (!current) break;
		const indent = indentOf(line.match(/^\s*/)![0]);
		if (indent <= baseIndent && !marker) {
			// Lazy continuation of the current item's paragraph.
			current.push(line.trim());
			index += 1;
			continue;
		}
		current.push(line.slice(Math.min(line.length, baseIndent + 2)));
		index += 1;
	}
	flush();

	return [
		{
			type: "list",
			ordered,
			start: ordered ? Number.parseInt(first![2], 10) : 1,
			items
		},
		index
	];
}

const AUTOLINK = /^<((?:https?|mailto):[^>\s]+)>/;
const LINK = /^\[([^\]]*)\]\(([^)\s]*)(?:\s+"[^"]*")?\)/;
const IMAGE = /^!\[([^\]]*)\]\(([^)\s]*)(?:\s+"[^"]*")?\)/;
const BARE_URL = /^https?:\/\/[^\s<>()]+[^\s<>().,;:!?]/;

/**
 * Longest marker first, so `***both***` is not read as `*` around `**both**`
 * and left with a stray asterisk.
 */
const EMPHASIS = [
	["***", "strong"],
	["**", "strong"],
	["~~", "strike"],
	["__", "strong"],
	["*", "em"],
	["_", "em"]
] as const;

/**
 * Inline parse. Emphasis is matched by scanning for the closing run rather
 * than by regex, so `a * b * c` stays arithmetic and `**bold**` does not eat
 * the rest of the paragraph when its closer is missing.
 */
export function parseInline(source: string): InlineNode[] {
	const nodes: InlineNode[] = [];
	let buffer = "";
	let index = 0;

	const pushText = () => {
		if (buffer) {
			nodes.push({ type: "text", value: buffer });
			buffer = "";
		}
	};

	while (index < source.length) {
		const rest = source.slice(index);
		const character = source[index];

		if (character === "\\" && index + 1 < source.length) {
			buffer += source[index + 1];
			index += 2;
			continue;
		}

		if (character === "`") {
			const run = /^`+/.exec(rest)![0];
			const close = source.indexOf(run, index + run.length);
			if (close !== -1) {
				pushText();
				nodes.push({ type: "code", value: source.slice(index + run.length, close).trim() });
				index = close + run.length;
				continue;
			}
		}

		const image = IMAGE.exec(rest);
		if (image) {
			pushText();
			nodes.push({ type: "image", alt: image[1], src: image[2] });
			index += image[0].length;
			continue;
		}

		const link = LINK.exec(rest);
		if (link) {
			pushText();
			nodes.push({ type: "link", href: link[2], children: parseInline(link[1]) });
			index += link[0].length;
			continue;
		}

		const autolink = AUTOLINK.exec(rest);
		if (autolink) {
			pushText();
			nodes.push({ type: "link", href: autolink[1], children: [{ type: "text", value: autolink[1] }] });
			index += autolink[0].length;
			continue;
		}

		const bare = BARE_URL.exec(rest);
		if (bare && (index === 0 || /[\s(]/.test(source[index - 1]))) {
			pushText();
			nodes.push({ type: "link", href: bare[0], children: [{ type: "text", value: bare[0] }] });
			index += bare[0].length;
			continue;
		}

		let emphasized = false;
		for (const [marker, type] of EMPHASIS) {
			if (!rest.startsWith(marker)) continue;
			const close = source.indexOf(marker, index + marker.length);
			if (close === -1) continue;
			const inner = source.slice(index + marker.length, close);
			if (!inner.trim()) continue;
			// CommonMark's flanking rule, which is what keeps `2 * 3 * 4`
			// arithmetic: an opener may not be followed by space, and a closer
			// may not be preceded by one.
			if (/\s/.test(inner[0]) || /\s/.test(inner[inner.length - 1])) continue;
			pushText();
			nodes.push({ type, children: parseInline(inner) } as InlineNode);
			index = close + marker.length;
			emphasized = true;
			break;
		}
		if (emphasized) continue;

		buffer += character;
		index += 1;
	}

	pushText();
	return nodes;
}

/** Plain text of a parsed document — the tab tooltip and the search index. */
export function markdownText(blocks: Block[]): string {
	return blocks
		.map((block) => {
			switch (block.type) {
				case "heading":
				case "paragraph":
					return inlineText(block.children);
				case "code":
					return block.value;
				case "quote":
					return markdownText(block.blocks);
				case "list":
					return block.items.map((item) => markdownText(item.blocks)).join("\n");
				case "table":
					return [block.head, ...block.rows]
						.map((row) => row.map(inlineText).join(" "))
						.join("\n");
				case "rule":
					return "";
			}
		})
		.join("\n");
}

/**
 * Headings, for the document outline. Kept beside the parser so the outline
 * and the rendered anchors cannot disagree about a slug.
 */
export function outline(blocks: Block[]): { depth: number; text: string; slug: string }[] {
	return blocks.flatMap((block) =>
		block.type === "heading"
			? [{ depth: block.depth, text: inlineText(block.children), slug: block.slug }]
			: block.type === "quote"
				? outline(block.blocks)
				: []
	);
}
