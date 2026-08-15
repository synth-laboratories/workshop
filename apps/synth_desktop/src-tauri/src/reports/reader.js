(() => {
  const data = JSON.parse(document.getElementById("synth-report-data").textContent);
  const revision = data.revision || {};
  const missingMark = "—";
  const decisionKinds = {
    hypothesis: 1,
    decision: 1,
    protocol_change: 1,
    correction: 1,
    claim_decision: 1,
    limitation: 1,
  };

  function text(value) {
    return value === null || value === undefined || value === "" ? "" : String(value);
  }
  function escape(value) {
    return text(value).replace(/[&<>"']/g, (ch) => ({
      "&": "&amp;",
      "<": "&lt;",
      ">": "&gt;",
      '"': "&quot;",
      "'": "&#39;",
    })[ch]);
  }
  function display(value) {
    const raw = text(value);
    return raw ? escape(raw) : missingMark;
  }
  function field(obj, ...keys) {
    for (const key of keys) {
      if (obj && obj[key] !== undefined && obj[key] !== null && obj[key] !== "") return obj[key];
    }
    return null;
  }
  function el(tag, attrs, html) {
    const node = document.createElement(tag);
    Object.entries(attrs || {}).forEach(([name, value]) => {
      if (value !== undefined && value !== null && value !== false) node.setAttribute(name, value === true ? "" : String(value));
    });
    if (html !== undefined) node.innerHTML = html;
    return node;
  }
  function prose(markdown) {
    const escaped = escape(markdown || "").replace(/\*\*(.+?)\*\*/g, "<strong>$1</strong>");
    return escaped
      .split(/\n{2,}/)
      .filter(Boolean)
      .map((para) => `<p>${para.replace(/\n/g, "<br>")}</p>`)
      .join("");
  }
  function missingNode() {
    return el("p", { class: "missing" }, missingMark);
  }
  function isMissing(block) {
    return field(block, "accessState", "access_state") === "missing";
  }

  const root = document.getElementById("app");
  document.title = text(revision.title) || "Sealed Report";

  const header = el("header");
  header.append(
    el("p", { class: "kicker" }, `Report · ${escape(field(revision, "report_id", "reportId") || data.report_id)} · rev ${escape(field(revision, "revision"))}`),
    el("h1", {}, escape(revision.title)),
    el("p", { class: "summary" }, escape(revision.summary || "")),
  );
  if (Array.isArray(revision.authors) && revision.authors.length) {
    header.append(el("p", { class: "authors" }, escape(revision.authors.join(" · "))));
  }
  root.append(header);

  const outline = Array.isArray(data.outline) ? data.outline : [];
  if (outline.length) {
    const nav = el("nav", { class: "outline", "aria-label": "Generated outline" });
    nav.append(el("strong", {}, "Outline"));
    const list = el("ol");
    outline.forEach((item) => {
      const li = el("li");
      li.append(el("a", { href: `#${escape(item.anchor)}` }, escape(item.title)));
      list.append(li);
    });
    nav.append(list);
    root.append(nav);
  }

  function renderBlock(block) {
    const kind = text(block.kind);
    const anchor = text(block.anchor);
    if (kind === "report.outline.v1" || anchor === "outline") return;
    const section = el("section", { id: anchor });
    section.append(el("h2", {}, escape(block.title || kind)));
    if (isMissing(block)) {
      section.append(missingNode());
      root.append(section);
      return;
    }
    const payload = block.payload || {};
    if (kind === "report.prose.v1" || payload.markdown) {
      section.append(el("div", { class: "prose" }, prose(payload.markdown || "")));
    } else if (kind === "report.experiment-records.v1") {
      return;
    } else if (kind === "report.research-log.v1") {
      return;
    } else if (kind === "report.claim.v1") {
      section.append(el("p", {}, `<span class="chip">${display(payload.status)}</span>${escape(payload.statement || payload.markdown || "")}`));
    } else if (kind === "report.trace-v5.v1") {
      const mount = el("div", { class: "trace-viewer" });
      const traces = Array.isArray(payload.traces) ? payload.traces : (payload.projection ? [{ projection: payload.projection, label: payload.label }] : []);
      if (window.SynthRolloutInspector) {
        window.SynthRolloutInspector.mount(mount, { traces, emptyMessage: missingMark });
      } else {
        mount.append(missingNode());
      }
      section.append(mount);
    } else if (kind === "report.result.v1" && payload.schema_version === "craftax.compare-story.v1") {
      const mount = el("div", { class: "compare-story" });
      if (window.SynthCompareStory) {
        window.SynthCompareStory.mount(mount, payload);
      } else {
        mount.append(missingNode());
      }
      section.append(mount);
    } else if ((kind === "report.visual.v1" || kind === "report.diagram.v1") && payload.sealedHtml) {
      const frame = el("iframe", {
        class: "sealed-visual",
        title: block.title || (kind === "report.diagram.v1" ? "Sealed diagram" : "Sealed visual"),
        sandbox: "allow-scripts",
      });
      frame.srcdoc = payload.sealedHtml;
      section.append(frame);
    } else {
      const identity = field(payload, "visualId", "visual_id", "collectionId", "collection_id", "diagramId", "diagram_id");
      const card = el("div", { class: "evidence-card" });
      card.append(el("div", { class: "kind" }, escape(kind)));
      card.append(el("div", {}, identity ? `Frozen identity ${escape(identity)}` : "Frozen evidence is attached to this revision."));
      const sourceRevision = field(block, "sourceRevision", "source_revision") || field(payload, "sourceRevision", "source_revision");
      if (sourceRevision) {
        card.append(el("div", { class: "path" }, `revision ${escape(sourceRevision)}`));
      }
      section.append(card);
    }
    root.append(section);
  }

  (revision.blocks || []).forEach(renderBlock);

  const limitations = revision.limitations || [];
  const claims = revision.claims || [];
  if (limitations.length) {
    const section = el("section", { id: "limitations" });
    section.append(el("h2", {}, "Limitations"));
    const list = el("ul", { class: "limitations" });
    limitations.forEach((item) => {
      list.append(el("li", {}, escape(typeof item === "string" ? item : item.body || item.limitationId || missingMark)));
    });
    section.append(list);
    root.append(section);
  }
  if (claims.length) {
    const section = el("section", { id: "claims" });
    section.append(el("h2", {}, "Claims"));
    const list = el("ul", { class: "claims" });
    claims.forEach((claim) => {
      list.append(el("li", {}, `<span class="chip">${display(claim.status)}</span>${escape(claim.statement || "")}`));
    });
    section.append(list);
    root.append(section);
  }

  const experiments = Array.isArray(data.experiments) && data.experiments.length
    ? data.experiments
    : ((revision.blocks || []).find((block) => block.kind === "report.experiment-records.v1") || { payload: {} }).payload.records || [];
  const experimentSection = el("section", { id: "experiment-records" });
  const expHead = el("header", { class: "section-head" });
  expHead.append(el("h2", {}, "Experiment Records"));
  const expTabs = el("nav", { class: "tabs", "aria-label": "Experiment views" });
  const expPane = el("div");
  let expView = "ledger";
  let selectedExp = experiments[0] || null;
  function primaryResult(row) {
    const results = row.results || row.payload && row.payload.results;
    const first = Array.isArray(results) ? results[0] : null;
    return first && (first.reward !== undefined ? first.reward : first.primaryMetric);
  }
  function renderExperiments() {
    expPane.innerHTML = "";
    if (!experiments.length) {
      expPane.append(el("p", { class: "missing" }, "No experiment records in this revision."));
      return;
    }
    if (expView === "ledger") {
      const table = el("table", { class: "ledger" });
      table.innerHTML = "<thead><tr><th>Experiment</th><th>Status</th><th>Protocol</th><th>Primary result</th></tr></thead>";
      const body = el("tbody");
      experiments.forEach((row) => {
        const tr = el("tr", { class: selectedExp === row ? "active" : undefined });
        tr.innerHTML = `<td>${display(row.title)}</td><td>${display(row.status)}</td><td>${display(field(row, "protocolDigest", "protocol_digest"))}</td><td>${display(primaryResult(row))}</td>`;
        tr.addEventListener("click", () => {
          selectedExp = row;
          expView = "inspector";
          renderExperiments();
        });
        body.append(tr);
      });
      table.append(body);
      expPane.append(table);
    } else if (expView === "lineage") {
      const list = el("ul", { class: "lineage" });
      experiments.forEach((row) => {
        const li = el("li");
        const arms = Array.isArray(row.arms) ? row.arms.length : missingMark;
        const runs = Array.isArray(row.runs) ? row.runs.length : missingMark;
        li.innerHTML = `<strong>${display(row.title)}</strong><div class="path">protocol ${display(field(row, "protocolDigest", "protocol_digest"))}</div><div class="path">arms ${escape(arms)} → runs ${escape(runs)}</div>`;
        list.append(li);
      });
      expPane.append(list);
    } else if (selectedExp) {
      const dl = el("dl", { class: "inspector" });
      [
        ["Experiment", field(selectedExp, "experimentId", "experiment_id")],
        ["Hypothesis", selectedExp.hypothesis],
        ["Status", selectedExp.status],
        ["Protocol digest", field(selectedExp, "protocolDigest", "protocol_digest")],
        ["Primary result", primaryResult(selectedExp)],
      ].forEach(([label, value]) => {
        dl.append(el("dt", {}, escape(label)), el("dd", {}, display(value)));
      });
      expPane.append(dl);
    }
    [...expTabs.querySelectorAll("button")].forEach((button) => {
      button.className = button.getAttribute("data-view") === expView ? "active" : "";
    });
  }
  ["ledger", "lineage", "inspector"].forEach((view, index) => {
    const label = ["Ledger", "Lineage", "Run inspector"][index];
    const button = el("button", { type: "button", "data-view": view, class: view === expView ? "active" : undefined }, label);
    button.addEventListener("click", () => {
      expView = view;
      renderExperiments();
    });
    expTabs.append(button);
  });
  expHead.append(expTabs);
  experimentSection.append(expHead, expPane);
  root.append(experimentSection);
  renderExperiments();

  const log = Array.isArray(data.research_log) ? data.research_log : [];
  const logSection = el("section", { id: "research-log" });
  const logHead = el("header", { class: "section-head" });
  logHead.append(el("h2", {}, "Research Log"));
  const logTabs = el("nav", { class: "tabs", "aria-label": "Research log views" });
  const logPane = el("div");
  let logView = "timeline";
  let selectedLog = log[0] || null;
  function renderLog() {
    logPane.innerHTML = "";
    const rows = logView === "decisions" ? log.filter((entry) => decisionKinds[entry.entryKind || entry.entry_kind]) : log;
    if (!rows.length) {
      logPane.append(el("p", { class: "missing" }, "No research-log entries in this revision."));
      return;
    }
    const list = el("ol", { class: "log" });
    rows.forEach((entry) => {
      const li = el("li", { class: selectedLog === entry ? "active" : undefined });
      li.innerHTML = `<strong>${display(entry.title)}</strong><div class="meta">${display(field(entry, "entryKind", "entry_kind"))} · ${display(entry.author)} · ${display(field(entry, "occurredAt", "occurred_at"))}</div><p>${escape(entry.body || "")}</p>${field(entry, "supersedesEntryId", "supersedes_entry_id") ? `<p class="missing">Corrects ${escape(field(entry, "supersedesEntryId", "supersedes_entry_id"))}</p>` : ""}`;
      li.addEventListener("click", () => {
        selectedLog = entry;
        logView = "inspector";
        renderLog();
      });
      list.append(li);
    });
    logPane.append(list);
    if (logView === "inspector" && selectedLog) {
      const dl = el("dl", { class: "inspector" });
      [
        ["Entry", field(selectedLog, "entryId", "entry_id")],
        ["Kind", field(selectedLog, "entryKind", "entry_kind")],
        ["Claim effect", field(selectedLog, "claimEffect", "claim_effect")],
      ].forEach(([label, value]) => {
        dl.append(el("dt", {}, escape(label)), el("dd", {}, display(value)));
      });
      logPane.append(dl);
    }
    [...logTabs.querySelectorAll("button")].forEach((button) => {
      button.className = button.getAttribute("data-view") === logView ? "active" : "";
    });
  }
  ["timeline", "decisions", "inspector"].forEach((view, index) => {
    const label = ["Timeline", "Decision trail", "Entry inspector"][index];
    const button = el("button", { type: "button", "data-view": view, class: view === logView ? "active" : undefined }, label);
    button.addEventListener("click", () => {
      logView = view;
      renderLog();
    });
    logTabs.append(button);
  });
  logHead.append(logTabs);
  logSection.append(logHead, logPane);
  root.append(logSection);
  renderLog();

  const compiler = data.compiler || {};
  root.append(
    el(
      "footer",
      { class: "provenance" },
      `Report ${escape(field(revision, "report_id", "reportId"))} · revision ${escape(revision.revision)} · digest ${display(field(revision, "content_digest", "contentDigest"))} · ${escape(compiler.name || "workshop")} ${escape(compiler.version || "")}`,
    ),
  );
})();
