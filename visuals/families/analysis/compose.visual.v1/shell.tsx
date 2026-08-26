import { useMemo, useState } from "react";
import { VisualChrome } from "../../../chrome/VisualChrome.tsx";
import { useLiveEvalStream } from "../../../chrome/useLiveEvalStream.ts";
import { DetailModal } from "../../../components/detail_modal.v1/DetailModal.tsx";
import { EventStream } from "../../../components/event_stream.v1/EventStream.tsx";
import { parseComposeSpec } from "../../../runtime/composeSpec.ts";
import type { LiveTemplateProps } from "../../../runtime/replayClient.ts";
import type { LiveEvalEvent, VisualBinding } from "../../../runtime/types.ts";

type StreamPayload = {
  events?: LiveEvalEvent[];
};

export type ShellProps = LiveTemplateProps & {
  title?: string;
  lede?: string;
  spec?: unknown;
  stream?: StreamPayload;
  bindings?: VisualBinding[] | { slots?: VisualBinding[] };
};

function asStream(raw: unknown): StreamPayload {
  if (raw && typeof raw === "object") return raw as StreamPayload;
  return {};
}

type Cursor = {
  placementId: string;
  identity: string;
  event: LiveEvalEvent;
};

export function Shell(props: ShellProps) {
  const parsed = parseComposeSpec(props.spec);
  const stream = asStream(props.stream);
  const declaredStreamCount = props.replay?.streams.length ?? 0;
  const fixtureEvents = useMemo(
    () => (declaredStreamCount > 0 ? undefined : stream.events),
    [declaredStreamCount, stream.events]
  );
  const hasSource = declaredStreamCount > 0 || Boolean(stream.events?.length);
  const { events, state, error } = useLiveEvalStream({
    replay: props.replay,
    fixtureEvents,
    visualId: props.visualId,
    revision: props.revision
  });
  const [cursor, setCursor] = useState<Cursor | null>(null);

  if (!parsed.ok) {
    return (
      <VisualChrome kicker="Compose" title={props.title ?? "Compose visual"} testId="visual-compose">
        <p role="alert" data-testid="visual-compose-invalid">
          {parsed.error}
        </p>
      </VisualChrome>
    );
  }

  const needsStream = parsed.spec.placements.some((row) => row.component === "event_stream.v1");
  if (needsStream && !hasSource) {
    return (
      <VisualChrome
        kicker="Compose"
        title={props.title ?? parsed.spec.title ?? "Compose visual"}
        testId="visual-compose"
      >
        <p role="alert" data-testid="visual-compose-invalid">
          Placement requires a bound stream slot
        </p>
      </VisualChrome>
    );
  }

  return (
    <VisualChrome
      kicker="Compose"
      live={state === "live"}
      title={props.title ?? parsed.spec.title ?? "Compose visual"}
      lede={props.lede ?? parsed.spec.lede}
      testId="visual-compose"
      footer="compose.visual.v1"
    >
      {parsed.spec.placements.map((placement) => {
        if (placement.component === "event_stream.v1") {
          return (
            <EventStream
              key={placement.id}
              events={events}
              state={state}
              error={error}
              includeKinds={placement.config?.includeKinds}
              cursorId={cursor?.placementId === placement.id ? cursor.identity : null}
              onSelect={(event, identity) => setCursor({ placementId: placement.id, identity, event })}
            />
          );
        }
        return (
          <DetailModal
            key={placement.id}
            event={cursor && cursor.placementId === placement.from ? cursor.event : null}
            onClose={() => setCursor(null)}
          />
        );
      })}
    </VisualChrome>
  );
}

export default Shell;
