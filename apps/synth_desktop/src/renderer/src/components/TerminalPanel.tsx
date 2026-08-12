import { useCallback, useEffect, useRef, useState } from "react";
import { FitAddon } from "@xterm/addon-fit";
import { Terminal } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";
import type { TerminalEvent, TerminalInfo } from "../bridge";

type Props = {
	open: boolean;
	workspaceId: string;
	workspaceRoot: string | null;
	height: number;
	fontFamily: string;
	fontSize: number;
	onOpenChange(open: boolean): void;
	onHeightChange(height: number): void;
};

function decode(value: string): Uint8Array {
	const binary = atob(value);
	return Uint8Array.from(binary, (character) => character.charCodeAt(0));
}

export function TerminalPanel({
	open,
	workspaceId,
	workspaceRoot,
	height,
	fontFamily,
	fontSize,
	onOpenChange,
	onHeightChange
}: Props) {
	const [terminals, setTerminals] = useState<TerminalInfo[]>([]);
	const [activeId, setActiveId] = useState<string | null>(null);
	const [error, setError] = useState<string | null>(null);
	const viewport = useRef<HTMLDivElement>(null);
	const xterm = useRef<Terminal | null>(null);
	const fit = useRef<FitAddon | null>(null);
	const seen = useRef(new Set<number>());
	const resizeStart = useRef<{ y: number; height: number } | null>(null);

	const createTerminal = useCallback(async () => {
		if (!workspaceRoot || !window.synthTerminal.available) return;
		try {
			const info = await window.synthTerminal.create({ workspaceId, workspaceRoot });
			setTerminals((current) => [...current, info]); setActiveId(info.id); setError(null);
		} catch (reason) { setError(reason instanceof Error ? reason.message : String(reason)); }
	}, [workspaceId, workspaceRoot]);

	useEffect(() => {
		const create = () => void createTerminal();
		window.addEventListener("synth:new-terminal", create);
		return () => window.removeEventListener("synth:new-terminal", create);
	}, [createTerminal]);

	useEffect(() => {
		if (!open || !window.synthTerminal.available) return;
		let disposed = false;
		void window.synthTerminal.list(workspaceId).then((items) => {
			if (disposed) return; setTerminals(items);
			if (items.length) setActiveId((current) => items.some((item) => item.id === current) ? current : items[0].id);
			else void createTerminal();
		}).catch((reason) => !disposed && setError(String(reason)));
		return () => { disposed = true; };
	}, [createTerminal, open, workspaceId]);

	useEffect(() => {
		if (!open || !activeId || !viewport.current) return;
		const terminal = new Terminal({
			convertEol: true,
			cursorBlink: true,
			fontFamily,
			fontSize,
			lineHeight: 1.18,
			theme: {
				background: "#111318",
				foreground: "#dce2ea",
				cursor: "#ff8656",
				cursorAccent: "#111318",
				selectionBackground: "#33455dcc",
				selectionInactiveBackground: "#2a354480",
				black: "#252a33",
				brightBlack: "#667080",
				red: "#ef7d8e",
				brightRed: "#ff9aa8",
				green: "#78d19c",
				brightGreen: "#9ae7b5",
				yellow: "#e5bd78",
				brightYellow: "#f5d899",
				blue: "#82a9ff",
				brightBlue: "#acc8ff",
				magenta: "#c39bff",
				brightMagenta: "#d9bdff",
				cyan: "#70ced5",
				brightCyan: "#9ce5eb",
				white: "#dce2ea",
				brightWhite: "#ffffff"
			}
		});
		const addon = new FitAddon(); terminal.loadAddon(addon); terminal.open(viewport.current); addon.fit();
		xterm.current = terminal; fit.current = addon; seen.current = new Set();
		const apply = (event: TerminalEvent) => {
			if (event.terminalId !== activeId || seen.current.has(event.sequence)) return;
			seen.current.add(event.sequence);
			if (event.dataBase64) terminal.write(decode(event.dataBase64));
			if (event.kind === "exit") {
				setTerminals((current) => current.map((item) => item.id === activeId
					? { ...item, status: "exited", exitCode: event.exitCode }
					: item));
				terminal.writeln(`\r\n[process exited${event.exitCode == null ? "" : ` ${event.exitCode}`}]`);
			}
			if (event.kind === "error") {
				setTerminals((current) => current.map((item) => item.id === activeId ? { ...item, status: "failed" } : item));
				if (event.message) terminal.writeln(`\r\n[terminal error: ${event.message}]`);
			}
		};
		const unlisten = window.synthTerminal.onEvent(apply);
		void window.synthTerminal.snapshot(activeId).then((events) => events.sort((a, b) => a.sequence - b.sequence).forEach(apply));
		const data = terminal.onData((value) => void window.synthTerminal.write(activeId, value));
		const resize = new ResizeObserver(() => { addon.fit(); void window.synthTerminal.resize(activeId, terminal.cols, terminal.rows); });
		resize.observe(viewport.current);
		terminal.focus();
		return () => { unlisten(); data.dispose(); resize.disconnect(); terminal.dispose(); xterm.current = null; fit.current = null; };
	}, [activeId, fontFamily, fontSize, open]);

	const closeActive = async () => {
		if (!activeId) return;
		await window.synthTerminal.close(activeId).catch((reason) => setError(String(reason)));
		setTerminals((current) => { const next = current.filter((item) => item.id !== activeId); setActiveId(next[0]?.id ?? null); return next; });
	};

	const clampHeight = (next: number) => Math.round(Math.min(
		Math.max(120, Math.min(480, window.innerHeight - 280)),
		Math.max(120, next)
	));

	const resizeTerminal = (clientY: number) => {
		if (!resizeStart.current) return;
		onHeightChange(clampHeight(resizeStart.current.height + resizeStart.current.y - clientY));
	};

	const activeTerminal = terminals.find((item) => item.id === activeId) ?? null;
	const runningCount = terminals.filter((item) => item.status === "running").length;

	if (!open) return null;
	return <section className="terminal-panel" aria-label="Terminal panel" data-testid="terminal-panel" data-status={activeTerminal?.status ?? "idle"}>
		<div
			className="terminal-resize-handle"
			role="separator"
			aria-label="Resize terminal"
			aria-orientation="horizontal"
			aria-valuemin={120}
			aria-valuemax={Math.max(120, Math.min(480, window.innerHeight - 280))}
			aria-valuenow={height}
			tabIndex={0}
			data-testid="terminal-resize-handle"
			onPointerDown={(event) => {
				event.preventDefault();
				event.currentTarget.focus();
				resizeStart.current = { y: event.clientY, height };
				event.currentTarget.setPointerCapture(event.pointerId);
			}}
			onPointerMove={(event) => {
				if (event.currentTarget.hasPointerCapture(event.pointerId)) resizeTerminal(event.clientY);
			}}
			onPointerUp={(event) => {
				if (event.currentTarget.hasPointerCapture(event.pointerId)) event.currentTarget.releasePointerCapture(event.pointerId);
				resizeStart.current = null;
			}}
			onPointerCancel={() => { resizeStart.current = null; }}
			onKeyDown={(event) => {
				if (event.key !== "ArrowUp" && event.key !== "ArrowDown") return;
				event.preventDefault();
				onHeightChange(clampHeight(height + (event.key === "ArrowUp" ? 1 : -1) * (event.shiftKey ? 64 : 24)));
			}}
		/>
		<header className="terminal-head">
			<div className="terminal-identity" aria-label={`${runningCount} running terminal${runningCount === 1 ? "" : "s"}`}>
				<span className="terminal-glyph" aria-hidden>{">_"}</span>
				<span className="terminal-label">Terminal</span>
				<span className="terminal-session-count">{runningCount || terminals.length}</span>
			</div>
			<div className="terminal-tabs" role="tablist" aria-label="Terminal sessions">
				{terminals.map((item) => <button type="button" role="tab" aria-selected={item.id === activeId} className={`terminal-tab${item.id === activeId ? " is-active" : ""}`} key={item.id} onClick={() => setActiveId(item.id)} title={`${item.title} · ${item.status}`}><span className={`terminal-dot ${item.status}`} /><span className="terminal-tab-title">{item.title}</span></button>)}
			</div>
			<div className="terminal-actions" aria-label="Terminal controls">
				<button type="button" className="terminal-action" aria-label="New terminal" title="New terminal (⌘⇧T)" onClick={() => void createTerminal()}><span aria-hidden>+</span></button>
				<button type="button" className="terminal-action terminal-close-action" aria-label="Close terminal" title="Close active terminal" disabled={!activeId} onClick={() => void closeActive()}><span aria-hidden>×</span></button>
				<button type="button" className="terminal-action terminal-hide-action" aria-label="Hide terminal" title="Hide terminal (⌘J)" onClick={() => onOpenChange(false)}><span aria-hidden>⌄</span></button>
			</div>
		</header>
		{!window.synthTerminal.available ? <div className="terminal-empty">Terminal is available in the desktop app.</div> : error ? <div className="terminal-empty" role="alert">{error}</div> : <div className="terminal-viewport" ref={viewport} />}
	</section>;
}
