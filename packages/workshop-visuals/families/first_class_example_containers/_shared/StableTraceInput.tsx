import { useMemo, useRef, type InputHTMLAttributes } from "react";

type Props = Pick<InputHTMLAttributes<HTMLInputElement>,
  "value" | "type" | "min" | "max" | "onChange" | "placeholder" | "aria-label" | "style">;

/** Keep the actual input element stable across unrelated session updates.
 * React transiently rewrites input.name even when input props are unchanged;
 * avoiding that commit preserves the strict native pixel mutation barrier. */
export function StableTraceInput({ onChange, ...props }: Props) {
  const handler = useRef(onChange);
  handler.current = onChange;
  // These deliberately limited props are all serializable presentation values.
  const identity = JSON.stringify(props);
  return useMemo(() => <input {...props} onChange={event => handler.current?.(event)} />, [identity]);
}
