/** Shared accessible timeline scrubber for rollout / live templates. */
import type { ChangeEvent } from "react";

export type TimelineScrubberProps = {
  index: number;
  total: number;
  onChange: (index: number) => void;
  playing?: boolean;
  onTogglePlay?: () => void;
  label?: string;
  valueText?: string;
};

export function TimelineScrubber({
  index,
  total,
  onChange,
  playing,
  onTogglePlay,
  label = "Timeline",
  valueText
}: TimelineScrubberProps) {
  const max = Math.max(0, total - 1);
  const text = valueText ?? `Step ${index + 1} of ${Math.max(total, 1)}`;

  return (
    <div
      role="group"
      aria-label={label}
      style={{
        display: "flex",
        alignItems: "center",
        gap: 10,
        marginTop: 10
      }}
    >
      {onTogglePlay ? (
        <button
          type="button"
          className="sv-btn"
          aria-pressed={playing ? true : false}
          aria-label={playing ? "Pause playback" : "Play playback"}
          onClick={onTogglePlay}
        >
          {playing ? "Pause" : "Play"}
        </button>
      ) : null}
      <input
        className="sv-scrubber"
        type="range"
        min={0}
        max={max}
        value={Math.min(index, max)}
        aria-valuemin={0}
        aria-valuemax={max}
        aria-valuenow={index}
        aria-valuetext={text}
        aria-label={label}
        onChange={(e: ChangeEvent<HTMLInputElement>) => onChange(Number(e.target.value))}
      />
      <span className="sv-mono" aria-live="polite">
        {text}
      </span>
    </div>
  );
}
