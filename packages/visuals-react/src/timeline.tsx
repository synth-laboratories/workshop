import type { ChangeEvent } from "react";

export function Timeline({ value, min = 0, max, label, valueText, playing = false, follow = false, reducedMotion = false, onSeek, onTogglePlay, onFollow }: {
  value: number;
  min?: number;
  max: number;
  label: string;
  valueText?: string;
  playing?: boolean;
  follow?: boolean;
  reducedMotion?: boolean;
  onSeek: (value: number) => void;
  onTogglePlay?: () => void;
  onFollow?: () => void;
}) {
  return <div role="group" aria-label={label} className="visuals-timeline">
    {onTogglePlay ? <button type="button" aria-pressed={playing} onClick={onTogglePlay} disabled={reducedMotion}>{playing ? "Pause" : "Play"}</button> : null}
    <input type="range" min={min} max={Math.max(min, max)} value={Math.max(min, Math.min(max, value))} aria-label={label} aria-valuetext={valueText} onChange={(event: ChangeEvent<HTMLInputElement>) => onSeek(Number(event.target.value))} />
    <output aria-live="polite">{valueText ?? value}</output>
    {onFollow ? <button type="button" aria-pressed={follow} onClick={onFollow}>{follow ? "Following live" : "Follow live"}</button> : null}
  </div>;
}
export function useReducedMotion(): boolean {
  return typeof window !== "undefined" && window.matchMedia?.("(prefers-reduced-motion: reduce)").matches === true;
}
