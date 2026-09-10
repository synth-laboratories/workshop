/**
 * The rendered body of one workspace document.
 *
 * Markdown is typeset by default with a View source toggle; every other text
 * file is a single highlighted code block. Nothing here is set as HTML: the
 * markdown tree and the token list both render as React elements, so a
 * `<script>` in a README is a paragraph containing the characters
 * `<script>` — structurally, not because a sanitizer caught it.
 */

import { useMemo, useState, type ReactNode } from "react";
import { openPath } from "@tauri-apps/plugin-opener";

// The `.document-*` rules this module's output depends on. Imported here rather
// than only from `DocumentPane` so that `Markdown` carries its own typography
// wherever it is used — `ReportsPage` renders report prose through it and never
// mounts the pane.
import "./DocumentPane.css";

import { formatBytes, type WorkspaceDocument } from "./bridge.ts";
import { highlight, isHighlightable, type Token } from "./highlight.ts";
import { outline, parseMarkdown, type Block, type InlineNode } from "./markdown.ts";

/** How a link inside a document is followed. */
export type DocumentLinkHandler = (path: string) => void;

function CopyButton({ value, label = "Copy" }: { value: string; label?: string }) {
	const [copied, setCopied] = useState(false);
	return (
		<button
			type="button"
			className="document-copy"
			// State is announced, not only coloured: the label itself changes.
			onClick={() => {
				void navigator.clipboard?.writeText(value).then(
					() => {
						setCopied(true);
						window.setTimeout(() => setCopied(false), 1_400);
					},
					() => setCopied(false)
				);
			}}
		>
			{copied ? "Copied" : label}
		</button>
	);
}

function TokenSpans({ tokens }: { tokens: Token[] }) {
	return (
		<>
			{tokens.map((token, index) =>
				token.kind === "plain"
					? token.value
					: (
						<span key={index} className={`document-token document-token-${token.kind}`}>
							{token.value}
						</span>
					)
			)}
		</>
	);
}

export function CodeBlock({ value, language }: { value: string; language: string }) {
	const tokens = useMemo(() => highlight(value, language), [value, language]);
	const known = isHighlightable(language);
	const lines = value.split("\n").length;
	return (
		<figure className="document-code" data-language={language || "text"}>
			<figcaption className="document-code-head">
				{/* The badge tells the truth about this build's coverage: a
				    language it cannot colour says so rather than displaying a
				    badge that implies highlighting the reader is not getting. */}
				<span className="document-code-language">
					{language ? language : "text"}
					{language && !known ? <span className="document-code-plain"> · not highlighted</span> : null}
				</span>
				<span className="document-code-lines">{lines === 1 ? "1 line" : `${lines} lines`}</span>
				<CopyButton value={value} />
			</figcaption>
			<pre className="document-code-body">
				<code>
					<TokenSpans tokens={tokens} />
				</code>
			</pre>
		</figure>
	);
}

function isExternal(href: string): boolean {
	return /^[a-z][a-z0-9+.-]*:/i.test(href);
}

function Inline({ nodes, onOpenLink }: { nodes: InlineNode[]; onOpenLink?: DocumentLinkHandler }) {
	return (
		<>
			{nodes.map((node, index): ReactNode => {
				switch (node.type) {
					case "text":
						return node.value;
					case "code":
						return <code key={index} className="document-inline-code">{node.value}</code>;
					case "strong":
						return <strong key={index}><Inline nodes={node.children} onOpenLink={onOpenLink} /></strong>;
					case "em":
						return <em key={index}><Inline nodes={node.children} onOpenLink={onOpenLink} /></em>;
					case "strike":
						return <s key={index}><Inline nodes={node.children} onOpenLink={onOpenLink} /></s>;
					case "image":
						// Deliberately not an <img>. A document may reference a
						// remote image, and a pane that fetched it would make
						// the reader's machine talk to whoever wrote the file.
						return (
							<span key={index} className="document-image-ref" title={node.src}>
								🖼 {node.alt || node.src}
							</span>
						);
					case "link": {
						const external = isExternal(node.href);
						const anchor = node.href.startsWith("#");
						return (
							<a
								key={index}
								className="document-link"
								href={anchor ? node.href : undefined}
								title={node.href}
								onClick={(event) => {
									if (anchor) return;
									event.preventDefault();
									if (external) void openPath(node.href);
									else onOpenLink?.(node.href);
								}}
							>
								<Inline nodes={node.children} onOpenLink={onOpenLink} />
							</a>
						);
					}
				}
			})}
		</>
	);
}

function Blocks({ blocks, onOpenLink }: { blocks: Block[]; onOpenLink?: DocumentLinkHandler }) {
	return (
		<>
			{blocks.map((block, index): ReactNode => {
				switch (block.type) {
					case "heading": {
						const Tag = `h${block.depth}` as "h1";
						return (
							<Tag key={index} id={block.slug} className="document-heading">
								<Inline nodes={block.children} onOpenLink={onOpenLink} />
							</Tag>
						);
					}
					case "paragraph":
						return <p key={index}><Inline nodes={block.children} onOpenLink={onOpenLink} /></p>;
					case "code":
						return <CodeBlock key={index} value={block.value} language={block.language} />;
					case "quote":
						return (
							<blockquote key={index} className="document-quote">
								<Blocks blocks={block.blocks} onOpenLink={onOpenLink} />
							</blockquote>
						);
					case "rule":
						return <hr key={index} className="document-rule" />;
					case "list": {
						const items = block.items.map((item, itemIndex) => (
							<li key={itemIndex} className={item.checked === null ? undefined : "document-task"}>
								{item.checked === null ? null : (
									<input
										type="checkbox"
										checked={item.checked}
										readOnly
										// A rendered document is a projection. A
										// checkbox the reader could tick would be
										// the pane inventing durable state the
										// file does not know about.
										aria-label={item.checked ? "Done" : "Not done"}
									/>
								)}
								<Blocks blocks={item.blocks} onOpenLink={onOpenLink} />
							</li>
						));
						return block.ordered
							? <ol key={index} start={block.start} className="document-list">{items}</ol>
							: <ul key={index} className="document-list">{items}</ul>;
					}
					case "table":
						return (
							<div key={index} className="document-table-scroll">
								<table className="document-table">
									<thead>
										<tr>
											{block.head.map((cell, cellIndex) => (
												<th key={cellIndex} style={{ textAlign: block.align[cellIndex] ?? undefined }}>
													<Inline nodes={cell} onOpenLink={onOpenLink} />
												</th>
											))}
										</tr>
									</thead>
									<tbody>
										{block.rows.map((row, rowIndex) => (
											<tr key={rowIndex}>
												{row.map((cell, cellIndex) => (
													<td key={cellIndex} style={{ textAlign: block.align[cellIndex] ?? undefined }}>
														<Inline nodes={cell} onOpenLink={onOpenLink} />
													</td>
												))}
											</tr>
										))}
									</tbody>
								</table>
							</div>
						);
				}
			})}
		</>
	);
}

/** Markdown, typeset. Exported so reports can render `report.prose.v1` blocks
 *  through the same renderer instead of growing a second one. */
export function Markdown({ source, onOpenLink }: { source: string; onOpenLink?: DocumentLinkHandler }) {
	const blocks = useMemo(() => parseMarkdown(source), [source]);
	return <div className="document-prose"><Blocks blocks={blocks} onOpenLink={onOpenLink} /></div>;
}

export function DocumentOutline({ source, onJump }: { source: string; onJump: (slug: string) => void }) {
	const headings = useMemo(() => outline(parseMarkdown(source)), [source]);
	if (headings.length < 3) return null;
	return (
		<nav className="document-outline" aria-label="Document outline">
			{headings.map((heading) => (
				<button
					key={heading.slug}
					type="button"
					className="document-outline-item"
					data-depth={heading.depth}
					onClick={() => onJump(heading.slug)}
				>
					{heading.text}
				</button>
			))}
		</nav>
	);
}

/**
 * One document's body, with the View source toggle for markdown.
 *
 * `truncated` is stated in the body rather than in a toast: the reader is
 * looking at the text, and the fact that the text stops early is a property of
 * what they are reading.
 */
export function DocumentContent({
	document,
	onOpenLink
}: {
	document: WorkspaceDocument;
	onOpenLink?: DocumentLinkHandler;
}) {
	const [source, setSource] = useState(false);
	const isMarkdown = document.kind === "markdown";
	return (
		<div className="document-content">
			{isMarkdown ? (
				<div className="document-content-controls">
					<button
						type="button"
						className="document-toggle"
						aria-pressed={source}
						onClick={() => setSource((current) => !current)}
					>
						{source ? "Typeset" : "View source"}
					</button>
					<CopyButton value={document.text} label="Copy file" />
				</div>
			) : null}
			{document.truncated ? (
				<p className="document-truncated" role="status">
					Showing the first {formatBytes(new Blob([document.text]).size)} of {formatBytes(document.byteSize)}.
					Open it externally to read the rest.
				</p>
			) : null}
			{isMarkdown && !source ? (
				<Markdown source={document.text} onOpenLink={onOpenLink} />
			) : (
				<CodeBlock value={document.text} language={isMarkdown ? "markdown" : document.language} />
			)}
		</div>
	);
}
