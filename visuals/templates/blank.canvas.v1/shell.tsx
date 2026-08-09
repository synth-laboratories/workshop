import type { VisualBinding } from "../../runtime/types.ts";

type CanvasDocument = {
  title?: string;
  description?: string;
  html: string;
  css?: string;
  background?: string;
  height?: number;
};

export type ShellProps = {
  title?: string;
  lede?: string;
  document?: CanvasDocument;
  data?: CanvasDocument;
  bindings?: VisualBinding[];
};

function escapeHtml(value: string): string {
  return value
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;");
}

function sourceFor(document: CanvasDocument, title: string): string {
  const description = document.description
    ? `<p class="canvas-description">${escapeHtml(document.description)}</p>`
    : "";
  return `<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1" />
  <title>${escapeHtml(title)}</title>
  <style>
    :root { color-scheme: light; font-family: Inter, ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif; color: #20242c; background: ${document.background ?? "#fff"}; }
    * { box-sizing: border-box; }
    html, body { margin: 0; min-height: 100%; background: ${document.background ?? "#fff"}; }
    body { padding: 24px; }
    .canvas-title { margin: 0; font-size: 24px; line-height: 1.15; letter-spacing: -.025em; }
    .canvas-description { margin: 8px 0 20px; color: #697180; font-size: 13px; line-height: 1.5; }
    svg { display: block; max-width: 100%; height: auto; }
    table { width: 100%; border-collapse: collapse; }
    img { max-width: 100%; }
    ${document.css ?? ""}
  </style>
</head>
<body>
  <header><h1 class="canvas-title">${escapeHtml(title)}</h1>${description}</header>
  <main>${document.html}</main>
</body>
</html>`;
}

export function Shell(props: ShellProps) {
  const document = props.data ?? props.document;
  const title = props.title ?? document?.title ?? "Untitled visual";
  if (!document?.html) {
    return (
      <div role="alert" style={{ padding: 24, color: "#697180" }}>
        <strong style={{ color: "#20242c" }}>Blank canvas</strong>
        <p style={{ marginBottom: 0 }}>No canvas document has been authored yet.</p>
      </div>
    );
  }
  const height = Math.min(2400, Math.max(320, document.height ?? 720));
  return (
    <iframe
      title={title}
      srcDoc={sourceFor(document, title)}
      sandbox=""
      referrerPolicy="no-referrer"
      style={{ display: "block", width: "100%", height, border: 0, background: document.background ?? "#fff" }}
    />
  );
}

export default Shell;
