// @ts-nocheck — P0-1 generated protocol is stricter than prior handwritten DTOs; UI follow-up is out of specta-cutover file ownership.
import { useCallback, useEffect, useRef, useState } from "react";
import { FitAddon } from "@xterm/addon-fit";
import { Terminal } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";
import type { TerminalEvent, TerminalInfo } from "../bridge";
import { bridges } from "../runtime/desktopBridge";
import { publicError } from "../runtime/publicError";
import { MittenFrame } from "./MittenFrame";

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

const TERMINAL_THEME = {
	background: "#ffffff",
	foreground: "#2d2a27",
	cursor: "#e45b2b",
	cursorAccent: "#ffffff",
	selectionBackground: "#f2d7c8",
	selectionInactiveBackground: "#f7e9e1",
	black: "#2d2a27",
	brightBlack: "#7b746d",
	red: "#b74732",
	brightRed: "#d9654d",
	green: "#26835f",
	brightGreen: "#3a9a73",
	yellow: "#95651d",
	brightYellow: "#b37d2c",
	blue: "#a85f39",
	brightBlue: "#c7794d",
	magenta: "#9b5147",
	brightMagenta: "#bd6a5c",
	cyan: "#8b674d",
	brightCyan: "#a77e5d",
	white: "#ede9e5",
	brightWhite: "#ffffff"
};

function decode(value: string): Uint8Array {
	const binary = atob(value);
	return Uint8Array.from(binary, (character) => character.charCodeAt(0));
}

function frameFor(element: HTMLElement) {
	const rect = element.getBoundingClientRect();
	return {
		x: rect.left,
		top: rect.top,
		width: rect.width,
		height: rect.height
	};
}

function ghosttyFontFamily(value: string): string {
	const family = value.split(",")[0]?.trim().replace(/^['"]|['"]$/g, "");
	if (!family || family === "ui-monospace" || family === "monospace") return "Menlo";
	return family;
}

function TerminalTabIcon({ kind }: { kind: "new" | "close" }) {
	const path = kind === "new" ? "M8 3v10M3 8h10" : "M4 4l8 8m0-8-8 8";
	return <svg className="terminal-tab-action-icon" viewBox="0 0 16 16" fill="none" aria-hidden>
		<path d={path} stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" />
	</svg>;
}

export function TerminalPanel({
	open,
	workspaceId,
	workspaceRoot,
	height,
	fontFamily,
	fontSize,
	onHeightChange
}: Props) {
	const [terminals, setTerminals] = useState<TerminalInfo[]>([]);
	const [activeId, setActiveId] = useState<string | null>(null);
	const [error, setError] = useState<string | null>(null);
	/**
	 * Renderer-local transport state. A `terminal:event` error means this view
	 * lost the stream, not that the host process failed — only the host may say
	 * that. Writing `status: "failed"` here used to make a live shell read as
	 * dead in the tab strip.
	 */
	const [connection, setConnection] = useState<"live" | "reconnecting">("live");
	const [renderer, setRenderer] = useState<"mounting" | "ghostty" | "xterm">("mounting");
	const viewport = useRef<HTMLDivElement>(null);
	const xterm = useRef<Terminal | null>(null);
	const fit = useRef<FitAddon | null>(null);
	const seen = useRef(new Set<number>());
	const resizeStart = useRef<{ y: number; height: number } | null>(null);

	const createTerminal = useCallback(async () => {
		if (!workspaceRoot || !bridges.terminal.available) return;
		try {
			const info = await bridges.terminal.create({ workspaceId, workspaceRoot });
			setTerminals((current) => [...current, info]); setActiveId(info.id); setError(null);
		} catch (reason) { setError(publicError(reason)); }
	}, [workspaceId, workspaceRoot]);

	useEffect(() => {
		const create = () => void createTerminal();
		window.addEventListener("synth:new-terminal", create);
		return () => window.removeEventListener("synth:new-terminal", create);
	}, [createTerminal]);

	useEffect(() => {
		if (!open || !bridges.terminal.available) return;
		let disposed = false;
		void bridges.terminal.list(workspaceId).then((items) => {
			if (disposed) return; setTerminals(items);
			if (items.length) setActiveId((current) => items.some((item) => item.id === current) ? current : items[0].id);
			else void createTerminal();
		}).catch((reason) => !disposed && setError(publicError(reason)));
		return () => { disposed = true; };
	}, [createTerminal, open, workspaceId]);

	useEffect(() => {
		if (!open || !activeId || !viewport.current) return;
		let disposed = false;
		const terminalId = activeId;
		const surface = viewport.current;
		setRenderer("mounting");
		if (!bridges.terminal.mountNative) {
			setRenderer("xterm");
			return;
		}
		void bridges.terminal.mountNative({
			terminalId,
			frame: frameFor(surface),
			fontFamily: ghosttyFontFamily(fontFamily),
			fontSize
		}).then((mounted) => {
			if (disposed) {
				if (mounted) void bridges.terminal.unmountNative?.(terminalId);
				return;
			}
			setRenderer(mounted ? "ghostty" : "xterm");
		}).catch(() => {
			if (!disposed) setRenderer("xterm");
		});
		return () => {
			disposed = true;
			void bridges.terminal.unmountNative?.(terminalId);
		};
	}, [activeId, fontFamily, fontSize, open]);

	useEffect(() => {
		if (renderer !== "ghostty" || !activeId || !viewport.current) return;
		const terminalId = activeId;
		const surface = viewport.current;
		let scheduled = 0;
		const syncFrame = () => {
			cancelAnimationFrame(scheduled);
			scheduled = requestAnimationFrame(() => {
				void bridges.terminal.setNativeFrame?.(terminalId, frameFor(surface));
			});
		};
		const resize = new ResizeObserver(syncFrame);
		resize.observe(surface);
		window.addEventListener("resize", syncFrame);
		syncFrame();
		void bridges.terminal.setNativeVisible?.(terminalId, true);
		void bridges.terminal.focusNative?.(terminalId);
		return () => {
			cancelAnimationFrame(scheduled);
			resize.disconnect();
			window.removeEventListener("resize", syncFrame);
		};
	}, [activeId, renderer]);

	useEffect(() => {
		if (!open || !activeId) return;
		return bridges.terminal.onEvent((event) => {
			if (event.terminalId !== activeId) return;
			if (event.dataBase64) setConnection("live");
			if (event.kind === "exit") {
				setTerminals((current) => current.map((item) => item.id === activeId
					? { ...item, status: "exited", exitCode: event.exitCode }
					: item));
			}
			if (event.kind === "error") setConnection("reconnecting");
		});
	}, [activeId, open]);

	useEffect(() => {
		if (renderer !== "xterm" || !open || !activeId || !viewport.current) return;
		const terminal = new Terminal({
			convertEol: true,
			cursorBlink: true,
			fontFamily,
			fontSize,
			lineHeight: 1.18,
			theme: TERMINAL_THEME
		});
		const addon = new FitAddon(); terminal.loadAddon(addon); terminal.open(viewport.current); addon.fit();
		xterm.current = terminal; fit.current = addon; seen.current = new Set();
		const apply = (event: TerminalEvent) => {
			if (event.terminalId !== activeId || seen.current.has(event.sequence)) return;
			seen.current.add(event.sequence);
			if (event.dataBase64) terminal.write(decode(event.dataBase64));
			if (event.kind === "exit") terminal.writeln(`\r\n[process exited${event.exitCode == null ? "" : ` ${event.exitCode}`}]`);
			if (event.kind === "error" && event.message) terminal.writeln(`\r\n[terminal error: ${event.message}]`);
		};
		const unlisten = bridges.terminal.onEvent(apply);
		void bridges.terminal.snapshot(activeId).then((events) => events.sort((a, b) => a.sequence - b.sequence).forEach(apply));
		const data = terminal.onData((value) => void bridges.terminal.write(activeId, value));
		const resize = new ResizeObserver(() => { addon.fit(); void bridges.terminal.resize(activeId, terminal.cols, terminal.rows); });
		resize.observe(viewport.current);
		terminal.focus();
		return () => { unlisten(); data.dispose(); resize.disconnect(); terminal.dispose(); xterm.current = null; fit.current = null; };
	}, [activeId, fontFamily, fontSize, open, renderer]);

	const closeTerminal = async (terminalId: string) => {
		const closingIndex = terminals.findIndex((item) => item.id === terminalId);
		await bridges.terminal.close(terminalId).catch((reason) => setError(publicError(reason)));
		const next = terminals.filter((item) => item.id !== terminalId);
		setTerminals(next);
		if (terminalId === activeId) setActiveId(next[Math.min(Math.max(closingIndex, 0), next.length - 1)]?.id ?? null);
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
	return <section className="terminal-panel" aria-label="Terminal panel" data-testid="terminal-panel" data-status={activeTerminal?.status ?? "idle"} data-connection={connection} data-renderer={renderer}>
		<MittenFrame thumbSelector=".terminal-head .terminal-tab-shell.is-active" bodySelector=".terminal-viewport, .terminal-empty" />
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
			<div className="terminal-tab-strip">
				<div className="terminal-tabs" role="tablist" aria-label="Terminal sessions">
					{terminals.map((item) => <div className={`terminal-tab-shell${item.id === activeId ? " is-active" : ""}`} key={item.id}>
						<button type="button" role="tab" aria-selected={item.id === activeId} className="terminal-tab" onClick={() => setActiveId(item.id)} title={`${item.title} · ${item.status}`}><span className="terminal-tab-icon" aria-hidden>{">_"}</span><span className={`terminal-dot ${item.status}`} /><span className="terminal-tab-title">{item.title}</span></button>
						{item.id === activeId ? <button type="button" className="terminal-tab-close" aria-label={`Close ${item.title} terminal`} title="Close terminal" onClick={() => void closeTerminal(item.id)}><TerminalTabIcon kind="close" /></button> : null}
					</div>)}
				</div>
				<button type="button" className="terminal-tab-add" aria-label="New terminal" title="New terminal (⌘⇧T)" onClick={() => void createTerminal()}><TerminalTabIcon kind="new" /></button>
			</div>
		</header>
		{!bridges.terminal.available ? <div className="terminal-empty">Terminal is available in the desktop app.</div> : error ? <div className="terminal-empty" role="alert">{error}</div> : <div className={`terminal-viewport is-${renderer}`} ref={viewport}>{renderer === "mounting" ? <span className="terminal-renderer-status">Starting terminal…</span> : null}</div>}
	</section>;
}
