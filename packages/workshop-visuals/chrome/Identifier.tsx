/**
 * Safe presentation for long identifiers (rollout IDs, digests, stream refs):
 * middle truncation, full value on hover/focus, one-click copy, and no
 * ability to widen the parent grid. Self-styled so any template can use it.
 */

import { useState, type CSSProperties } from "react";

function middleTruncate(value: string, max: number): string {
  if (value.length <= max) return value;
  const head = Math.ceil((max - 1) / 2);
  const tail = Math.floor((max - 1) / 2);
  return `${value.slice(0, head)}…${value.slice(value.length - tail)}`;
}

async function copyValue(text: string): Promise<void> {
  if (navigator.clipboard?.writeText) {
    await navigator.clipboard.writeText(text);
    return;
  }
  const textarea = document.createElement("textarea");
  textarea.value = text;
  textarea.style.position = "fixed";
  textarea.style.opacity = "0";
  document.body.appendChild(textarea);
  textarea.select();
  document.execCommand("copy");
  textarea.remove();
}

export function Identifier({
  value,
  label,
  max = 24,
  copy = true,
  style
}: {
  value: string;
  /** Optional human-friendly prefix shown before the truncated value. */
  label?: string;
  /** Maximum characters of the value shown inline. */
  max?: number;
  copy?: boolean;
  style?: CSSProperties;
}) {
  const [copied, setCopied] = useState(false);
  const truncated = middleTruncate(value, max);
  return (
    <span
      title={value}
      style={{
        display: "inline-flex",
        alignItems: "center",
        gap: 5,
        minWidth: 0,
        maxWidth: "100%",
        font: "11px ui-monospace, SFMono-Regular, Menlo, monospace",
        ...style
      }}
    >
      {label ? <span style={{ fontFamily: "inherit", opacity: 0.75 }}>{label}</span> : null}
      <span style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{truncated}</span>
      {copy ? (
        <button
          type="button"
          aria-label={`Copy ${label ?? "identifier"} ${value}`}
          onClick={(event) => {
            event.stopPropagation();
            void copyValue(value).then(() => {
              setCopied(true);
              window.setTimeout(() => setCopied(false), 1400);
            });
          }}
          style={{
            flexShrink: 0,
            padding: "0 5px",
            border: "1px solid currentColor",
            borderRadius: 5,
            background: "transparent",
            color: "inherit",
            font: "9px inherit",
            opacity: copied ? 1 : 0.65,
            cursor: "pointer"
          }}
        >
          {copied ? "copied" : "copy"}
        </button>
      ) : null}
    </span>
  );
}
