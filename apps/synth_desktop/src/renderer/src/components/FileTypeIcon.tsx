/** Poolside-style file-type glyphs for transcript activity. */

export function fileExt(path: string): string {
	const base = path.split(/[/\\]/).pop() ?? path;
	const i = base.lastIndexOf(".");
	if (i <= 0) return "";
	return base.slice(i + 1).toLowerCase();
}

export function FileTypeIcon({ path, className = "file-type-icon" }: { path: string; className?: string }) {
	const ext = fileExt(path);

	if (ext === "md" || ext === "mdx") {
		return (
			<span className={`${className} file-type-md`} title="Markdown" aria-hidden>
				<span className="file-type-md-m">M</span>
				<span className="file-type-md-arrow">↓</span>
			</span>
		);
	}

	if (ext === "rs") {
		return (
			<span className={`${className} file-type-rs`} title="Rust" aria-hidden>
				<svg width="14" height="14" viewBox="0 0 16 16" fill="none">
					<circle cx="8" cy="8" r="6.2" stroke="#dea584" strokeWidth="1.4" />
					<circle cx="8" cy="8" r="2.2" fill="#dea584" />
					<path
						d="M8 1.8v1.6M8 12.6v1.6M1.8 8h1.6M12.6 8h1.6M3.4 3.4l1.1 1.1M11.5 11.5l1.1 1.1M12.6 3.4l-1.1 1.1M4.5 11.5l-1.1 1.1"
						stroke="#dea584"
						strokeWidth="1.2"
						strokeLinecap="round"
					/>
				</svg>
			</span>
		);
	}

	if (ext === "ts" || ext === "tsx") {
		return (
			<span className={`${className} file-type-ts`} title="TypeScript" aria-hidden>
				TS
			</span>
		);
	}

	if (ext === "js" || ext === "jsx" || ext === "mjs") {
		return (
			<span className={`${className} file-type-js`} title="JavaScript" aria-hidden>
				JS
			</span>
		);
	}

	if (ext === "py") {
		return (
			<span className={`${className} file-type-py`} title="Python" aria-hidden>
				Py
			</span>
		);
	}

	if (ext === "toml" || ext === "json" || ext === "yaml" || ext === "yml") {
		return (
			<span className={`${className} file-type-cfg`} title={ext} aria-hidden>
				{ext === "toml" ? "T" : ext.slice(0, 1).toUpperCase()}
			</span>
		);
	}

	if (ext === "tsx" || ext === "css" || ext === "html") {
		return (
			<span className={`${className} file-type-web`} title={ext} aria-hidden>
				{ext.slice(0, 2).toUpperCase()}
			</span>
		);
	}

	return (
		<span className={`${className} file-type-generic`} title={ext || "file"} aria-hidden>
			<svg width="14" height="14" viewBox="0 0 16 16" fill="none">
				<path
					d="M4.5 2.5h5.2L12.5 5.3V13a.8.8 0 01-.8.8H4.5a.8.8 0 01-.8-.8V3.3a.8.8 0 01.8-.8z"
					stroke="currentColor"
					strokeWidth="1.25"
					strokeLinejoin="round"
				/>
				<path d="M9.5 2.5V5.5H12.3" stroke="currentColor" strokeWidth="1.25" strokeLinejoin="round" />
			</svg>
		</span>
	);
}

export function shortenPath(path: string, max = 56): string {
	if (path.length <= max) return path;
	const parts = path.split("/");
	if (parts.length < 3) return `…${path.slice(-(max - 1))}`;
	return `…/${parts.slice(-3).join("/")}`;
}
