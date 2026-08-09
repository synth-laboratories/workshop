import { useCallback, useEffect, useRef, useState } from "react";
import { FitAddon } from "@xterm/addon-fit";
import { Terminal } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";
import type { TerminalEvent, TerminalInfo } from "../env";

type Props = { open: boolean; workspaceId: string; workspaceRoot: string | null; onOpenChange(open: boolean): void };

function decode(value: string): Uint8Array {
	const binary = atob(value);
	return Uint8Array.from(binary, (character) => character.charCodeAt(0));
}

export function TerminalPanel({ open, workspaceId, workspaceRoot, onOpenChange }: Props) {
	const [terminals, setTerminals] = useState<TerminalInfo[]>([]);
	const [activeId, setActiveId] = useState<string | null>(null);
	const [error, setError] = useState<string | null>(null);
	const viewport = useRef<HTMLDivElement>(null);
	const xterm = useRef<Terminal | null>(null);
	const fit = useRef<FitAddon | null>(null);
	const seen = useRef(new Set<number>());

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
		const terminal = new Terminal({ convertEol: true, cursorBlink: true, fontFamily: '"SFMono-Regular", Menlo, monospace', fontSize: 12, theme: { background: "#fbfbfc", foreground: "#242425", cursor: "#f05f22", selectionBackground: "#d9def0" } });
		const addon = new FitAddon(); terminal.loadAddon(addon); terminal.open(viewport.current); addon.fit();
		xterm.current = terminal; fit.current = addon; seen.current = new Set();
		const apply = (event: TerminalEvent) => {
			if (event.terminalId !== activeId || seen.current.has(event.sequence)) return;
			seen.current.add(event.sequence);
			if (event.dataBase64) terminal.write(decode(event.dataBase64));
			if (event.kind === "exit") terminal.writeln(`\r\n[process exited${event.exitCode == null ? "" : ` ${event.exitCode}`}]`);
			if (event.kind === "error" && event.message) terminal.writeln(`\r\n[terminal error: ${event.message}]`);
		};
		const unlisten = window.synthTerminal.onEvent(apply);
		void window.synthTerminal.snapshot(activeId).then((events) => events.sort((a, b) => a.sequence - b.sequence).forEach(apply));
		const data = terminal.onData((value) => void window.synthTerminal.write(activeId, value));
		const resize = new ResizeObserver(() => { addon.fit(); void window.synthTerminal.resize(activeId, terminal.cols, terminal.rows); });
		resize.observe(viewport.current);
		terminal.focus();
		return () => { unlisten(); data.dispose(); resize.disconnect(); terminal.dispose(); xterm.current = null; fit.current = null; };
	}, [activeId, open]);

	const closeActive = async () => {
		if (!activeId) return;
		await window.synthTerminal.close(activeId).catch((reason) => setError(String(reason)));
		setTerminals((current) => { const next = current.filter((item) => item.id !== activeId); setActiveId(next[0]?.id ?? null); return next; });
	};

	if (!open) return null;
	return <section className="terminal-panel" aria-label="Terminal panel" data-testid="terminal-panel">
		<header className="terminal-head">
			<div className="terminal-tabs" role="tablist" aria-label="Terminal sessions">
				{terminals.map((item) => <button type="button" role="tab" aria-selected={item.id === activeId} className={`terminal-tab${item.id === activeId ? " is-active" : ""}`} key={item.id} onClick={() => setActiveId(item.id)}><span className={`terminal-dot ${item.status}`} />{item.title}</button>)}
			</div>
			<button type="button" className="terminal-action" aria-label="New terminal" title="New terminal (⌘⇧T)" onClick={() => void createTerminal()}>+</button>
			<button type="button" className="terminal-action" aria-label="Close terminal" onClick={() => void closeActive()}>×</button>
			<button type="button" className="terminal-action" aria-label="Hide terminal" onClick={() => onOpenChange(false)}>⌄</button>
		</header>
		{!window.synthTerminal.available ? <div className="terminal-empty">Terminal is available in the desktop app.</div> : error ? <div className="terminal-empty" role="alert">{error}</div> : <div className="terminal-viewport" ref={viewport} />}
	</section>;
}
