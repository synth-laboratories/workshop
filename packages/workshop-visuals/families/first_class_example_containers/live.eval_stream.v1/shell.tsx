import { useVisualState } from "@synth/visuals-react";
import { eventCursorSchema, resolveEventCursor } from "../../../runtime/presentationSchemas.ts";
import { useMemo } from "react";
import { VisualChrome } from "../../../chrome/VisualChrome.tsx";
import { useLiveEvalStream } from "../../../chrome/useLiveEvalStream.ts";
import { DetailModal } from "../../../components/detail_modal.v1/DetailModal.tsx";
import { EventStream } from "../../../components/event_stream.v1/EventStream.tsx";
import { Metrics } from "../../../components/metrics.v1/Metrics.tsx";
import { Scrubber } from "../../../components/scrubber.v1/Scrubber.tsx";
import type { LiveTemplateProps } from "../../../runtime/replayClient.ts";
import type { LiveEvalEvent, VisualBinding } from "../../../runtime/types.ts";

type StreamPayload = {
  run_id?: string;
  events?: LiveEvalEvent[];
  sse_url?: string;
};

export type ShellProps = LiveTemplateProps & {
  title?: string;
  lede?: string;
  stream?: StreamPayload;
  data?: StreamPayload;
  bindings?: VisualBinding[] | { inputs?: VisualBinding[]; slots?: VisualBinding[] };
};

function asStream(raw: unknown): StreamPayload {
  if (raw && typeof raw === "object") return raw as StreamPayload;
  return {};
}

type Cursor = {
  identity: string;
};

export function Shell(props: ShellProps) {
  const stream = asStream(props.stream ?? props.data);
  const declaredStreamCount = props.replay?.streams.length ?? 0;

  const fixtureEvents = useMemo(
    () => (declaredStreamCount > 0 ? undefined : stream.events),
    [declaredStreamCount, stream.events]
  );

  const { events, state, error } = useLiveEvalStream({
    replay: props.replay,
    fixtureEvents,
    visualId: props.visualId,
    revision: props.revision
  });
  const live = state === "live";
  const [cursor, setCursor] = useVisualState<Cursor | null>("eval.cursor", null, eventCursorSchema);

  return (
    <VisualChrome
      kicker="Live eval"
      live={live}
      title={props.title ?? `Run ${stream.run_id ?? events[0]?.run_id ?? "—"}`}
      lede={props.lede}
      testId="visual-live-eval-stream"
      footer="live.eval_stream.v1"
    >
      <Metrics events={events} />
      <Scrubber
        events={events}
        cursorId={cursor?.identity ?? null}
        onSelect={(_event, identity) => setCursor({ identity })}
      />
      <EventStream
        events={events}
        state={state}
        error={error}
        cursorId={cursor?.identity ?? null}
        onSelect={(_event, identity) => setCursor({ identity })}
      />
      <DetailModal event={resolveEventCursor(events, cursor?.identity)} onClose={() => setCursor(null)} />
    </VisualChrome>
  );
}

export default Shell;
