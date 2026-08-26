import { VisualChrome } from "../../../chrome/VisualChrome.tsx";

/** Fallback if the host did not compile stored TSX. VisualHost mounts the sourced module instead. */
export function Shell(props: { title?: string }) {
  return (
    <VisualChrome kicker="Sourced" title={props.title ?? "Custom visual"} testId="visual-sourced">
      <p role="alert" data-testid="visual-sourced-invalid">
        sourced.visual.v1 requires content
      </p>
    </VisualChrome>
  );
}

export default Shell;
