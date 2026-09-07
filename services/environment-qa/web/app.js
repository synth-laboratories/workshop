"use strict";
let token = document.querySelector('meta[name="qa-token"]').content;
const byId = id => document.getElementById(id);
let selected = location.hash.slice(1), lastRevision = -1, busy = false;
let selectedInteraction = "";
const query = new URLSearchParams(location.search);
// Which client this is. Workshop loads the same page with embed=workshop, so the
// surface is knowable here and nowhere else; the service cross-checks it against
// the Referer the browser sets, which this script cannot author.
const SURFACE = query.get("embed") === "workshop" ? "workshop-embed" : "standalone-web";
// The operator secret, when the service was started with one. Held per tab so a
// CUA harness driving a different session does not inherit it.
function operatorToken() {
  try { return sessionStorage.getItem("qa-operator-token") || ""; } catch { return ""; }
}
const draftKey = (run, interaction) => `qa-review:${run.id}:${interaction.id}:${interaction.context_digest}`;
function savedDraft(run, interaction) {
  try { return localStorage.getItem(draftKey(run, interaction)) || ""; } catch { return ""; }
}
function notifyWorkshop(runs) {
  const parentOrigin = query.get("parentOrigin");
  if (window.parent === window || !parentOrigin) return;
  // Read-only status only. No tokens, evidence, decisions or mutation channel.
  try {
    const url = new URL(parentOrigin);
    if (!(parentOrigin === "tauri://localhost" || ["http:", "https:"].includes(url.protocol) &&
          ["localhost", "127.0.0.1", "tauri.localhost"].includes(url.hostname))) return;
    window.parent.postMessage({type:"workshop-qa:status", version:1,
      pending:runs.reduce((n,r) => n + r.interactions.filter(i => i.status === "open").length, 0)}, parentOrigin);
  } catch { /* A malformed embedding origin never receives status. */ }
}
const escapeHtml = value => String(value ?? "").replace(/[&<>"']/g, c => ({"&":"&amp;","<":"&lt;",">":"&gt;",'"':"&quot;","'":"&#39;"}[c]));
async function api(path, data, reconnected = false) {
  const operator = operatorToken();
  const response = await fetch(path, {method: data ? "POST" : "GET", headers: {"X-QA-Token": token, "Content-Type": "application/json",
    ...(operator ? {"X-QA-Operator": operator} : {})}, ...(data ? {body: JSON.stringify(data)} : {})});
  const body = await response.json();
  if (response.status === 401 && !reconnected) {
    const shell = await (await fetch("/", {cache:"no-store"})).text();
    token = new DOMParser().parseFromString(shell, "text/html").querySelector('meta[name="qa-token"]')?.content || "";
    return api(path, data, true);
  }
  if (!response.ok) throw new Error(body.error || "QA request failed");
  return body;
}
function error(reason) { byId("error").hidden = !reason; byId("error").textContent = reason?.message || ""; }
async function guarded(action) { try { error(null); await action(); } catch (reason) { error(reason); } }
function select(id) { selected = id; lastRevision = -1; location.hash = id; return refresh(); }
function executionSummary(run) {
  if (!run.policy.pipeline) return "Legacy baseline";
  const produced = new Set(run.evidence.map(e => e.gate));
  const errors = run.gates.filter(g => ["failed","inconclusive"].includes(g.status) && !run.evidence.some(e => e.gate === g.id && (e.result.assessment || g.executor === "disposition")));
  return `Execution: ${produced.size}/${run.gates.length} stages recorded · ${errors.length} execution gap(s). Quality: ${run.verdict || "pending"}.`;
}
function renderFindings(run) {
  const e = escapeHtml;
  const renderOne = f => `<article class="finding"><h3>${e(f.title)}</h3><small>${e(f.severity)} · ${e(f.disposition)} · ${e(f.gate_id || "review")}</small><button data-source="${e(f.path)}" data-evidence="${e(f.evidence_id || "")}">${e(f.path)}:${e(f.line)}</button><details><summary>Evidence and decision history</summary>${f.causal_claim ? `<p>${e(f.causal_claim)}</p><p>When: ${e(f.failure_condition)}</p><p>Impact: ${e(f.affected_behavior)}</p>` : ""}<pre>${e(f.evidence)}</pre>${(f.supporting_evidence || []).map(s => `<small>${e(s.path)}</small><pre>${e(s.evidence)}</pre>`).join("")}${(f.assessments || []).map(a => `<p>${e(a.gate_id)} · ${e(a.status)}: ${e(a.reason)}</p>`).join("")}</details></article>`;
  const active = run.findings.filter(f => f.disposition !== "dismissed");
  const dismissed = run.findings.filter(f => f.disposition === "dismissed");
  return `<h2>Findings · ${active.length} active</h2>${active.map(renderOne).join("") || '<p class="muted">No active findings. Check coverage before drawing a quality conclusion.</p>'}${dismissed.length ? `<details><summary>${dismissed.length} dismissed findings · audit history</summary>${dismissed.map(renderOne).join("")}</details>` : ""}`;
}
function renderContractCoverage(run) {
  const contracts = run.evidence.find(ev => ev.gate === "dependency-contracts")?.result.contracts || [];
  if (!contracts.length) return "";
  const coverage = run.evidence.find(ev => ev.gate === "contract-analysis")?.result.coverage || {};
  const measured = contracts.filter((_, index) => ["satisfied", "violated"].includes(coverage[index]?.status)).length;
  const e = escapeHtml;
  return `<details class="contract-coverage"><summary>Dependency checks: ${measured}/${contracts.length} assessed from observations · ${contracts.length - measured} untested</summary><p class="muted">AI assessment of these selected checks only—not complete task validation.</p>${contracts.map((contract, index) => {
    const assessment = coverage[index];
    const status = assessment?.status || "not_checked";
    return `<article><h4>${e(contract.package)} · ${e(status === "not_checked" ? "untested" : status)}</h4><p>${e(assessment?.reason || "No completed observation assessment.")}</p><small>${e(contract.contract)}</small></article>`;
  }).join("")}</details>`;
}
function renderDag(run) {
  const levels = new Map();
  const depth = gate => {
    if (!levels.has(gate.id)) levels.set(gate.id, gate.depends_on?.length ? 1 + Math.max(...gate.depends_on.map(id => depth(run.gates.find(g => g.id === id)))) : 0);
    return levels.get(gate.id);
  };
  run.gates.forEach(depth);
  return `<details open class="dag"><summary>QA workflow · ${run.gates.filter(g => g.status === "succeeded").length}/${run.gates.length} gates succeeded</summary>${Array.from(new Set(levels.values())).sort((a,b)=>a-b).map(level =>
    `<div class="dag-layer" aria-label="Dependency level ${level}">${run.gates.filter(g => levels.get(g.id) === level).map(g => `<button class="stage" data-gate="${escapeHtml(g.id)}" data-status="${escapeHtml(g.status)}"><span class="gate-name">${escapeHtml(g.id)}</span><span>${escapeHtml(g.status)}</span><small>After ${escapeHtml(g.depends_on?.join(", ") || "submission")}</small></button>`).join("")}</div>`).join("")}</details>`;
}
function interactionPresentation(interaction, now = Date.now() / 1000) {
  const type = interaction.type || "run_disposition";
  const expired = Number.isFinite(interaction.expires_at) && interaction.expires_at <= now;
  const reviewTypes = ["run_disposition", "assessment", "review", "probe_approval", "technical_review", "domain_review"];
  const typedPermission = type === "permission" && interaction.response_contract === "environment-qa.permission.v1";
  const typedClarification = type === "clarification" && interaction.response_contract === "environment-qa.clarification.v1";
  const supported = reviewTypes.includes(type) || typedPermission || typedClarification;
  const permission = /permission|requestApproval/.test(type);
  const clarification = /clarification|user_input/.test(type);
  const title = permission ? "Tool permission required" : clarification ? "Clarification required" : supported ? "Review required" : "Unsupported interaction";
  const allowed = interaction.allowed ?? ["confirm", "dismiss", "request_evidence"];
  return {title, supported, expired,
    decisions: supported && !expired && Array.isArray(allowed) ? allowed.filter(d => (typedPermission ? ["once", "reject"] : typedClarification ? ["answer", "reject"] : ["confirm", "dismiss", "request_evidence"]).includes(d)) : [],
    message: expired ? "This checkpoint has expired. Refresh for the current request; no decision will be submitted."
      : !supported ? "This request needs a typed response contract from the executor. Ordinary review approval cannot answer it. No response will be submitted."
      : typedPermission ? "This authorizes or refuses only the displayed tool request. It is not independent gold adjudication."
      : typedClarification ? "Answer only from authorized task context. This response is not a tool approval or independent gold adjudication."
      : "Review the evidence before deciding. This workflow decision is not a tool permission or independent gold adjudication."};
}
function chooseLaunchProfile(profiles, requestedMode) {
  const preferred = requestedMode === "hitl" ? "tbench-hitl" : "tbench-non-hitl";
  return profiles.some(profile => profile.id === preferred) ? preferred : "legacy";
}
function renderGateInspector(run, gateId, activity = null) {
  const e = escapeHtml, gate = run.gates.find(g => g.id === gateId);
  if (!gate) return "";
  const evidence = run.evidence.filter(ev => ev.gate === gate.id);
  const receipts = evidence.flatMap(ev => [ev.result?.receipt, ...(ev.result?.receipts || [])]).filter(r => r?.schema === "environment-qa.gate-receipt.v1" && r.gate_id === gate.id);
  const view = activity && activity.run_id === run.id ? activity.gates?.find(g => g.gate_id === gate.id && g.attempt === gate.attempt) : null;
  const receipt = view?.has_activity ? {state:view.session_state, model:run.policy?.pipeline?.model || "Not recorded", process:{pid:view.pid}, thread_id:view.thread_id, turn_ids:view.turn_ids, usage:view.usage,
    blocked_reason:view.blocking_reason, events:(view.tools || []).map((tool, index) => ({sequence:index + 1, kind:`${tool.tool}: ${tool.ok ? "succeeded" : "failed"}`}))} : receipts.at(-1);
  const duration = Number.isFinite(gate.started_at) && Number.isFinite(gate.finished_at) && gate.finished_at >= gate.started_at
    ? `${(gate.finished_at - gate.started_at).toFixed(1)}s wall time (includes any waits)` : "Not yet recorded";
  const waiting = run.interactions.filter(i => i.gate_id === gate.id && i.status === "open");
  const reason = receipt?.blocked_reason || gate.blocked_reason || evidence.flatMap(ev => ev.result?.limitations || []).join("; ");
  const events = receipt?.events || [];
  return `<section class="gate-inspector" aria-label="Gate inspector"><div class="row"><h2>${e(gate.id)}</h2><button id="close-gate">Close inspector</button></div>
    <p>${e(gate.status)} · ${e(gate.executor || "executor not recorded")}</p>
    <dl><dt>Duration</dt><dd>${e(duration)}</dd><dt>Depends on</dt><dd>${e(gate.depends_on?.join(", ") || "Submission")}</dd>
    <dt>Next action</dt><dd>${e(waiting.length ? "Resolve the checkpoint below" : reason || (["succeeded", "skipped", "cancelled"].includes(gate.status) ? "No action required for this gate" : "Await scheduler update; inspect evidence for gaps"))}</dd></dl>
    ${receipt ? `<p>Recorded executor receipt · ${e(receipt.state)} (not a live process health check)</p><dl><dt>Model</dt><dd>${e(receipt.model)}</dd><dt>Process ID</dt><dd>${e(receipt.process?.pid ?? "Not recorded / already stopped")}</dd><dt>Thread</dt><dd>${e(receipt.thread_id || "Not recorded")}</dd><dt>Turns</dt><dd>${e(receipt.turn_ids?.join(", ") || "Not recorded")}</dd></dl>
    <details><summary>Recorded usage</summary><pre>${e(JSON.stringify(receipt.usage ?? {}, null, 2))}</pre></details>
    <details><summary>Recent activity · ${events.length} recorded events</summary>${events.slice(-12).map(event => `<p>${e(event.sequence)} · ${e(event.kind)}</p>`).join("") || "No events recorded."}</details>`
    : '<p class="muted">No executor receipt exposed for this gate. Process, session and usage are unknown—not zero.</p>'}
    <details><summary>Evidence · ${evidence.length} record(s)</summary><pre>${e(JSON.stringify(evidence, null, 2))}</pre></details></section>`;
}
let inspectedGate = "", inspectedRun = "";
async function refreshInspector(run) {
  if (!inspectedGate || inspectedRun !== run.id) return;
  const gateId = inspectedGate;
  let activity;
  try { activity = await api(`/api/runs/${run.id}/activity`); }
  catch { return; } // Older stores/services have evidence but no activity endpoint.
  if (selected !== run.id || inspectedGate !== gateId) return;
  const root = byId("gate-inspector");
  const open = [...root.querySelectorAll("details")].map(d => d.open);
  root.innerHTML = renderGateInspector(run, gateId, activity);
  root.querySelectorAll("details").forEach((d, index) => d.open = open[index] || false);
  if (byId("close-gate")) byId("close-gate").onclick = () => { inspectedGate = ""; root.innerHTML = ""; };
}
byId("create").onsubmit = event => {
  event.preventDefault();
  guarded(async () => {
    const form = new FormData(event.target);
    const data = Object.fromEntries(form); data.probes = form.has("probes"); data.request_key = crypto.randomUUID();
    data.surface = SURFACE;
    // "legacy" is not a profile: it is the rules-only baseline, and sending it as
    // profile_id would be refused as unknown. Everything else launches by profile,
    // whose version and mode the server pins.
    if (data.profile_id === "legacy") { delete data.profile_id; data.budget_usd = 0; }
    else { data.budget_usd = Number(data.budget_usd); }
    const button = event.target.querySelector('button[type="submit"]'); button.disabled = true;
    try { const run = await api("/api/runs", data); await select(run.id); } finally { button.disabled = false; }
  });
};
function render(run) {
  const e = escapeHtml;
  const historyOpen = byId("decision-history")?.dataset.run === run.id && byId("decision-history").open;
  const openInteractions = run.interactions.filter(i => i.status === "open");
  const interaction = openInteractions.find(i => i.id === selectedInteraction) || openInteractions[0];
  const presentation = interaction ? interactionPresentation(interaction) : null;
  if (inspectedRun !== run.id) { inspectedGate = ""; inspectedRun = run.id; }
  const draft = interaction ? savedDraft(run, interaction) : "";
  byId("detail").innerHTML = `<div class="row"><div><h2>${e(run.policy.charter.name)}</h2><span class="muted">${e(run.id.slice(0,12))} · ${e(run.mode)} · ${e(run.policy.reviewer)}</span></div><span class="badge">${e(run.status)}${run.verdict ? " · " + e(run.verdict) : ""}</span></div>
  <p>${e(run.policy.charter.goal)}</p>${run.policy.task_goals ? `<p>${e(run.policy.task_goals)}</p>` : ""}
  <small>Snapshot ${e(run.bundle.sha256.slice(0,16))} · policy ${e(run.policy.pipeline?.profile || run.policy.version)} · maximum $${run.budget.limit_usd.toFixed(2)} · ${run.budget.transport ? `transport bound $${run.budget.transport.reserved_usd.toFixed(4)} (not an invoice)` : `reserved $${run.budget.reserved_usd.toFixed(4)}`}</small>
  <p class="execution-summary">${e(executionSummary(run))}</p>
  ${renderContractCoverage(run)}
  <div class="actions">${["queued","running","waiting_interaction"].includes(run.status) ? '<button data-control="pause">Pause</button>' : ""}${run.status === "paused" ? '<button data-control="resume">Resume</button>' : ""}${!["completed","cancelled","failed","cancelling"].includes(run.status) ? '<button data-control="cancel">Cancel</button>' : ""}<button id="copy-id">Copy run ID</button></div>
  ${interaction ? `<section class="review-context" aria-label="Current review checkpoint"><h2>Awaiting your decision</h2>${openInteractions.length > 1 ? `<label>Pending checkpoint<select id="interaction-select">${openInteractions.map(i => `<option value="${e(i.id)}" ${i.id === interaction.id ? "selected" : ""}>${e(i.gate_id || i.type || i.id)}</option>`).join("")}</select></label>` : ""}<p>${e(interaction.gate_id || "Findings review")} · ${e(interaction.type || "assessment")}</p><small>Evidence context ${e(interaction.context_digest)}${interaction.expires_at ? ` · expires ${e(new Date(interaction.expires_at * 1000).toLocaleString())}` : ""}</small><button id="jump-review">Go to decision</button><details><summary>Evidence for this checkpoint</summary>${run.evidence.filter(ev => !interaction.gate_id || run.gates.find(g => g.id === interaction.gate_id)?.depends_on?.includes(ev.gate)).map(ev => `<h3>${e(ev.gate)}</h3><pre>${e(JSON.stringify(ev.result,null,2))}</pre>`).join("") || "No direct dependency evidence recorded. Inspect the workflow and findings before deciding."}</details></section>` : ""}
  ${renderDag(run)}
  <div id="gate-inspector">${renderGateInspector(run, inspectedGate)}</div>
  <div class="columns"><section>${renderFindings(run)}
  <details><summary>Gate evidence</summary>${run.evidence.map(ev => `<h3>${e(ev.gate)}</h3><pre>${e(JSON.stringify(ev.result, null, 2))}</pre>${(ev.result.trials || []).map(t => `<button data-artifact="${e(t.log)}">${e(t.agent)} log</button>`).join("")}`).join("")}</details>
  <details><summary>Snapshot files · ${Object.keys(run.bundle.files).length}</summary><div class="actions">${Object.keys(run.bundle.files).map(f => `<button data-source="${e(f)}">${e(f)}</button>`).join("")}</div></details></section>
  <section class="review"><h2>${interaction ? "Review required" : "Disposition"}</h2>${interaction ? `<p>Resolve the current finding set. Decisions bind to the evidence shown.</p><label>Reason<textarea id="reason" rows="4" required></textarea></label><div class="actions"><button data-decision="confirm">Confirm assessment</button><button data-decision="dismiss">Dismiss findings</button><button data-decision="request_evidence">More evidence needed</button></div><small>More evidence records an inconclusive result. Submit a linked follow-up run for additional checks.</small>` : `<p>${run.seal ? "Predictions sealed. This prototype does not issue production qualification." : "Checks and decisions are persisted as the run progresses."}</p>`}
  ${run.seal ? `<small class="seal">Seal ${e(run.seal.sha256)}</small><div class="actions"><button id="export">Export sealed result</button><button id="followup">Review repaired version</button></div>` : ""}
  <details id="decision-history" data-run="${e(run.id)}" ${historyOpen ? "open" : ""}><summary>Decision history · ${run.interactions.filter(i => i.status !== "open").length}</summary>${run.interactions.filter(i => i.status !== "open").map(i => `<article><h3>${e(i.gate_id || "Findings review")}</h3><p>${e(i.status)} · ${e(i.decision || "no decision")} · ${e(i.actor || "not attributed")}</p><p>${e(i.reason || "")}</p><small>Context ${e(i.context_digest)}</small></article>`).join("") || "No resolved decisions yet."}</details>
  <details open><summary>Coverage and limitations</summary><ul>${run.limitations.map(l => `<li>${e(l)}</li>`).join("") || "<li>Coverage has not yet been finalized.</li>"}</ul></details>
  <p class="muted">Public QA ground truth is available only to the separate eval scorer.</p>${run.seal ? '<button id="evaluation">View eval comparison</button><div id="eval-result" hidden></div>' : ""}</section></div>`;
  byId("detail").querySelectorAll("[data-control]").forEach(button => button.onclick = () => guarded(async () => {
    await api(`/api/runs/${run.id}/${button.dataset.control}`, {revision: run.revision, request_key: crypto.randomUUID()}); await refresh();
  }));
  if (run.status === "paused") {
    const panel = document.createElement("section");
    panel.innerHTML = '<h3>Paused at gate boundary</h3><p>Already-admitted work may finish. New gates and turns remain stopped.</p>' +
      `<p>${run.gates.filter(g => g.status === "running").length} active gate(s) still settling.</p>`;
    if (!run.gates.some(g => g.status === "running") && run.limitations?.some(line => line.includes("Host app-server capacity exhausted"))) {
      panel.innerHTML += '<label>Recovery reason<textarea id="recovery-reason"></textarea></label><label><input type="checkbox" id="recovery-actor">Agent operating CUA (not human adjudication)</label><label><input type="checkbox" id="retry-source">Also retry interrupted source-only reviews (new paid attempts, previous charges retained)</label><button id="retry-unstarted">Retry selected recoverable gates</button><small>Completed gates and container experiments are never retried here. Existing charges and failed attempt evidence remain.</small>';
    }
    byId("detail").querySelector('.actions').after(panel);
    if (byId("retry-unstarted")) byId("retry-unstarted").onclick = () => guarded(async () => {
      await api(`/api/runs/${run.id}/retry-unstarted`, {revision:run.revision, request_key:crypto.randomUUID(),
        actor:byId("recovery-actor").checked ? "agent-cua" : "local-human", reason:byId("recovery-reason").value,
        include_interrupted_source:byId("retry-source").checked});
      await refresh();
    });
  }
  if (byId("reason")) {
    byId("detail").querySelector('.review > h2').textContent = presentation.title;
    if (interaction.type === "permission") {
      byId("detail").querySelector('.review .actions').innerHTML = '<button data-decision="once">Allow once</button><button data-decision="reject">Refuse</button>';
      byId("detail").querySelector('.review > small').textContent = "Responds only to this bound request. No persistent permission is granted.";
      const request = document.createElement("pre");
      request.textContent = JSON.stringify(interaction.request || {}, null, 2);
      byId("reason").parentElement.before(request);
    }
    if (interaction.type === "clarification" && presentation.supported) {
      byId("detail").querySelector('.review .actions').innerHTML = '<button data-decision="answer">Submit answers</button><button data-decision="reject">Cannot answer</button>';
      byId("detail").querySelector('.review > small').textContent = "Answers bind to this request only. Do not enter credentials or reference gold.";
      for (const question of interaction.request?.params?.questions || []) {
        const label = document.createElement("label");
        label.textContent = question.question;
        const input = document.createElement("textarea");
        input.dataset.questionId = question.id;
        input.rows = 2; input.maxLength = 10000;
        const key = draftKey(run, interaction) + ":answer:" + question.id;
        try { input.value = localStorage.getItem(key) || ""; } catch {}
        input.oninput = () => { try { localStorage.setItem(key, input.value); } catch {} };
        label.append(input);
        byId("reason").parentElement.before(label);
      }
    }
    const actorLabel = document.createElement("label");
    actorLabel.className = "check";
    actorLabel.textContent = "Agent operating CUA (not human adjudication)";
    const assurance = document.createElement("small");
    assurance.textContent = operatorToken()
      ? "Human decisions from this tab are recorded as operator-verified."
      : "Without the operator secret, a human decision is recorded as an unverified client claim.";
    const actorSelect = document.createElement("input");
    actorSelect.type = "checkbox";
    actorSelect.id = "review-actor";
    try { actorSelect.checked = localStorage.getItem("qa-review-actor") === "agent-cua"; } catch {}
    actorSelect.onchange = () => { try { localStorage.setItem("qa-review-actor", actorSelect.checked ? "agent-cua" : "local-human"); } catch {} };
    actorLabel.append(actorSelect);
    byId("reason").parentElement.before(actorLabel);
    byId("reason").parentElement.before(assurance);
    if (byId("interaction-select")) byId("interaction-select").onchange = event => {
      selectedInteraction = event.target.value; render(run);
    };
    byId("reason").dataset.interaction = interaction.id;
    byId("reason").value = draft || "";
    byId("reason").oninput = () => {
      try { localStorage.setItem(draftKey(run, interaction), byId("reason").value); }
      catch { error(new Error("Draft could not be saved locally. Copy your reason before leaving this page.")); }
    };
    byId("jump-review").onclick = () => byId("reason").focus();
    if (interaction.gate_id && !["permission", "clarification"].includes(interaction.type)) {
      byId("detail").querySelector('[data-decision="dismiss"]').textContent = "Reject gate";
      byId("detail").querySelector('.review > p').textContent = "Review this gate's evidence and decide whether it may continue. This does not dismiss individual findings.";
      const label = document.createElement("p");
      label.textContent = `Decision gate: ${interaction.gate_id}. Confirm continues this gate; dismiss rejects it; more evidence leaves it inconclusive.`;
      byId("reason").parentElement.before(label);
    }
    byId("detail").querySelector('.review > p').textContent = presentation.message;
    byId("detail").querySelectorAll("[data-decision]").forEach(button => {
      button.disabled = !presentation.decisions.includes(button.dataset.decision);
    });
  }
  byId("detail").querySelectorAll("[data-decision]").forEach(button => button.onclick = () => guarded(async () => {
    if (!interactionPresentation(interaction).decisions.includes(button.dataset.decision)) throw new Error("This interaction cannot accept that review decision. Refresh the checkpoint.");
    const reason = byId("reason").value.trim(); if (!reason) throw new Error("Add a reason before resolving this gate.");
    const buttons = [...byId("detail").querySelectorAll("[data-decision]")];
    buttons.forEach(b => b.disabled = true);
    try {
      const answers = Object.fromEntries([...byId("detail").querySelectorAll("[data-question-id]")].map(input => [input.dataset.questionId, {answers:[input.value.trim()]}]));
      await api(`/api/runs/${run.id}/${["permission", "clarification"].includes(interaction.type) ? interaction.type : "decision"}`, {revision: run.revision, request_key: crypto.randomUUID(), interaction_id: interaction.id,
        context_digest: interaction.context_digest, decision: button.dataset.decision, reason,
        ...(interaction.type === "clarification" ? {answers} : {}),
        actor: byId("review-actor").checked ? "agent-cua" : "local-human"});
      try { localStorage.removeItem(draftKey(run, interaction)); } catch {}
      await refresh();
    } finally { buttons.forEach(b => b.disabled = false); }
  }));
  byId("detail").querySelectorAll("[data-source]").forEach(button => button.onclick = () => guarded(async () => {
    const data = await api(`/api/runs/${run.id}/source?path=${encodeURIComponent(button.dataset.source)}&context=${encodeURIComponent(button.dataset.evidence || "")}`);
    byId("source-title").textContent = data.path;
    byId("source-text").textContent = data.text + (data.truncated ? "\n[Preview truncated]" : ""); byId("source-dialog").showModal();
  }));
  byId("copy-id").onclick = () => guarded(() => navigator.clipboard.writeText(run.id));
  const closeInspector = () => { if (byId("close-gate")) byId("close-gate").onclick = () => { inspectedGate = ""; byId("gate-inspector").innerHTML = ""; }; };
  closeInspector();
  byId("detail").querySelectorAll("[data-gate]").forEach(button => button.onclick = () => guarded(async () => {
    inspectedGate = button.dataset.gate;
    byId("gate-inspector").innerHTML = renderGateInspector(run, inspectedGate);
    closeInspector();
    byId("gate-inspector").scrollIntoView({block:"nearest"});
    await refreshInspector(run);
  }));
  byId("detail").querySelectorAll("[data-artifact]").forEach(button => button.onclick = () => guarded(async () => {
    const data = await api(`/api/runs/${run.id}/artifact?path=${encodeURIComponent(button.dataset.artifact)}`);
    byId("source-title").textContent = data.path;
    byId("source-text").textContent = data.text + (data.truncated ? "\n[Preview truncated]" : "");
    byId("source-dialog").showModal();
  }));
  if (byId("followup")) byId("followup").onclick = () => {
    const form = byId("create");
    form.elements.parent_id.value = run.id;
    form.elements.charter.value = run.policy.charter_id;
    form.elements.mode.value = run.mode;
    form.elements.task_goals.value = run.policy.task_goals;
    byId("parent-label").hidden = false;
    byId("parent-label").textContent = "New snapshot linked to " + run.id.slice(0,12) + ". All checks rerun.";
    form.elements.task_path.focus();
  };
  if (byId("evaluation")) byId("evaluation").onclick = () => guarded(async () => {
    const report = await api(`/api/runs/${run.id}/evaluation`);
    const pair = report?.pair_validation;
    byId("eval-result").hidden = false;
    byId("eval-result").innerHTML = report ? `<h3>Public QA comparison · ${e(report.case_id)}</h3>
      ${pair ? `<section><h4>Original / repaired validation</h4><p>Execution: ${pair.execution_passed ? "passed" : "gaps remain"} · Reference detection: ${pair.reference_detection_passed ? "full match" : "not established"}</p><table><thead><tr><th>Snapshot</th><th>First → repeat reward</th></tr></thead><tbody><tr><td>Original</td><td>${e(pair.original_repeat_rewards?.[0]?.join(" → "))}</td></tr><tr><td>Repaired</td><td>${e(pair.repaired_repeat_rewards?.[0]?.join(" → "))}</td></tr></tbody></table><small>${e(pair.note)}</small></section>` : ""}
      <p>${report.matches.filter(m => m.coverage === "full").length} fully identified · ${report.matches.filter(m => m.coverage === "partial").length} partly identified · ${report.missed_gold_ids.length} missed</p>
      ${report.clause_assessments ? `<details><summary>Identification versus reproduction</summary><p>Identifying a conditional failure is not proof that it occurred in this run.</p>${Object.entries(report.clause_assessments).map(([id, clauses]) => `<h4>${e(id)}</h4>${Object.entries(clauses).map(([index, c]) => `<p>Clause ${e(Number(index)+1)} · ${c.identified ? "identified" : "missed"} · ${e(c.evidence_level)}<br>${e(c.reason)}</p>`).join("")}`).join("")}</details>` : ""}
      <p>${report.unmatched_predictions.length} additional prediction(s) awaiting adjudication.</p>
      <p>Blocking recall: ${report.blocking_recall === null ? "not scored — independent adjudication required" : e(report.blocking_recall)}</p>
      <small>Development regression only. Additional findings are not automatically false positives.</small>
      <details><summary>Comparison receipt</summary><pre>${e(JSON.stringify(report,null,2))}</pre></details>` : "No eval receipt yet. Run the separate scorer against this sealed result.";
    const comparison = await api(`/api/runs/${run.id}/adjudication`);
    if (comparison) renderComparison(run, comparison);
  });
  if (byId("export")) byId("export").onclick = () => {
    const url = URL.createObjectURL(new Blob([JSON.stringify(run, null, 2)], {type:"application/json"}));
    const link = document.createElement("a"); link.href = url; link.download = `qa-${run.id}.json`; link.click(); URL.revokeObjectURL(url);
  };
}
function renderComparison(run, comparison) {
  const e = escapeHtml, root = byId("eval-result");
  root.innerHTML = `<h3>Post-seal review queue</h3><p>Assistant proposals until individually reviewed. Original predictions remain immutable. Public labels are provisional; primary metrics remain unscored.</p>
    <h4>Public reference defects</h4>${comparison.gold.map(g => `<p>${e(g.id)} · ${comparison.findings.some(f => f.disposition === "matched_public" && f.gold_id === g.id) ? "proposed/reviewed match" : "MISSED"}<br>${e(g.rationale)}</p>`).join("") || "<p>No labeled defects. This is not a certified clean control.</p>"}
    ${comparison.findings.map(f => `<form data-review="${e(f.prediction_id)}" class="finding"><h4>${e(run.findings.find(p => p.id === f.prediction_id)?.title)}</h4><small>${f.human_confirmed ? "Human reviewed" : "Assistant proposal"}</small>
    <label>Disposition<select name="disposition">${["matched_public","additional_supported","duplicate","unsupported","needs_evidence"].map(d => `<option ${f.disposition === d ? "selected" : ""}>${d}</option>`).join("")}</select></label>
    <label>Reference defect<select name="gold_id"><option value="">None</option>${comparison.gold.map(g => `<option value="${e(g.id)}" ${g.id === f.gold_id ? "selected" : ""}>${e(g.id)}</option>`).join("")}</select></label>
    <label>Duplicate of<select name="duplicate_of"><option value="">None</option>${run.findings.filter(p => p.id !== f.prediction_id).map(p => `<option value="${e(p.id)}" ${p.id === f.duplicate_of ? "selected" : ""}>${e(p.title)}</option>`).join("")}</select></label>
    <label>Review rationale<textarea name="reason" required>${e(f.reason)}</textarea></label><label><input name="agent_cua" type="checkbox">Agent operating CUA (not human adjudication)</label><button type="submit">Save attributed review</button></form>`).join("")}`;
  root.querySelectorAll("[data-review]").forEach(form => form.onsubmit = event => {
    event.preventDefault();
    guarded(async () => {
      const updated = await api(`/api/runs/${run.id}/adjudication`, {...Object.fromEntries(new FormData(form)), prediction_id: form.dataset.review,
        actor:form.elements.agent_cua.checked ? "agent-cua" : "local-human",
        prediction_seal: comparison.prediction_seal, revision: comparison.revision, request_key: crypto.randomUUID()});
      renderComparison(run, updated);
    });
  });
}
async function refresh() {
  if (busy) return; busy = true;
  try {
    const runs = await api("/api/runs");
    notifyWorkshop(runs);
    const pending = runs.filter(r => r.interactions.some(i => i.status === "open"));
    byId("queue-summary").textContent = `${pending.length} run(s) need review`;
    const filter = byId("run-filter").value;
    const visible = runs.filter(r => filter === "all" || (filter === "pending" ? pending.includes(r) : r.mode === "hitl"));
    const runListHtml = visible.map(r => `<button class="run" data-id="${r.id}" aria-pressed="${r.id === selected}">${escapeHtml(r.policy.charter.name)} · ${r.id.slice(0,8)}<span>${escapeHtml(r.status)} · ${escapeHtml(r.mode)}</span></button>`).join("") || '<p class="muted">No runs in this queue.</p>';
    // Keep unchanged buttons attached while a person or CUA is targeting them.
    if (byId("runs").innerHTML !== runListHtml) byId("runs").innerHTML = runListHtml;
    byId("runs").querySelectorAll("[data-id]").forEach(b => b.onclick = () => guarded(() => select(b.dataset.id)));
    if (!selected && visible.length) { selected = visible[0].id; location.hash = selected; }
    const run = runs.find(r => r.id === selected);
    if (run && run.revision !== lastRevision) { render(run); lastRevision = run.revision; }
    if (run) await refreshInspector(run);
    if (!run) { byId("detail").textContent = selected ? "Selected run is not available in this store." : "Select a run or submit a task."; lastRevision = -1; }
  } finally { busy = false; }
}
byId("close-source").onclick = () => byId("source-dialog").close();
byId("run-filter").onchange = () => guarded(refresh);
byId("create").elements.profile_id.onchange = () => {
  const form = byId("create");
  const legacy = form.elements.profile_id.value === "legacy";
  form.elements.budget_usd.disabled = legacy;
  form.elements.budget_usd.value = legacy ? "0" : "1";
  // A profile pins its own mode. Show the real one rather than letting the form
  // imply a choice the server would refuse.
  const option = form.elements.profile_id.selectedOptions[0];
  if (!legacy && option?.dataset.mode) form.elements.mode.value = option.dataset.mode;
  form.elements.mode.disabled = !legacy;
};
window.addEventListener("hashchange", () => { selected = location.hash.slice(1); lastRevision = -1; guarded(refresh); });
if (query.get("mode") === "hitl") byId("create").elements.mode.value = "hitl";
guarded(async () => { const config = await api("/api/config"); byId("roots").textContent = "Allowed roots: " + config.task_roots.join(", ");
  byId("allowance").textContent = config.allowance ? `Authorized service allowance remaining: $${(config.allowance.maximum-config.allowance.committed).toFixed(2)}` : "Full QA needs an operator-authorized provider allowance. Legacy baseline remains available.";
  const codexReady = config.ai_runtime === "codex-app-server" && config.provider_calls_enabled;
  const profileSelect = byId("create").elements.profile_id;
  for (const profile of config.profiles || []) {
    const option = document.createElement("option");
    option.value = profile.id;
    option.textContent = `${profile.id} · v${profile.version} · ${profile.description}`;
    option.dataset.mode = profile.mode;
    option.disabled = !codexReady;
    profileSelect.insertBefore(option, profileSelect.firstChild);
  }
  if (codexReady) profileSelect.value = chooseLaunchProfile(config.profiles || [], query.get("mode"));
  if (!codexReady) {
    profileSelect.value = "legacy";
    byId("allowance").textContent = config.ai_runtime_disabled_reason
      ? `Codex AI dispatch is unavailable: ${config.ai_runtime_disabled_reason}`
      : "Codex AI dispatch is not available. Only the rules-only baseline can launch here; no direct-provider fallback.";
  }
  profileSelect.onchange();
  const operatorField = byId("operator");
  operatorField.hidden = !config.operator_token_required;
  if (config.operator_token_required) {
    const input = operatorField.querySelector("input");
    input.value = operatorToken();
    input.oninput = () => { try { sessionStorage.setItem("qa-operator-token", input.value.trim()); } catch {} };
  }
  await refresh(); });
setInterval(() => guarded(refresh), 1500);
