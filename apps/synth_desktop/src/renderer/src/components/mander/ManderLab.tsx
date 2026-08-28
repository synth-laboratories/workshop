import { useEffect, useMemo, useState } from "react";
import { Mander } from "./Mander";
import { transitionKey } from "./Mander.transitions";
import { MANDER_STATES, type ManderMotion, type ManderState, type TransitionKey } from "./Mander.types";

const SIZES = [16, 24, 32, 64, 128, 256] as const;
const MOTIONS: ManderMotion[] = ["auto", "full", "reduced", "still"];

const STATE_LABEL: Record<ManderState, string> = {
	idle: "Idle",
	thinking: "Thinking",
	working: "Working",
	success: "Success"
};

const MASCOT_LABEL: Record<ManderState, string> = {
	idle: "Synth is idle",
	thinking: "Synth is thinking",
	working: "Synth is working",
	success: "Synth succeeded"
};

const MATRIX: Array<{ key: TransitionKey; from: ManderState; to: ManderState; label: string }> = MANDER_STATES.flatMap(
	(from) =>
		MANDER_STATES.map((to) => ({
			key: `${from}->${to}` as TransitionKey,
			from,
			to,
			label: from === to ? `${STATE_LABEL[from]} loop` : `${STATE_LABEL[from]} → ${to}`
		}))
);

function nextState(current: ManderState): ManderState {
	return MANDER_STATES[(MANDER_STATES.indexOf(current) + 1) % MANDER_STATES.length]!;
}

function randomChaosDelay(): number {
	return 100 + Math.floor(Math.random() * 251);
}

export function ManderLab() {
	const [state, setState] = useState<ManderState>("idle");
	const [motion, setMotion] = useState<ManderMotion>("full");
	const [size, setSize] = useState<(typeof SIZES)[number]>(128);
	const [theme, setTheme] = useState<"light" | "dark">("light");
	const [rapid, setRapid] = useState(false);
	const [chaos, setChaos] = useState(false);

	useEffect(() => {
		if (!rapid) return;
		const id = window.setInterval(() => {
			setState(nextState);
		}, 80);
		return () => window.clearInterval(id);
	}, [rapid]);

	useEffect(() => {
		if (!chaos) return;
		let timer = 0;
		const tick = () => {
			setState(nextState);
			timer = window.setTimeout(tick, randomChaosDelay());
		};
		timer = window.setTimeout(tick, randomChaosDelay());
		return () => window.clearTimeout(timer);
	}, [chaos]);

	const matrixActive = useMemo(() => transitionKey(state, state), [state]);

	return (
		<div className={`mander-lab mander-lab-${theme}`} data-testid="mander-lab" data-theme={theme}>
			<header className="mander-lab-head">
				<div>
					<p className="ws-eyebrow">Development fixture</p>
					<h1 className="ws-title">Mander Lab</h1>
					<p className="ws-lede">Four-state motion matrix. Not a product surface.</p>
				</div>
				<p className="mander-lab-state" data-testid="mander-lab-state">
					State: {state}
				</p>
			</header>

			<div className="mander-lab-stage" data-testid="mander-lab-stage">
				<Mander
					state={state}
					size={size}
					motion={motion}
					label={MASCOT_LABEL[state]}
				/>
			</div>

			<section className="mander-lab-controls" aria-label="Mander lab controls">
				<div className="ws-btn-row">
					{MANDER_STATES.map((value) => (
						<button
							key={value}
							type="button"
							className={`ws-btn ws-btn-small${state === value ? " ws-btn-primary" : " ws-btn-secondary"}`}
							onClick={() => setState(value)}
						>
							{STATE_LABEL[value]}
						</button>
					))}
				</div>

				<div className="seg-control" role="group" aria-label="Motion">
					{MOTIONS.map((value) => (
						<button
							key={value}
							type="button"
							className={motion === value ? "active" : undefined}
							aria-pressed={motion === value}
							onClick={() => setMotion(value)}
						>
							{value}
						</button>
					))}
				</div>

				<div className="seg-control" role="group" aria-label="Size">
					{SIZES.map((value) => (
						<button
							key={value}
							type="button"
							className={size === value ? "active" : undefined}
							aria-pressed={size === value}
							onClick={() => setSize(value)}
							data-testid={`mander-size-${value}`}
						>
							{value}
						</button>
					))}
				</div>

				<div className="seg-control" role="group" aria-label="Theme">
					<button type="button" className={theme === "light" ? "active" : undefined} aria-pressed={theme === "light"} onClick={() => setTheme("light")}>
						Light
					</button>
					<button type="button" className={theme === "dark" ? "active" : undefined} aria-pressed={theme === "dark"} onClick={() => setTheme("dark")}>
						Dark
					</button>
				</div>

				<div className="ws-btn-row">
					<button type="button" className={`ws-btn ws-btn-small${rapid ? " ws-btn-primary" : " ws-btn-secondary"}`} onClick={() => { setChaos(false); setRapid((value) => !value); }} data-testid="mander-rapid-toggle">
						Rapid toggle
					</button>
					<button type="button" className={`ws-btn ws-btn-small${chaos ? " ws-btn-primary" : " ws-btn-secondary"}`} onClick={() => { setRapid(false); setChaos((value) => !value); }} data-testid="mander-chaos">
						Chaos
					</button>
				</div>

				<div className="mander-lab-matrix" role="group" aria-label="State matrix">
					{MATRIX.map((cell) => (
						<button
							key={cell.key}
							type="button"
							className={`ws-btn ws-btn-small${matrixActive === transitionKey(cell.to, cell.to) && state === cell.to ? " ws-btn-primary" : " ws-btn-secondary"}`}
							data-testid={`mander-matrix-${cell.key.replace("->", "-to-")}`}
							onClick={() => {
								setRapid(false);
								setChaos(false);
								setState(cell.from);
								window.requestAnimationFrame(() => setState(cell.to));
							}}
						>
							{cell.label}
						</button>
					))}
				</div>
			</section>
		</div>
	);
}

export function ManderLabGate() {
	const [open, setOpen] = useState(() => window.location.hash === "#mander-lab");

	useEffect(() => {
		const sync = () => setOpen(window.location.hash === "#mander-lab");
		window.addEventListener("hashchange", sync);
		return () => window.removeEventListener("hashchange", sync);
	}, []);

	if (!open) return null;
	return <ManderLab />;
}
