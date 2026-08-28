/**
 * Source → coloured tokens. No dependency, and no HTML.
 *
 * Same two reasons as the markdown parser: nothing installable can be verified
 * in this build, and every highlighter worth installing emits an HTML string,
 * which would mean `dangerouslySetInnerHTML` over file bytes. This returns a
 * flat token list the pane renders as spans.
 *
 * It is a *lexer*, not a parser, and it says so: one pass, no nesting, no
 * semantic knowledge. It will colour a keyword used as an identifier. That is
 * the honest ceiling of 200 lines, and it is the right ceiling for a reading
 * pane — the alternative is not a better highlighter, it is a worse promise.
 */

export type TokenKind =
	| "plain"
	| "comment"
	| "string"
	| "number"
	| "keyword"
	| "type"
	| "function"
	| "punctuation"
	| "meta"
	| "inserted"
	| "deleted";

export type Token = { kind: TokenKind; value: string };

type Grammar = {
	lineComment: string[];
	blockComment: [string, string][];
	strings: string[];
	/** Python/JS triple- and template-quoted forms that may span lines. */
	multilineStrings: [string, string][];
	keywords: Set<string>;
	types: Set<string>;
	/** A leading marker that colours the whole line, e.g. `#!` or a preprocessor. */
	metaLine: RegExp | null;
};

function grammar(partial: Partial<Grammar>): Grammar {
	return {
		lineComment: partial.lineComment ?? [],
		blockComment: partial.blockComment ?? [],
		strings: partial.strings ?? ['"', "'"],
		multilineStrings: partial.multilineStrings ?? [],
		keywords: partial.keywords ?? new Set(),
		types: partial.types ?? new Set(),
		metaLine: partial.metaLine ?? null
	};
}

const words = (value: string) => new Set(value.split(/\s+/).filter(Boolean));

const C_LIKE_BLOCK: [string, string][] = [["/*", "*/"]];

const GRAMMARS: Record<string, Grammar> = {
	rust: grammar({
		lineComment: ["//"],
		blockComment: C_LIKE_BLOCK,
		keywords: words(`as async await break const continue crate dyn else enum extern fn for if impl in let loop
			match mod move mut pub ref return self Self static struct super trait type unsafe use where while
			macro_rules union yield true false`),
		types: words(`bool char str String Vec Option Result Box Arc Rc RefCell HashMap BTreeMap
			u8 u16 u32 u64 u128 usize i8 i16 i32 i64 i128 isize f32 f64`),
		metaLine: /^\s*#!?\[/
	}),
	typescript: grammar({
		lineComment: ["//"],
		blockComment: C_LIKE_BLOCK,
		strings: ['"', "'", "`"],
		keywords: words(`abstract any as async await break case catch class const continue declare default delete do
			else enum export extends finally for from function get if implements import in infer instanceof interface
			keyof let new of private protected public readonly return satisfies set static super switch this throw try
			type typeof var void while yield true false null undefined`),
		types: words(`string number boolean object symbol bigint never unknown Array Promise Record Partial Readonly
			Map Set Date RegExp Error`)
	}),
	python: grammar({
		lineComment: ["#"],
		strings: ['"', "'"],
		multilineStrings: [['"""', '"""'], ["'''", "'''"]],
		keywords: words(`and as assert async await break class continue def del elif else except finally for from
			global if import in is lambda nonlocal not or pass raise return try while with yield True False None
			match case`),
		types: words(`int float str bool bytes list dict set tuple frozenset object type Exception`),
		metaLine: /^#!/
	}),
	go: grammar({
		lineComment: ["//"],
		blockComment: C_LIKE_BLOCK,
		strings: ['"', "'", "`"],
		keywords: words(`break case chan const continue default defer else fallthrough for func go goto if import
			interface map package range return select struct switch type var nil true false`),
		types: words(`bool byte complex64 complex128 error float32 float64 int int8 int16 int32 int64 rune string
			uint uint8 uint16 uint32 uint64 uintptr`)
	}),
	c: grammar({
		lineComment: ["//"],
		blockComment: C_LIKE_BLOCK,
		keywords: words(`auto break case char const continue default do double else enum extern float for goto if
			inline int long register restrict return short signed sizeof static struct switch typedef union unsigned
			void volatile while`),
		types: words(`size_t ssize_t uint8_t uint16_t uint32_t uint64_t int8_t int16_t int32_t int64_t bool FILE`),
		metaLine: /^\s*#\s*(include|define|ifdef|ifndef|endif|pragma|if|else|elif|undef)\b/
	}),
	java: grammar({
		lineComment: ["//"],
		blockComment: C_LIKE_BLOCK,
		keywords: words(`abstract assert boolean break byte case catch char class const continue default do double
			else enum extends final finally float for goto if implements import instanceof int interface long native
			new package private protected public return short static strictfp super switch synchronized this throw
			throws transient try void volatile while true false null var record sealed`),
		types: words(`String Integer Long Double Boolean Object List Map Set Optional Stream`)
	}),
	ruby: grammar({
		lineComment: ["#"],
		keywords: words(`alias and begin break case class def defined? do else elsif end ensure false for if in module
			next nil not or redo rescue retry return self super then true undef unless until when while yield`),
		types: words(`String Integer Float Array Hash Symbol Struct Module Class`)
	}),
	shell: grammar({
		lineComment: ["#"],
		strings: ['"', "'"],
		keywords: words(`if then elif else fi for while until do done case esac function in select return break
			continue local export readonly declare set unset source exit trap shift`),
		types: words(`echo printf cd ls cat grep sed awk find xargs curl git npm cargo node python python3 make`),
		metaLine: /^#!/
	}),
	sql: grammar({
		lineComment: ["--"],
		blockComment: C_LIKE_BLOCK,
		strings: ["'", '"'],
		keywords: words(`select from where group by having order limit offset insert into values update set delete
			create table view index drop alter add column primary key foreign references join left right inner outer
			on as and or not null distinct union all case when then else end with returning`),
		types: words(`integer int bigint smallint text varchar char boolean real double precision numeric decimal
			date time timestamp json jsonb blob uuid`)
	}),
	css: grammar({
		blockComment: C_LIKE_BLOCK,
		strings: ['"', "'"],
		keywords: words(`important media supports keyframes import charset font-face root var calc`),
		types: new Set()
	}),
	json: grammar({ strings: ['"'], keywords: words("true false null") }),
	yaml: grammar({ lineComment: ["#"], strings: ['"', "'"], keywords: words("true false null yes no on off") }),
	toml: grammar({ lineComment: ["#"], strings: ['"', "'"], keywords: words("true false") }),
	ini: grammar({ lineComment: ["#", ";"], strings: ['"', "'"], keywords: words("true false") }),
	html: grammar({ blockComment: [["<!--", "-->"]], strings: ['"', "'"] }),
	hcl: grammar({
		lineComment: ["#", "//"],
		blockComment: C_LIKE_BLOCK,
		keywords: words("resource variable module output provider data locals terraform true false null for in if")
	}),
	docker: grammar({
		lineComment: ["#"],
		keywords: words(`FROM RUN CMD LABEL EXPOSE ENV ADD COPY ENTRYPOINT VOLUME USER WORKDIR ARG ONBUILD
			STOPSIGNAL HEALTHCHECK SHELL AS`)
	}),
	make: grammar({ lineComment: ["#"], keywords: words(".PHONY include ifeq ifneq endif else define endef export") })
};

const ALIASES: Record<string, string> = {
	tsx: "typescript",
	jsx: "typescript",
	javascript: "typescript",
	js: "typescript",
	ts: "typescript",
	mjs: "typescript",
	cjs: "typescript",
	py: "python",
	rs: "rust",
	sh: "shell",
	bash: "shell",
	zsh: "shell",
	console: "shell",
	cpp: "c",
	"c++": "c",
	h: "c",
	hpp: "c",
	kotlin: "java",
	swift: "java",
	yml: "yaml",
	svg: "html",
	xml: "html",
	terraform: "hcl",
	tf: "hcl",
	dockerfile: "docker",
	makefile: "make"
};

/** Whether this build can colour a language, for the badge's benefit. */
export function isHighlightable(language: string): boolean {
	const key = language.toLowerCase();
	return Boolean(GRAMMARS[ALIASES[key] ?? key]);
}

const IDENTIFIER = /^[A-Za-z_$][\w$]*/;
const NUMBER = /^(?:0[xXbBoO][0-9a-fA-F_]+|\d[\d_]*(?:\.[\d_]+)?(?:[eE][+-]?\d+)?)[a-zA-Z_]*/;
const PUNCTUATION = /^[{}()[\].,;:!?<>=+\-*/%&|^~@#$]/;

/**
 * Line-oriented, because a diff's meaning is the line prefix and nothing else.
 * Running the generic lexer over a patch would colour `-` as an operator and
 * lose the only signal the reader wants.
 */
function highlightDiff(source: string): Token[] {
	return source.split("\n").flatMap((line, index) => {
		const kind: TokenKind = line.startsWith("+++") || line.startsWith("---")
			? "meta"
			: line.startsWith("@@")
				? "meta"
				: line.startsWith("+")
					? "inserted"
					: line.startsWith("-")
						? "deleted"
						: "plain";
		const token: Token = { kind, value: line };
		return index === 0 ? [token] : [{ kind: "plain" as TokenKind, value: "\n" }, token];
	});
}

/**
 * Tokenize `source` for display.
 *
 * An unknown language is not an error and not a blank pane: it returns one
 * plain token, which renders as monospaced, selectable, uncoloured text.
 */
export function highlight(source: string, language: string): Token[] {
	const key = language.toLowerCase();
	if (key === "diff" || key === "patch") return highlightDiff(source);
	const spec = GRAMMARS[ALIASES[key] ?? key];
	if (!spec) return source ? [{ kind: "plain", value: source }] : [];

	const tokens: Token[] = [];
	let plain = "";
	let index = 0;
	const push = (kind: TokenKind, value: string) => {
		if (plain) {
			tokens.push({ kind: "plain", value: plain });
			plain = "";
		}
		tokens.push({ kind, value });
	};

	while (index < source.length) {
		const rest = source.slice(index);
		const atLineStart = index === 0 || source[index - 1] === "\n";

		if (atLineStart && spec.metaLine?.test(rest.slice(0, rest.indexOf("\n") + 1 || undefined))) {
			const end = rest.indexOf("\n");
			const line = end === -1 ? rest : rest.slice(0, end);
			push("meta", line);
			index += line.length;
			continue;
		}

		const lineComment = spec.lineComment.find((marker) => rest.startsWith(marker));
		if (lineComment) {
			const end = rest.indexOf("\n");
			const value = end === -1 ? rest : rest.slice(0, end);
			push("comment", value);
			index += value.length;
			continue;
		}

		const block = spec.blockComment.find(([open]) => rest.startsWith(open));
		if (block) {
			const close = source.indexOf(block[1], index + block[0].length);
			const end = close === -1 ? source.length : close + block[1].length;
			push("comment", source.slice(index, end));
			index = end;
			continue;
		}

		const multiline = spec.multilineStrings.find(([open]) => rest.startsWith(open));
		if (multiline) {
			const close = source.indexOf(multiline[1], index + multiline[0].length);
			const end = close === -1 ? source.length : close + multiline[1].length;
			push("string", source.slice(index, end));
			index = end;
			continue;
		}

		const quote = spec.strings.find((mark) => rest.startsWith(mark));
		if (quote) {
			let cursor = index + quote.length;
			while (cursor < source.length) {
				if (source[cursor] === "\\") {
					cursor += 2;
					continue;
				}
				if (source.startsWith(quote, cursor)) {
					cursor += quote.length;
					break;
				}
				// An unterminated string ends at the line, not at the file:
				// one stray quote must not paint the rest of the document.
				if (source[cursor] === "\n" && quote !== "`") break;
				cursor += 1;
			}
			push("string", source.slice(index, cursor));
			index = cursor;
			continue;
		}

		const number = NUMBER.exec(rest);
		if (number && !/[\w$]/.test(source[index - 1] ?? "")) {
			push("number", number[0]);
			index += number[0].length;
			continue;
		}

		const identifier = IDENTIFIER.exec(rest);
		if (identifier) {
			const value = identifier[0];
			const after = rest.slice(value.length).match(/^\s*/)![0].length;
			const next = rest[value.length + after];
			const kind: TokenKind = spec.keywords.has(value)
				? "keyword"
				: spec.types.has(value)
					? "type"
					: next === "("
						? "function"
						: /^[A-Z]/.test(value) && spec.types.size > 0
							? "type"
							: "plain";
			if (kind === "plain") plain += value;
			else push(kind, value);
			index += value.length;
			continue;
		}

		if (PUNCTUATION.test(rest)) {
			push("punctuation", rest[0]);
			index += 1;
			continue;
		}

		plain += source[index];
		index += 1;
	}

	if (plain) tokens.push({ kind: "plain", value: plain });
	return tokens;
}
