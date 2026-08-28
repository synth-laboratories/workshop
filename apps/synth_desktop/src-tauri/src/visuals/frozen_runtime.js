// The sealed viewer. It renders projections; it does not compute them.
//
// This file used to locate a projection by scanning `data.bindings` for a
// known schema version, and for a live-eval visual there was nothing to find:
// it printed the raw envelope JSON into a `<pre>`. That made the viewer a
// third implementation of a projection that has one home in Rust, and it made
// every sealed bundle depend on the viewer still agreeing with that home.
//
// The seal now names its own views. `data.projection.views[]` carries either
// an inline `data` (the live-eval fold's output, frozen at export time) or a
// `ref` — a JSON Pointer to a projection the template's own resolver already
// placed inside the sealed document. Either way the projection is *in* the
// bundle, so a seal keeps rendering after the plugin, the user template or the
// build that produced it is gone. Everything below is presentation.
(() => {
  const data = JSON.parse(document.getElementById("synth-artifact-data").textContent);
  const root = document.getElementById("app");
  root.innerHTML = "";
  const header = document.createElement("header");
  const kicker = document.createElement("p");
  kicker.className = "kicker";
  kicker.textContent = data.template_id;
  const title = document.createElement("h1");
  title.textContent = data.title;
  header.append(kicker, title);
  const section = document.createElement("section");
  section.className = "visual";
  root.append(header, section);

  const MISSING = "—";
  const TRACE_SCHEMA = "synth.trace-projection.rollout-inspector.v1";
  const LIVE_EVAL_SCHEMA = "synth.live-eval-projection.v1";

  // RFC 6901. The only indirection in this file, and it stays inside the
  // sealed document: a pointer names where a projection already is, never
  // where one might be recomputed from.
  function deref(document_, pointer) {
    if (typeof pointer !== "string" || pointer === "") return null;
    let node = document_;
    for (const raw of pointer.split("/").slice(1)) {
      const key = raw.replace(/~1/g, "/").replace(/~0/g, "~");
      if (node == null || typeof node !== "object") return null;
      node = Array.isArray(node) ? node[Number(key)] : node[key];
    }
    return node == null ? null : node;
  }

  function block(parent) {
    const node = document.createElement("div");
    parent.append(node);
    return node;
  }

  function definitions(parent, rows) {
    const list = document.createElement("dl");
    list.className = "inspector";
    for (const [label, value] of rows) {
      const term = document.createElement("dt");
      term.textContent = label;
      const detail = document.createElement("dd");
      detail.textContent = value === null || value === undefined || value === "" ? MISSING : String(value);
      list.append(term, detail);
    }
    parent.append(list);
  }

  // A tally of a sealed array of strings. Presentation, not a fold: the array
  // is one entry per projected envelope and was written by the Rust fold.
  function tally(values) {
    const counts = new Map();
    for (const value of values || []) counts.set(value, (counts.get(value) || 0) + 1);
    return [...counts.entries()].sort((a, b) => b[1] - a[1] || String(a[0]).localeCompare(String(b[0])));
  }

  function renderLiveEval(parent, projection) {
    const usage = projection.usage || {};
    const cutoff = projection.cutoff || {};
    const streams = Object.keys(cutoff);
    definitions(parent, [
      ["projected envelopes", projection.event_count],
      ["live frames", projection.has_live_frames ? "yes" : "no"],
      ["reward.txt emitted", projection.has_reward_txt ? "yes" : "no"],
      ["reward", projection.reward],
      ["input tokens", usage.prompt_tokens],
      ["output tokens", usage.completion_tokens],
      ["total tokens", usage.total_tokens],
      ["cost (usd)", usage.cost_usd],
      ["cutoff", streams.length ? streams.map((id) => `${id}:${cutoff[id]}`).join(" · ") : "whole prefix"],
    ]);
    const kinds = tally(projection.kinds);
    if (kinds.length) {
      const list = document.createElement("ul");
      for (const [kind, count] of kinds) {
        const row = document.createElement("li");
        row.textContent = `${kind} · ${count}`;
        list.append(row);
      }
      const heading = document.createElement("h2");
      heading.textContent = "Envelope kinds";
      parent.append(heading, list);
    }
  }

  function renderView(view, projection) {
    const parent = block(section);
    if (view.schema_version === TRACE_SCHEMA && window.SynthRolloutInspector) {
      window.SynthRolloutInspector.mount(parent, {
        traces: [{ label: data.title, projection }],
        emptyMessage: MISSING,
      });
      return true;
    }
    if (view.schema_version === LIVE_EVAL_SCHEMA) {
      renderLiveEval(parent, projection);
      return true;
    }
    // A schema this build does not know still renders its own values rather
    // than nothing: the projection is sealed, so there is something to show.
    const pre = document.createElement("pre");
    pre.textContent = JSON.stringify(projection, null, 2);
    parent.append(pre);
    return true;
  }

  const views = data.projection && Array.isArray(data.projection.views) ? data.projection.views : [];
  let rendered = false;
  for (const view of views) {
    if (!view || typeof view !== "object") continue;
    const projection = view.data !== undefined && view.data !== null ? view.data : deref(data, view.ref);
    if (!projection) continue;
    rendered = renderView(view, projection) || rendered;
  }

  // The fallback the seal has always had, for a bundle that names no view:
  // the bindings it was sealed with, verbatim.
  if (!rendered) {
    const pre = document.createElement("pre");
    pre.textContent = JSON.stringify(data.bindings, null, 2);
    section.append(pre);
  }
})();
