/*
 * This file is deliberately a static public asset rather than an inline frame
 * script.  Sandboxed data/srcdoc frames inherit the Desktop WebView CSP on
 * macOS, which means a renderer can be imported and selected but never start.
 * The only code this runtime evaluates is the reviewed, immutable renderer
 * body supplied by the native visual-content bridge.
 */
(() => {
  let initialized = false;

  const report = (type, message) => parent.postMessage({ type, message: String(message || "Managed renderer failed") }, "*");

  const renderSource = (source) => {
    const parsed = new DOMParser().parseFromString(source, "text/html");
    const external = [...parsed.querySelectorAll("script[src]")];
    if (external.length > 0) throw new Error("Managed renderer contains an external script");

    document.querySelectorAll("style[data-synth-managed]").forEach((node) => node.remove());
    for (const style of parsed.querySelectorAll("style")) {
      const copy = document.createElement("style");
      copy.dataset.synthManaged = "true";
      copy.textContent = style.textContent;
      document.head.append(copy);
    }

    document.body.replaceChildren(...[...parsed.body.childNodes].filter((node) => node.nodeName !== "SCRIPT"));
    const scripts = [...parsed.querySelectorAll("script:not([src])")];
    if (scripts.length === 0) throw new Error("Managed renderer has no inline runtime");
    for (const script of scripts) {
      // The frame's CSP permits eval only for this app-bundled relay. Imported
      // sources are admission-checked before they reach this opaque sandbox.
      new Function(script.textContent || "")();
    }
  };

  addEventListener("error", (event) => report("synth.visual.managed.error", event.message));
  addEventListener("unhandledrejection", (event) => report("synth.visual.managed.error", event.reason));
  addEventListener("message", (event) => {
    const data = event.data || {};
    try {
      if (data.type === "synth.visual.managed.load.v1") {
        if (!initialized) {
          renderSource(String(data.source || ""));
          initialized = true;
          report("synth.visual.managed.ready", "ready");
        }
        dispatchEvent(new MessageEvent("message", { data: { type: "synth.visual.update.v1", payload: data.payload || {} } }));
      }
    } catch (error) {
      report("synth.visual.managed.error", error && error.message ? error.message : error);
    }
  });
  report("synth.visual.managed.ready", "runtime-ready");
})();
