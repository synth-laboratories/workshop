(() => {
  const SCHEMA = "synth.trace-projection.rollout-inspector.v1";
  const MISSING = "—";
  const FAMILY_META = {
    message: { label: "Message", glyph: "◆", tint: "#eaf3ff" },
    tool: { label: "Tool", glyph: ">_", tint: "#eef8f1" },
    thought: { label: "Thought", glyph: "✦", tint: "#f6f0ff" },
    model: { label: "Model", glyph: "◌", tint: "#f2f4f7" },
    span: { label: "Span", glyph: "↔", tint: "#f2f4f7" },
    evidence: { label: "Evidence", glyph: "✓", tint: "#fff4e9" },
    system: { label: "Event", glyph: "·", tint: "#f7f7f8" },
  };
  const FOCUS = { message: 1, tool: 1, thought: 1, evidence: 1 };

  function object(value) {
    return value && typeof value === "object" && !Array.isArray(value) ? value : {};
  }
  function text(value) {
    return typeof value === "string" ? value : "";
  }
  function escape(value) {
    return String(value ?? "").replace(/[&<>"']/g, (ch) => ({
      "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;",
    })[ch]);
  }
  function family(item) {
    const kind = text(item.kind);
    if (kind.startsWith("evidence.")) return "evidence";
    if (kind.startsWith("span.")) return "span";
    if (kind.includes("command_") || kind.startsWith("tool.")) return "tool";
    if (kind.includes("reasoning") || kind.includes("thought")) return "thought";
    if (kind.startsWith("message.") || kind === "codex.agent_message") return "message";
    if (kind.startsWith("model_call.") || kind.includes("turn_")) return "model";
    return "system";
  }
  function native(item) {
    return object(object(item.detail).native);
  }
  function primary(item) {
    const detail = object(item.detail);
    const nested = object(detail.payload);
    const n = native(item);
    for (const value of [n.text, n.command, detail.reply, detail.action, detail.message, nested.reason, detail.content, detail.text, detail.rationale, detail.task_id]) {
      if (typeof value === "string" && value.trim()) return value;
    }
    if (typeof detail.score === "number") return `Score ${detail.score}`;
    if (typeof detail.call_index === "number") return `Model call ${detail.call_index}`;
    return item.title || item.kind || MISSING;
  }
  function output(item) {
    const n = native(item);
    const detail = object(item.detail);
    const value = n.aggregated_output ?? detail.output ?? detail.result;
    return typeof value === "string" ? value : value == null ? "" : JSON.stringify(value, null, 2);
  }
  function statusColor(status) {
    return /error|fail|invalid/i.test(status || "") ? "#b84235" : /pass|ok|complete|decisive|valid/i.test(status || "") ? "#238558" : "#77808d";
  }
  function duration(items) {
    const stamps = items.map((item) => Date.parse(item.occurred_at || "")).filter(Number.isFinite);
    if (stamps.length < 2) return MISSING;
    const seconds = Math.max(0, Math.round((Math.max(...stamps) - Math.min(...stamps)) / 1000));
    return seconds < 60 ? `${seconds}s` : seconds < 3600 ? `${Math.floor(seconds / 60)}m ${seconds % 60}s` : `${Math.floor(seconds / 3600)}h ${Math.floor((seconds % 3600) / 60)}m`;
  }
  function clock(value) {
    if (!value) return MISSING;
    const date = new Date(value);
    return Number.isNaN(date.valueOf()) ? value : date.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", second: "2-digit" });
  }
  function tokens(value) {
    return typeof value === "number" && Number.isFinite(value) ? value.toLocaleString() : MISSING;
  }
  function projectionOf(entry) {
    const raw = entry && (entry.projection || entry.payload || entry);
    return raw && raw.schema_version === SCHEMA ? raw : null;
  }
  function traceOptions(traces, selected) {
    const groups = [];
    const seen = new Map();
    traces.forEach((row, index) => {
      const label = row.label || row.traceId || row.trace_id || `trace ${index + 1}`;
      const arm = String(label).split(" · ")[0] || "Traces";
      if (!seen.has(arm)) {
        seen.set(arm, groups.length);
        groups.push({ arm, rows: [] });
      }
      groups[seen.get(arm)].rows.push({ index, label });
    });
    return groups.map((group) => `<optgroup label="${escape(group.arm)}">${group.rows.map((row) => `<option value="${row.index}" ${row.index === selected ? "selected" : ""}>${escape(row.label)}</option>`).join("")}</optgroup>`).join("");
  }

  function mount(root, options) {
    const traces = (options && options.traces) || [];
    root.innerHTML = "";
    root.classList.add("sv-inspector");
    root.setAttribute("data-testid", "visual-trace-rollout-inspector");
    if (!traces.length) {
      root.innerHTML = `<p class="missing">${MISSING}</p>`;
      return;
    }
    const state = { index: 0, tab: "trace", density: "focus", lane: "all", query: "", expand: false };
    const shell = document.createElement("div");
    root.append(shell);

    function render() {
      const entry = traces[state.index] || {};
      const payload = projectionOf(entry);
      if (!payload) {
        const picker = traces.length > 1
          ? `<label class="sv-field">Trace <select data-role="trace">${traceOptions(traces, state.index)}</select></label>`
          : "";
        shell.innerHTML = `
          <p class="sv-kicker">Trace V5 · sealed · rollout inspector</p>
          <h3 class="sv-title">${escape(entry.label || "Trace")}</h3>
          ${picker}
          <p class="missing">${escape(options.emptyMessage || MISSING)}</p>
          <p class="sv-footer">trace.rollout_inspector.v1 · projection is read-only</p>
        `;
        shell.querySelector("[data-role=trace]")?.addEventListener("change", (event) => {
          state.index = Number(event.target.value);
          render();
        });
        return;
      }
      const visual = payload.visual || {};
      const items = visual.items || [];
      const lanes = visual.lanes || [];
      const summary = visual.summary || {};
      const usage = visual.usage || {};
      const evidence = items.filter((item) => family(item) === "evidence");
      const tools = items.filter((item) => family(item) === "tool");
      const filtered = items.filter((item) => {
        const needle = state.query.trim().toLowerCase();
        const fam = family(item);
        return (state.lane === "all" || item.lane_id === state.lane)
          && (state.density === "full" || FOCUS[fam] || /error|fail/i.test(item.status || ""))
          && (!needle || `${item.kind} ${item.title || ""} ${primary(item)} ${output(item)}`.toLowerCase().includes(needle));
      });
      const picker = traces.length > 1
        ? `<label class="sv-field">Trace <select data-role="trace">${traceOptions(traces, state.index)}</select></label>`
        : "";
      const digest = payload.trace_digest || entry.traceDigest || entry.trace_digest || MISSING;
      shell.innerHTML = `
        <p class="sv-kicker">Trace V5 · sealed · rollout inspector</p>
        <h3 class="sv-title">${escape(entry.label || payload.trace_id || "Trace")}</h3>
        <p class="sv-lede">${escape((visual.task_id || visual.run_id || "Agent trajectory") + " · " + String(digest).slice(0, 23))}…</p>
        ${picker}
        <div class="sv-metrics">
          <div><span>Events</span><strong>${escape(summary.visual_item_count ?? items.length)}</strong></div>
          <div><span>Duration</span><strong>${escape(duration(items))}</strong></div>
          <div><span>Tool calls</span><strong>${tools.length}</strong></div>
          <div><span>Evidence</span><strong>${evidence.length}</strong></div>
        </div>
        <nav class="sv-tabs" aria-label="Trace views">
          ${["trace", "evidence", "metadata"].map((tab) => `<button type="button" data-tab="${tab}" class="${state.tab === tab ? "active" : ""}">${tab}</button>`).join("")}
        </nav>
        <div data-role="body"></div>
        <p class="sv-footer">trace.rollout_inspector.v1 · projection is read-only</p>
      `;
      const body = shell.querySelector("[data-role=body]");
      if (state.tab === "trace") {
        body.innerHTML = `
          <div class="sv-controls">
            <div class="sv-density" role="group" aria-label="Trace density">
              ${["focus", "full"].map((value) => `<button type="button" data-density="${value}" class="${state.density === value ? "active" : ""}">${value}</button>`).join("")}
            </div>
            <select data-role="lane" aria-label="Trace lane">
              <option value="all">all lanes</option>
              ${lanes.map((lane) => `<option value="${escape(lane.lane_id)}" ${state.lane === lane.lane_id ? "selected" : ""}>${escape(lane.display_name || lane.role || lane.lane_id)}</option>`).join("")}
            </select>
            <input data-role="search" aria-label="Search trace" placeholder="Search commands, output, messages…" value="${escape(state.query)}">
          </div>
          <p class="sv-count">${filtered.length} of ${items.length} projected items · ${state.density === "focus" ? "operational signal" : "complete projection"}</p>
          <div class="sv-events" data-role="events">${filtered.map((item) => eventCard(item, state.expand)).join("") || `<p class="missing">No projected items match these filters.</p>`}</div>
        `;
      } else if (state.tab === "evidence") {
        body.innerHTML = evidence.length
          ? `<div class="sv-evidence-list">${evidence.map((item) => `
              <article class="sv-evidence" style="border-left-color:${statusColor(item.status)}">
                <div class="sv-row"><strong>${escape(item.title || item.kind)}</strong><span style="color:${statusColor(item.status)}">${escape(item.status || MISSING)}</span></div>
                <pre>${escape(JSON.stringify(item.detail || {}, null, 2))}</pre>
              </article>`).join("")}</div>`
          : `<p>No evaluation evidence was captured in this sealed trace.</p>`;
      } else {
        body.innerHTML = `
          <dl class="inspector">
            <dt>trace id</dt><dd>${escape(payload.trace_id)}</dd>
            <dt>trace digest</dt><dd>${escape(payload.trace_digest)}</dd>
            <dt>evidence digest</dt><dd>${escape(payload.evidence_digest || MISSING)}</dd>
            <dt>run</dt><dd>${escape(visual.run_id || MISSING)}</dd>
            <dt>task</dt><dd>${escape(visual.task_id || MISSING)}</dd>
            <dt>visibility</dt><dd>${escape(visual.visibility_ceiling || MISSING)}</dd>
            <dt>input tokens</dt><dd>${escape(tokens(usage.prompt_tokens))}</dd>
            <dt>output tokens</dt><dd>${escape(tokens(usage.completion_tokens))}</dd>
          </dl>
        `;
      }
      shell.querySelector("[data-role=trace]")?.addEventListener("change", (event) => {
        state.index = Number(event.target.value);
        render();
      });
      shell.querySelectorAll("[data-tab]").forEach((button) => button.addEventListener("click", () => {
        state.tab = button.getAttribute("data-tab");
        render();
      }));
      shell.querySelectorAll("[data-density]").forEach((button) => button.addEventListener("click", () => {
        state.density = button.getAttribute("data-density");
        render();
      }));
      shell.querySelector("[data-role=lane]")?.addEventListener("change", (event) => {
        state.lane = event.target.value;
        render();
      });
      const search = shell.querySelector("[data-role=search]");
      if (search) {
        search.addEventListener("input", (event) => { state.query = event.target.value; });
        search.addEventListener("keydown", (event) => {
          if (event.key === "Enter") render();
        });
        search.addEventListener("blur", () => render());
      }
    }

    function eventCard(item, expand) {
      const meta = FAMILY_META[family(item)];
      const body = primary(item);
      const toolOutput = output(item);
      return `<article class="sv-card" id="trace-${escape(item.item_id)}" data-testid="trace-item-${escape(item.item_id)}">
        <aside><div>#${escape(item.sequence ?? "·")}</div><time>${escape(clock(item.occurred_at))}</time></aside>
        <div class="sv-card-body">
          <header style="background:${meta.tint}"><span>${meta.glyph}</span><strong>${meta.label}</strong><em>${escape(item.kind)}</em>${item.status ? `<b style="color:${statusColor(item.status)}">${escape(item.status)}</b>` : ""}</header>
          <div class="sv-card-copy ${expand ? "open" : ""}">${escape(body)}</div>
          ${toolOutput ? `<pre class="${expand ? "open" : ""}">${escape(toolOutput)}</pre>` : ""}
        </div>
      </article>`;
    }

    render();
  }

  function extractProjection(bindings) {
    if (!bindings) return null;
    if (bindings.schema_version === SCHEMA) return bindings;
    const list = Array.isArray(bindings) ? bindings : Object.values(object(bindings));
    for (const row of list) {
      const data = object(row).data || object(row).projection || row;
      if (data && data.schema_version === SCHEMA) return data;
      if (object(row).slot === "projection") {
        const nested = object(row).data || object(object(row).source);
        if (nested && nested.schema_version === SCHEMA) return nested;
      }
    }
    return null;
  }

  window.SynthRolloutInspector = { SCHEMA, mount, extractProjection, projectionOf };
})();
