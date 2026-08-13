(() => {
  "use strict";

  const state = {
    events: [],
    keys: new Map(),
    sources: new Map(),
    selectedLane: null,
    cursor: -1,
    followLive: true,
    playing: false,
    playTimer: null,
    framePlaying: false,
    frameTimer: null,
    traceMode: "full",
    arrival: 0,
    selectedTraceKey: null,
    smoke: { base: null, rolloutId: null }
  };
  const requestedRollouts = new Set(new URL(window.location.href).searchParams.getAll("rollout").filter(Boolean));

  const $ = (id) => document.getElementById(id);
  const elements = {
    streamForm: $("stream-form"), streamInput: $("stream-input"), sourceMessage: $("source-message"),
    disconnect: $("disconnect-button"), dot: $("connection-dot"), connection: $("connection-label"),
    laneList: $("lane-list"), selectedRollout: $("selected-rollout"), selectedStatus: $("selected-status"),
    frame: $("game-frame"), frameEmpty: $("frame-empty"), frameStep: $("frame-step"), frameTime: $("frame-time"),
    framePlay: $("frame-play"), frameSpeed: $("frame-speed"), frameTimeline: $("frame-timeline"), frameCount: $("frame-count"),
    timeline: $("timeline"), timelineLabel: $("timeline-label"), timelineStart: $("timeline-start"), timelineEnd: $("timeline-end"),
    play: $("play-button"), live: $("live-button"), speed: $("speed-select"),
    activity: $("activity-list"), eventCount: $("event-count"),
    sourcePanel: $("source-panel"), createRollout: $("create-rollout"), stepRollout: $("step-rollout"),
    base: $("container-base"), seed: $("rollout-seed"), action: $("manual-action")
  };

  function object(value) {
    return value && typeof value === "object" && !Array.isArray(value) ? value : {};
  }

  function finite(...values) {
    for (const value of values) {
      const number = typeof value === "string" && value.trim() ? Number(value) : value;
      if (typeof number === "number" && Number.isFinite(number)) return number;
    }
    return undefined;
  }

  function text(...values) {
    for (const value of values) if (typeof value === "string" && value.trim()) return value;
    return undefined;
  }

  function timestampMs(value, fallback) {
    const parsed = typeof value === "number" ? value : Date.parse(value || "");
    return Number.isFinite(parsed) ? parsed : fallback;
  }

  function achievementNames(value) {
    if (Array.isArray(value)) return value.map(String);
    if (value && typeof value === "object") {
      return Object.entries(value).filter(([, unlocked]) => Boolean(unlocked)).map(([name]) => name);
    }
    return [];
  }

  function absoluteUrl(candidate, streamUrl) {
    if (!candidate) return undefined;
    try { return new URL(candidate, streamUrl).toString(); }
    catch { return undefined; }
  }

  function actionsFromReply(value) {
    if (typeof value !== "string" || !value) return [];
    const match = value.match(/ACTIONS?\s*:\s*(\[[\s\S]*?\])/i);
    if (!match) return [];
    try {
      const parsed = JSON.parse(match[1]);
      return Array.isArray(parsed) ? parsed.map(String) : [];
    } catch { return []; }
  }

  function normalize(raw, streamUrl, eventType) {
    const payload = object(raw.payload);
    const readout = object(payload.readout);
    const observation = Object.keys(object(readout.observation)).length ? object(readout.observation) : readout;
    const progress = object(payload.progress);
    const service = object(payload.service);
    const provenance = object(payload.source_provenance);
    const policy = object(payload.policy);
    const usage = object(payload.usage);
    const vitals = object(payload.vitals);
    const inventory = Object.keys(object(payload.inventory)).length ? object(payload.inventory) : object(observation.inventory);
    const resources = object(payload.resources);
    const gear = object(payload.gear);
    const stats = object(payload.stats);
    const callActions = Array.isArray(payload.actions) ? payload.actions.map(String) : actionsFromReply(payload.reply);
    const arrival = ++state.arrival;
    const rolloutId = text(raw.rollout_id, raw.rolloutId, payload.rollout_id, payload.rolloutId, raw.run_id, raw.runId, raw.lane) || "eval";
    const runId = text(raw.run_id, raw.runId, payload.run_id, payload.runId) || rolloutId;
    const cursor = raw.sequence ?? raw.event_id ?? raw.eventId ?? raw.seq ?? payload.sequence ?? progress.env_steps ?? progress.done;
    const sequence = finite(cursor);
    const occurredAt = text(raw.occurred_at, raw.occurredAt, raw.ts, payload.occurred_at, payload.ts);
    const timeMs = timestampMs(occurredAt, Date.now() + arrival / 1000);
    const kind = text(raw.kind, eventType, payload.kind) || "message";
    const carriesFrame = kind === "frame" || kind === "snapshot" || kind === "observation.frame";
    const achievements = achievementNames(payload.achievements ?? observation.achievements);
    const policyUsage = Object.keys(usage).length ? usage : object(policy.usage);
    const priorAttempts = Array.isArray(payload.prior_attempts) ? payload.prior_attempts : [];
    const attemptUsage = [policyUsage, ...priorAttempts.map((attempt) => object(object(attempt).usage))];
    const policyTokens = attemptUsage.reduce((sum, item) => sum + (finite(item.total_tokens) ?? 0), 0);
    const policyCost = attemptUsage.reduce((sum, item) => sum + (finite(item.cost_usd) ?? 0), 0);

    return {
      raw, streamUrl, arrival, runId, rolloutId,
      lane: text(raw.lane, payload.lane) || rolloutId,
      cursor: cursor == null ? String(arrival) : String(cursor), sequence, timeMs, occurredAt: occurredAt || new Date(timeMs).toISOString(), kind,
      step: finite(progress.done, progress.env_steps, payload.step_index, payload.env_steps, payload.step, payload.steps, readout.env_steps, raw.step) ?? 0,
      total: finite(progress.total, payload.max_steps),
      reward: finite(payload.total_reward, payload.reward, readout.private?.total_reward, raw.reward),
      rewardDelta: finite(payload.reward_delta, payload.step_reward, kind === "reward_signal" ? payload.value : undefined),
      achievements,
      runtime: text(provenance.name, service.lane === "rust" && service.env_family === "craftax-singleplayer" ? "GameBench symbolic Rust Gold" : undefined, readout.private ? "GameBench Craftax Rust Gold" : undefined),
      frameUrl: carriesFrame
        ? absoluteUrl(text(payload.frame_url, payload.frameUrl, payload.url, raw.frame_url), streamUrl)
        : undefined,
      terminated: Boolean(payload.terminated) || Boolean(readout.private?.terminated) || payload.status === "completed" || kind === "eval.run.terminal" || kind === "run_finished",
      truncated: Boolean(payload.truncated) || Boolean(readout.private?.truncated),
      error: text(payload.error, raw.error),
      action: text(payload.action, payload.selected_action, payload.chosen_action, raw.action, callActions[0]),
      actionPlan: callActions,
      observation: text(payload.observation_summary, observation.summary, payload.observation, readout.observation_text),
      ascii: text(payload.ascii, payload.grid, readout.ascii),
      inventory,
      resources,
      gear,
      stats,
      vitals: {
        health: finite(payload.health, vitals.health, inventory.health),
        food: finite(payload.food, vitals.food, inventory.food),
        drink: finite(payload.drink, vitals.drink, inventory.drink),
        energy: finite(payload.energy, vitals.energy, inventory.energy)
      },
      policy: {
        provider: text(policy.provider, payload.provider, raw.provider),
        model: text(policy.model, payload.model, raw.model),
        effort: text(policy.effort, policy.reasoning_effort, payload.reasoning_effort),
        call: finite(policy.call, payload.call, payload.policy_call_index),
        latencyMs: finite(policy.latency_ms, payload.latency_ms),
        tokens: policyTokens || finite(payload.tokens, payload.total_tokens),
        costUsd: policyCost || finite(payload.cost_usd, payload.estimated_usd, raw.cost_usd)
      }
    };
  }

  function eventKey(event) {
    return [event.runId, event.rolloutId, event.cursor, event.kind].join("|");
  }

  function orderedEvents() {
    return state.events.filter((event) => !requestedRollouts.size || requestedRollouts.has(event.rolloutId)).sort((a, b) =>
      a.timeMs - b.timeMs ||
      String(a.rolloutId).localeCompare(String(b.rolloutId)) ||
      (a.sequence ?? a.arrival) - (b.sequence ?? b.arrival) ||
      a.arrival - b.arrival
    );
  }

  function laneOrdered(events) {
    return [...events].sort((a, b) => {
      if (Number.isFinite(a.sequence) && Number.isFinite(b.sequence) && a.sequence !== b.sequence) return a.sequence - b.sequence;
      return a.timeMs - b.timeMs || a.arrival - b.arrival;
    });
  }

  function ingest(raw, streamUrl, eventType) {
    const event = normalize(raw, streamUrl, eventType);
    if (requestedRollouts.size && !requestedRollouts.has(event.rolloutId)) return;
    const key = eventKey(event);
    const digest = JSON.stringify(raw);
    const priorDigest = state.keys.get(key);
    if (priorDigest === digest) return;
    if (priorDigest !== undefined) {
      setConnectionState("error", "Conflicting replay refused");
      elements.sourceMessage.textContent = "Stream conflict: " + key + " was replayed with different data.";
      return;
    }
    state.keys.set(key, digest);
    state.events.push(event);
    if (!state.selectedLane) state.selectedLane = event.rolloutId;
    const ordered = orderedEvents();
    if (state.followLive) state.cursor = ordered.length - 1;
    render();
  }

  function splitSources(value) {
    return [...new Set(value.split(/[\n,]+/).map((item) => item.trim()).filter(Boolean))];
  }

  function setConnectionState(kind, label) {
    elements.dot.className = "status-dot" + (kind ? " " + kind : "");
    elements.connection.textContent = label;
  }

  const namedEvents = [
    "snapshot", "eval.phase", "eval.run.terminal", "run_finished", "error",
    "rollout.snapshot", "rollout.policy", "rollout.reward", "rollout.achievement",
    "eval.policy_model.response", "trace.sealed"
  ];

  function connect(url) {
    let parsed;
    try { parsed = new URL(url); }
    catch {
      elements.sourceMessage.textContent = "Invalid URL: " + url;
      return;
    }
    if (!/^https?:$/.test(parsed.protocol)) {
      elements.sourceMessage.textContent = "Only HTTP(S) SSE sources are supported.";
      return;
    }
    if (state.sources.has(parsed.toString())) return;

    const source = new EventSource(parsed.toString());
    const record = { source, status: "connecting", opened: false, reconnects: 0 };
    state.sources.set(parsed.toString(), record);
    setConnectionState("connecting", "Connecting");

    const receive = (message) => {
      try { ingest(JSON.parse(message.data), parsed.toString(), message.type); }
      catch (error) { elements.sourceMessage.textContent = "Malformed event shown as stream error: " + error.message; }
    };
    source.onmessage = receive;
    for (const kind of namedEvents) source.addEventListener(kind, receive);
    source.onopen = () => {
      const recovered = record.opened && record.reconnects > 0;
      record.opened = true;
      record.status = "connected";
      elements.sourceMessage.textContent = recovered
        ? "Connection restored; replayed envelopes were collapsed by identity."
        : state.sources.size + " real stream source" + (state.sources.size === 1 ? "" : "s") + " attached.";
      elements.sourcePanel.open = false;
      updateConnectionSummary();
    };
    source.onerror = () => {
      record.reconnects += 1;
      record.status = "reconnecting";
      elements.sourceMessage.textContent = "Stream interrupted; reconnecting with durable replay enabled.";
      updateConnectionSummary();
    };
  }

  function updateConnectionSummary() {
    const statuses = [...state.sources.values()].map((item) => item.status);
    const connected = statuses.filter((status) => status === "connected").length;
    if (!statuses.length) setConnectionState("", "Not connected");
    else if (connected === statuses.length) setConnectionState("connected", connected + " stream" + (connected === 1 ? "" : "s") + " live");
    else if (connected) setConnectionState("connecting", connected + "/" + statuses.length + " streams live");
    else setConnectionState("error", "Reconnecting");
  }

  function disconnectAll() {
    for (const { source } of state.sources.values()) source.close();
    state.sources.clear();
    updateConnectionSummary();
    elements.sourceMessage.textContent = "Disconnected. Captured events remain available for replay.";
  }

  function fmtTime(ms, full = false) {
    if (!Number.isFinite(ms)) return "—";
    return new Date(ms).toLocaleTimeString([], full
      ? { hour: "2-digit", minute: "2-digit", second: "2-digit", fractionalSecondDigits: 3 }
      : { hour: "2-digit", minute: "2-digit", second: "2-digit" });
  }

  function fmtNumber(value, digits = 2) {
    return Number.isFinite(value) ? Number(value).toFixed(digits) : "—";
  }

  function semanticCheckpointIndexes(events) {
    if (!events.length) return [];
    const significant = new Set([
      "trace.opened", "trace.reconciled", "capture.closed", "status",
      "env.episode.opened", "env.episode.closed", "terminal", "episode_truncated",
      "span.policy.opened", "span.policy.closed", "span.step.closed", "achievement_unlocked"
    ]);
    const indexes = [];
    for (let index = 0; index < events.length; index++) {
      if (index === 0 || index === events.length - 1 || significant.has(events[index].kind)) indexes.push(index);
    }
    return [...new Set(indexes)];
  }

  function checkpointPosition(checkpoints, cursor) {
    if (!checkpoints.length) return 0;
    let position = 0;
    while (position + 1 < checkpoints.length && checkpoints[position + 1] <= cursor) position += 1;
    return position;
  }

  function laneEventsAt(visible, lane) {
    return laneOrdered(visible.filter((event) => event.rolloutId === lane));
  }

  function latestByLane(visible) {
    const map = new Map();
    for (const event of visible) map.set(event.rolloutId, event);
    return map;
  }

  function lastDefined(events, accessor) {
    for (let index = events.length - 1; index >= 0; index--) {
      const value = accessor(events[index]);
      if (value !== undefined && value !== null && value !== "") return value;
    }
    return undefined;
  }

  function unionAchievements(events) {
    const names = new Set();
    for (const event of events) for (const name of event.achievements) names.add(name);
    return [...names];
  }

  function rolloutState(events) {
    return {
      step: Math.max(0, ...events.map((event) => event.step).filter(Number.isFinite)),
      terminated: events.some((event) => event.terminated),
      truncated: events.some((event) => event.truncated),
      error: lastDefined(events, (event) => event.error),
    };
  }

  function displayInventory(inventory) {
    const entries = [];
    const visit = (value, path, depth) => {
      if (entries.length >= 24 || depth > 2) return;
      if (Array.isArray(value)) {
        const meaningful = value.filter((item) => item !== 0 && item !== "none" && item !== null && item !== "");
        if (meaningful.length) entries.push([path, meaningful.join(", ")]);
        return;
      }
      if (value && typeof value === "object") {
        for (const [key, child] of Object.entries(value)) visit(child, path ? path + "." + key : key, depth + 1);
        return;
      }
      if (value === 0 || value === false || value === "none" || value === null || value === "") return;
      entries.push([path, String(value)]);
    };
    for (const [key, value] of Object.entries(inventory)) {
      if (["health", "food", "drink", "energy"].includes(key)) continue;
      visit(value, key, 0);
    }
    return entries;
  }

  function carriedFromInventory(inventory) {
    const names = [
      "wood", "stone", "coal", "iron", "diamond", "sapphire", "ruby", "sapling",
      "pickaxe", "sword", "bow", "arrows", "torches", "books", "armour", "potions"
    ];
    return Object.fromEntries(names.filter((name) => inventory[name] !== undefined).map((name) => [name, inventory[name]]));
  }

  function renderLanes(visible) {
    const lanes = latestByLane(visible);
    elements.laneList.replaceChildren();
    for (const [id, latest] of lanes) {
      const history = laneEventsAt(visible, id);
      const rollout = rolloutState(history);
      const reward = lastDefined(history, (event) => event.reward);
      const achievements = unionAchievements(history);
      const button = document.createElement("button");
      button.type = "button";
      button.setAttribute("aria-current", String(state.selectedLane === id));
      button.setAttribute("aria-label", "Select rollout " + id + ", step " + rollout.step);
      button.innerHTML =
        '<span class="lane-top"><span class="lane-name"></span><span class="lane-state"></span></span>' +
        '<span class="lane-bottom"></span>';
      button.querySelector(".lane-name").textContent = id;
      button.querySelector(".lane-state").textContent = rollout.error ? "failed" : rollout.terminated ? "finished" : "running";
      button.querySelector(".lane-bottom").textContent = "step " + rollout.step + " · reward " + fmtNumber(reward) + " · " + achievements.length + " achievements";
      button.addEventListener("click", () => { stopFrameReplay(); state.selectedLane = id; render(); });
      elements.laneList.append(button);
    }
  }

  function setText(id, value) { $(id).textContent = value; }

  function renderDetails(history) {
    const latest = history.at(-1);
    if (!latest) {
      elements.selectedRollout.textContent = "Waiting for events";
      elements.frame.hidden = true;
      $("ascii-frame").hidden = true;
      elements.frameEmpty.hidden = false;
      return;
    }

    const reward = lastDefined(history, (event) => event.reward);
    const rollout = rolloutState(history);
    const achievements = unionAchievements(history);
    const frameUrl = lastDefined(history, (event) => event.frameUrl);
    const allFrames = orderedEvents().filter((event) => event.rolloutId === state.selectedLane && event.frameUrl);
    const visibleFrame = [...history].reverse().find((event) => event.frameUrl);
    const frameIndex = Math.max(0, allFrames.indexOf(visibleFrame));
    const ascii = lastDefined(history, (event) => event.ascii);
    const provider = lastDefined(history, (event) => event.policy.provider);
    const model = lastDefined(history, (event) => event.policy.model);
    const effort = lastDefined(history, (event) => event.policy.effort);
    const action = lastDefined(history, (event) => event.action);
    const call = lastDefined(history, (event) => event.policy.call);
    const latency = lastDefined(history, (event) => event.policy.latencyMs);
    const tokens = lastDefined(history, (event) => event.policy.tokens);
    const costValues = history.map((event) => event.policy.costUsd).filter(Number.isFinite);
    const cost = costValues.length ? costValues.reduce((sum, value) => sum + value, 0) : undefined;
    const runtime = lastDefined(history, (event) => event.runtime);
    const inventory = lastDefined(history, (event) => Object.keys(event.inventory).length ? event.inventory : undefined) || {};
    const resources = lastDefined(history, (event) => Object.keys(event.resources).length ? event.resources : undefined) || {};
    const gear = lastDefined(history, (event) => Object.keys(event.gear).length ? event.gear : undefined) || {};
    const stats = lastDefined(history, (event) => Object.keys(event.stats).length ? event.stats : undefined) || {};
    const vitals = lastDefined(history, (event) => Object.values(event.vitals).some(Number.isFinite) ? event.vitals : undefined) || {};

    elements.selectedRollout.textContent = latest.rolloutId;
    elements.selectedStatus.textContent = rollout.error ? "failed" : rollout.terminated ? (rollout.truncated ? "truncated" : "finished") : "running";
    elements.frameStep.textContent = "step " + rollout.step;
    elements.frameTime.textContent = fmtTime(latest.timeMs, true);
    elements.frameCount.textContent = allFrames.length + " PNG frames from Containers";
    elements.framePlay.disabled = !allFrames.length;
    elements.frameTimeline.disabled = !allFrames.length;
    elements.frameTimeline.max = String(Math.max(0, allFrames.length - 1));
    elements.frameTimeline.value = String(frameIndex);
    if (frameUrl) {
      if (elements.frame.src !== frameUrl) elements.frame.src = frameUrl;
      elements.frame.hidden = false;
      $("ascii-frame").hidden = true;
      elements.frameEmpty.hidden = true;
    } else if (ascii) {
      elements.frame.hidden = true;
      $("ascii-frame").textContent = ascii;
      $("ascii-frame").hidden = false;
      elements.frameEmpty.hidden = true;
    } else {
      elements.frame.hidden = true;
      $("ascii-frame").hidden = true;
      elements.frameEmpty.hidden = false;
    }

    setText("policy-model", model || "Unavailable");
    setText("policy-effort", effort || "");
    setText("policy-provider", provider || "—");
    setText("policy-call", Number.isFinite(call) ? String(call) : "—");
    setText("policy-action", action || "not emitted");
    setText("policy-latency", Number.isFinite(latency) ? fmtNumber(latency, 0) + " ms" : "—");
    setText("policy-tokens", Number.isFinite(tokens) ? String(tokens) : "—");
    $("policy-note").hidden = Boolean(model || provider || action);

    $("vitals").replaceChildren();
    for (const [name, value] of Object.entries(vitals)) {
      if (!Number.isFinite(value)) continue;
      const row = document.createElement("div");
      const dt = document.createElement("dt"); dt.textContent = name;
      const dd = document.createElement("dd"); dd.textContent = String(value);
      row.append(dt, dd); $("vitals").append(row);
    }
    if (!$("vitals").children.length) $("vitals").innerHTML = '<p class="muted">No vitals emitted</p>';

    $("inventory").replaceChildren();
    const carried = Object.keys(resources).length || Object.keys(gear).length ? { ...resources, ...gear } : carriedFromInventory(inventory);
    for (const [name, value] of displayInventory(carried)) {
      const token = document.createElement("span"); token.textContent = name + " " + value; $("inventory").append(token);
    }
    if (!$("inventory").children.length) $("inventory").innerHTML = '<span class="muted">No carried resources</span>';

    $("attributes").replaceChildren();
    const effectiveStats = Object.keys(stats).length ? stats : Object.fromEntries(
      ["strength", "dexterity", "intelligence", "xp"].filter((name) => inventory[name] !== undefined).map((name) => [name, inventory[name]])
    );
    const changedStats = Object.entries(effectiveStats).filter(([, value]) => Number(value) > 1);
    for (const [name, value] of changedStats) {
      const token = document.createElement("span"); token.textContent = name + " " + value; $("attributes").append(token);
    }
    $("attributes-block").hidden = !changedStats.length;

    $("achievement-list").replaceChildren();
    for (const name of achievements) {
      const token = document.createElement("span"); token.textContent = name; $("achievement-list").append(token);
    }
    if (!achievements.length) $("achievement-list").innerHTML = '<span class="muted">None yet</span>';

    setText("metric-step", String(rollout.step));
    setText("metric-reward", fmtNumber(reward));
    setText("metric-achievements", String(achievements.length));
    setText("metric-cost", Number.isFinite(cost) ? "$" + fmtNumber(cost, 6) : "unavailable");
    setText("metric-runtime", runtime === "gamebench-craftax-singleplayer-gold-rust" ? "GameBench symbolic Rust Gold" : (runtime || "unreported"));
    setText("reward-value", fmtNumber(reward));
    setText("achievement-value", String(achievements.length));
  }

  function chart(svg, points, accessor, className) {
    svg.replaceChildren();
    const width = 640, height = 190, left = 48, right = 16, top = 14, bottom = 30;
    const xValues = points.map((point) => point.step);
    const yValues = points.map(accessor).filter(Number.isFinite);
    const xMin = points.length ? Math.min(...xValues, 0) : 0;
    const xMax = points.length ? Math.max(...xValues, 1) : 1;
    const yMinRaw = yValues.length ? Math.min(...yValues) : 0;
    const yMaxRaw = yValues.length ? Math.max(...yValues) : 0;
    const yPad = yMaxRaw === yMinRaw ? Math.max(Math.abs(yMaxRaw) * .1, .5) : Math.max((yMaxRaw - yMinRaw) * .08, .1);
    const yMin = Math.min(0, yMinRaw - yPad);
    const yMax = Math.max(yMin + 1, yMaxRaw + yPad);
    const x = (value) => left + (value - xMin) / Math.max(1, xMax - xMin) * (width - left - right);
    const y = (value) => top + (yMax - value) / Math.max(.0001, yMax - yMin) * (height - top - bottom);
    const ns = "http://www.w3.org/2000/svg";

    const frame = document.createElementNS(ns, "rect");
    frame.setAttribute("x", left); frame.setAttribute("y", top); frame.setAttribute("width", width-left-right); frame.setAttribute("height", height-top-bottom); frame.setAttribute("class", "chart-frame");
    svg.append(frame);

    if (!points.length || !yValues.length) {
      const waiting = document.createElementNS(ns, "text");
      waiting.setAttribute("x", width / 2); waiting.setAttribute("y", height / 2);
      waiting.setAttribute("text-anchor", "middle"); waiting.setAttribute("class", "chart-label");
      waiting.textContent = "Waiting for real events"; svg.append(waiting); return;
    }

    const labels = [
      { x: left, y: height - 9, anchor: "start", value: "step " + xMin },
      { x: width-right, y: height - 9, anchor: "end", value: "step " + xMax },
      { x: left-7, y: y(yMax)+4, anchor: "end", value: fmtNumber(yMax, yMax < 10 ? 1 : 0) },
      { x: left-7, y: y(yMin)+4, anchor: "end", value: fmtNumber(yMin, yMin < 10 ? 1 : 0) }
    ];
    for (const label of labels) {
      const node = document.createElementNS(ns, "text");
      node.setAttribute("x", label.x); node.setAttribute("y", label.y); node.setAttribute("text-anchor", label.anchor); node.setAttribute("class", "chart-label"); node.textContent = label.value; svg.append(node);
    }
    const pathData = points.map((point, index) => (index ? "L" : "M") + x(point.step).toFixed(2) + "," + y(accessor(point)).toFixed(2)).join(" ");
    const path = document.createElementNS(ns, "path");
    path.setAttribute("d", pathData); path.setAttribute("class", "chart-line " + className); svg.append(path);
    const last = points.at(-1);
    const dot = document.createElementNS(ns, "circle");
    dot.setAttribute("cx", x(last.step)); dot.setAttribute("cy", y(accessor(last))); dot.setAttribute("r", 4); dot.setAttribute("class", "chart-dot"); svg.append(dot);
  }

  function renderCharts(history) {
    const rewardPoints = [];
    const achievementPoints = [];
    for (let index = 0; index < history.length; index++) {
      const slice = history.slice(0, index + 1);
      const reward = lastDefined(slice, (event) => event.reward);
      if (Number.isFinite(reward)) rewardPoints.push({ step: history[index].step, value: reward });
      const count = unionAchievements(slice).length;
      achievementPoints.push({ step: history[index].step, value: count });
    }
    chart($("reward-chart"), rewardPoints, (point) => point.value, "");
    chart($("achievement-chart"), achievementPoints, (point) => point.value, "secondary");
  }

  function eventDetail(event) {
    if (event.error) return event.error;
    if (event.action) return "action " + event.action;
    if (event.achievements.length) return event.achievements.length + " achievements";
    if (Number.isFinite(event.reward)) return "reward " + fmtNumber(event.reward) + " · step " + event.step;
    return "step " + event.step;
  }

  function renderActivity(visible) {
    const selected = state.selectedLane ? visible.filter((event) => event.rolloutId === state.selectedLane) : visible;
    const recent = semanticTrace(selected).slice(-10).reverse();
    elements.activity.replaceChildren();
    for (const item of recent) {
      const event = item.last;
      const row = document.createElement("li");
      for (const [className, value] of [
        ["event-time", fmtTime(event.timeMs)],
        ["event-lane", event.rolloutId],
        ["event-kind", item.kind],
        ["event-detail", item.summary]
      ]) {
        const span = document.createElement("span"); span.className = className; span.textContent = value; row.append(span);
      }
      elements.activity.append(row);
    }
    elements.eventCount.textContent = recent.length + " semantic · " + selected.length + " raw";
  }

  function traceCategory(event) {
    const subtype = text(event.raw?.payload?.kind, event.kind) || "event";
    if (subtype === "policy.call" || subtype.includes("model") || subtype.includes("policy")) return "policy";
    if (event.kind.includes("terminal") || event.kind === "eval.phase") return "lifecycle";
    if (event.kind.startsWith("trace.")) return "evidence";
    if (subtype.toLowerCase().includes("reward") || subtype.toLowerCase().includes("achievement")) return "evidence";
    return "environment";
  }

  function traceSummary(event) {
    const payload = object(event.raw.payload);
    const subtype = text(payload.kind, event.kind) || "event";
    if (subtype === "policy.call" || event.kind.includes("policy")) {
      const actions = event.actionPlan.length ? " · " + event.actionPlan.join(" → ") : "";
      return (event.policy.model || "model") + " · call " + (event.policy.call ?? "—") + actions;
    }
    if (event.action) return event.action + " · step " + event.step;
    if (event.kind.startsWith("trace.")) return text(payload.capture_id, payload.trace_digest) || event.kind;
    return eventDetail(event);
  }

  function safeStructuralPayload(event) {
    const payload = object(event.raw.payload);
    const readout = object(payload.readout);
    const usage = object(payload.usage);
    const projection = {
      kind: text(payload.kind, event.kind),
      rollout_id: event.rolloutId,
      step: event.step,
      reward: event.reward,
      reward_delta: event.rewardDelta,
      achievements: event.achievements,
      action: event.action,
      action_plan: event.actionPlan,
      policy: {
        provider: event.policy.provider,
        model: event.policy.model,
        effort: event.policy.effort,
        call: event.policy.call,
        latency_ms: event.policy.latencyMs,
        tokens: event.policy.tokens,
        cost_usd: event.policy.costUsd
      },
      usage,
      progress: object(payload.progress),
      evidence: {
        capture_id: payload.capture_id,
        trace_id: payload.trace_id,
        trace_digest: payload.trace_digest,
        cursor: payload.cursor
      },
      environment: {
        schema: readout.schema,
        task_id: readout.task_id,
        grid_hash: readout.grid_hash,
        frame_url: payload.frame_url
      },
      error: text(payload.error, event.raw.error)
    };
    return JSON.parse(JSON.stringify(projection));
  }

  function traceValue(value) {
    if (value === undefined || value === null || value === "") return "not emitted";
    if (typeof value === "string") return value || "not emitted";
    try { return JSON.stringify(value, null, 2); }
    catch { return String(value); }
  }

  function traceInteraction(event, laneEvents) {
    const payload = object(event.raw.payload);
    const index = laneEvents.indexOf(event);
    const prior = index < 0 ? [] : laneEvents.slice(0, index + 1);
    const observation = [...prior].reverse().find((candidate) => candidate.kind === "observation");
    const observationPayload = object(observation?.raw?.payload);
    const channel = text(payload.channel);
    const input = payload.input ?? payload.prompt ?? payload.request ?? payload.messages ??
      (event.kind.startsWith("span.policy.")
        ? object(observationPayload.readout).observation_text ?? observationPayload.readout ?? observationPayload.grid
        : event.kind === "observation" ? payload.readout ?? payload.grid : undefined);
    const thinking = payload.reasoning ?? payload.thinking ??
      (channel === "reasoning" ? payload.text : undefined);
    const output = payload.assistant ?? payload.output ?? payload.response ?? payload.reply ?? payload.content ??
      (Array.isArray(payload.actions) ? { actions: payload.actions } : undefined) ??
      (channel === "content" ? payload.text : undefined);
    const tools = payload.tool_calls ?? payload.tools ?? payload.tool_arguments ??
      (channel === "tool" ? payload.text : undefined);
    return { input, thinking, output, tools };
  }

  function semanticTrace(events) {
    const items = [];
    let call = null;
    let ordinal = 0;
    const flush = () => {
      if (!call) return;
      const snapshots = call.events.filter((event) => event.kind === "span.policy.data" && event.raw?.payload?.delta !== true && event.raw?.payload?.channel !== "compact");
      const snapshot = object(snapshots.at(-1)?.raw?.payload);
      const channel = (name) => call.events
        .filter((event) => event.kind === "span.policy.data" && event.raw?.payload?.delta === true && event.raw?.payload?.channel === name)
        .map((event) => String(event.raw.payload.text || "")).join("");
      const opened = object(call.events[0]?.raw?.payload?.call);
      const plan = call.events.find((event) => event.kind === "span.policy.plan");
      const actions = Array.isArray(plan?.raw?.payload?.actions) ? plan.raw.payload.actions.map(String) : Array.isArray(snapshot.actions) ? snapshot.actions.map(String) : [];
      const tools = snapshot.tool_calls ?? snapshot.tool_arguments ?? (channel("tool") || undefined);
      const output = snapshot.assistant ?? snapshot.output ?? (channel("content") || undefined);
      const last = call.events.at(-1);
      items.push({
        key: `policy:${call.ordinal}:${call.events[0].cursor}`,
        kind: "policy.call", category: "policy", first: call.events[0], last,
        rawEvents: call.events,
        summary: `${snapshot.model || opened.model || "model"} · call ${snapshot.call || call.ordinal}${actions.length ? " · " + actions.join(" → ") : ""}`,
        interaction: {
          ...traceInteraction(last, events),
          thinking: snapshot.reasoning ?? snapshot.thinking ?? (channel("reasoning") || undefined),
          output,
          tools,
          responseType: tools && !output ? "tool_call" : output ? "text" : last?.kind === "span.policy.closed" ? "not_applicable" : "pending"
        }
      });
      call = null;
    };
    for (const event of events) {
      if (event.kind === "span.policy.opened") { flush(); call = { ordinal: ++ordinal, events: [event] }; continue; }
      if (event.kind.startsWith("span.policy.")) {
        if (!call) call = { ordinal: ++ordinal, events: [] };
        call.events.push(event);
        if (event.kind === "span.policy.closed") flush();
        continue;
      }
      if (event.kind === "span.step.closed") {
        items.push({ key: `step:${event.cursor}`, kind: "environment.step", category: "environment", first: event, last: event, rawEvents: [event], summary: `Step ${event.step} · ${event.action || "action unavailable"}` });
      } else if (["trace.opened", "trace.reconciled", "capture.closed", "status", "env.episode.opened", "env.episode.closed", "terminal", "episode_truncated"].includes(event.kind)) {
        items.push({ key: `${event.kind}:${event.cursor}`, kind: event.kind, category: event.kind.startsWith("trace.") || event.kind === "capture.closed" ? "evidence" : "lifecycle", first: event, last: event, rawEvents: [event], summary: traceSummary(event) || event.kind });
      } else if (event.kind === "achievement_unlocked") {
        items.push({ key: `achievement:${event.cursor}`, kind: event.kind, category: "achievement", first: event, last: event, rawEvents: [event], summary: traceSummary(event) });
      }
    }
    flush();
    return items;
  }

  function renderTrace(visible) {
    const laneVisible = state.selectedLane ? visible.filter((event) => event.rolloutId === state.selectedLane) : visible;
    const reconciled = [...laneVisible].reverse().find((event) => event.kind === "trace.reconciled");
    const partial = laneVisible.some((event) => event.kind === "trace.raw" || event.kind === "trace.visual");
    const status = $("trace-status");
    if (reconciled) {
      const digest = text(reconciled.raw?.payload?.trace_digest) || "digest recorded";
      status.textContent = "Sealed · " + digest;
      status.className = "trace-status sealed";
    } else {
      status.textContent = partial ? "Partial capture · unsealed" : (visible.length ? "Live eval projection · unsealed" : "Waiting · unsealed");
      status.className = "trace-status";
    }

    const list = $("trace-list");
    list.replaceChildren();
    const semantic = semanticTrace(laneVisible);
    const shown = state.traceMode === "full" ? semantic : semantic.filter((item) => item.category === "policy" || item.category === "evidence");
    $("trace-full").setAttribute("aria-pressed", String(state.traceMode === "full"));
    $("trace-focus").setAttribute("aria-pressed", String(state.traceMode === "focus"));
    $("trace-summary").textContent = state.traceMode === "full"
      ? shown.length + " semantic events folded from " + laneVisible.length + " durable envelopes."
      : shown.length + " policy calls and trace-authority events; transport partials are folded.";
    for (const itemData of shown) {
      const event = itemData.last;
      const key = itemData.key;
      const item = document.createElement("li");
      const button = document.createElement("button");
      button.type = "button";
      button.setAttribute("aria-current", String(state.selectedTraceKey === key));
      button.setAttribute("aria-label", itemData.kind + ": " + itemData.summary);
      for (const [className, value] of [
        ["trace-seq", itemData.first.cursor === itemData.last.cursor ? "#" + itemData.first.cursor : "#" + itemData.first.cursor + "–" + itemData.last.cursor],
        ["trace-kind", itemData.kind],
        ["trace-summary", itemData.summary]
      ]) {
        const span = document.createElement("span"); span.className = className; span.textContent = value; button.append(span);
      }
      button.addEventListener("click", () => { state.selectedTraceKey = key; render(); });
      item.append(button); list.append(item);
    }

    let selectedItem = semantic.find((item) => item.key === state.selectedTraceKey);
    if (!selectedItem) selectedItem = shown.at(-1);
    const selected = selectedItem?.last;
    if (!selected) {
      $("trace-detail-title").textContent = "Nothing selected";
      $("trace-detail-fields").replaceChildren();
      $("trace-detail-json").textContent = "Structural fields appear here. Prompt and reasoning text are withheld from this reference projection.";
      $("trace-input").textContent = "not emitted";
      $("trace-thinking").textContent = "not emitted";
      $("trace-output").textContent = "not emitted";
      $("trace-tools").textContent = "not emitted";
      return;
    }
    state.selectedTraceKey = selectedItem.key;
    $("trace-detail-title").textContent = selectedItem.kind;
    const fields = $("trace-detail-fields"); fields.replaceChildren();
    for (const [name, value] of [
      ["Time", fmtTime(selected.timeMs, true)], ["Sequence", selected.cursor || "—"],
      ["Step", selected.step], ["Category", traceCategory(selected)]
    ]) {
      const row = document.createElement("div"); const dt = document.createElement("dt"); const dd = document.createElement("dd");
      dt.textContent = name; dd.textContent = String(value); row.append(dt, dd); fields.append(row);
    }
    $("trace-detail-json").textContent = JSON.stringify(selectedItem.rawEvents.map(safeStructuralPayload), null, 2);
    const interaction = selectedItem.interaction || traceInteraction(selected, laneVisible);
    $("trace-input").textContent = traceValue(interaction.input);
    $("trace-thinking").textContent = traceValue(interaction.thinking);
    $("trace-output").textContent = interaction.responseType === "tool_call" && !interaction.output ? "Tool-only response (no text output)" : traceValue(interaction.output);
    $("trace-tools").textContent = traceValue(interaction.tools);
  }

  function render() {
    const ordered = orderedEvents();
    const checkpoints = semanticCheckpointIndexes(ordered);
    state.cursor = Math.max(-1, Math.min(state.cursor, ordered.length - 1));
    const visible = state.cursor >= 0 ? ordered.slice(0, state.cursor + 1) : [];
    const lanes = latestByLane(visible);
    if (state.selectedLane && !lanes.has(state.selectedLane) && lanes.size) state.selectedLane = lanes.keys().next().value;
    const history = state.selectedLane ? laneEventsAt(visible, state.selectedLane) : [];

    elements.timeline.disabled = !ordered.length;
    elements.timeline.max = String(Math.max(0, checkpoints.length - 1));
    elements.timeline.value = String(checkpointPosition(checkpoints, Math.max(0, state.cursor)));
    elements.play.disabled = checkpoints.length < 2;
    elements.live.disabled = !ordered.length;
    elements.live.textContent = state.followLive ? "Following live" : "Follow live";
    elements.timelineStart.textContent = ordered.length ? fmtTime(ordered[0].timeMs, true) : "—";
    elements.timelineEnd.textContent = ordered.length ? fmtTime(ordered.at(-1).timeMs, true) : "—";
    elements.timelineLabel.textContent = visible.length
      ? fmtTime(visible.at(-1).timeMs, true) + " · moment " + (checkpointPosition(checkpoints, state.cursor) + 1) + " of " + checkpoints.length + " · " + visible.length + " durable envelopes"
      : "Waiting for the first event";
    const completed = [...lanes.keys()].filter((lane) => {
      const rollout = rolloutState(laneEventsAt(visible, lane));
      return rollout.terminated || rollout.error;
    }).length;
    setText("metric-rollouts", lanes.size ? (lanes.size - completed) + " live · " + completed + " done" : "—");

    renderLanes(visible);
    renderDetails(history);
    renderCharts(history);
    renderActivity(visible);
    renderTrace(visible);
  }

  function stopReplay() {
    state.playing = false;
    clearTimeout(state.playTimer);
    state.playTimer = null;
    elements.play.textContent = "Play";
  }

  function stopFrameReplay() {
    state.framePlaying = false;
    clearTimeout(state.frameTimer);
    state.frameTimer = null;
    elements.framePlay.textContent = "Play video";
  }

  function selectedFrameEvents() {
    return orderedEvents().filter((event) => event.rolloutId === state.selectedLane && event.frameUrl);
  }

  function showFrameAt(index) {
    const ordered = orderedEvents();
    const frame = selectedFrameEvents()[index];
    const eventIndex = ordered.indexOf(frame);
    if (eventIndex < 0) return false;
    state.followLive = false;
    state.cursor = eventIndex;
    render();
    return true;
  }

  function replayNextFrame() {
    if (!state.framePlaying) return;
    const frames = selectedFrameEvents();
    if (!frames.length) { stopFrameReplay(); return; }
    const next = (Number(elements.frameTimeline.value) + 1) % frames.length;
    showFrameAt(next);
    state.frameTimer = setTimeout(replayNextFrame, 1000 / (Number(elements.frameSpeed.value) || 4));
  }

  function replayNext() {
    if (!state.playing) return;
    const ordered = orderedEvents();
    const checkpoints = semanticCheckpointIndexes(ordered);
    const position = checkpointPosition(checkpoints, state.cursor);
    if (!checkpoints.length || position >= checkpoints.length - 1) { stopReplay(); return; }
    const current = ordered[Math.max(0, state.cursor)];
    state.cursor = checkpoints[position + 1];
    const next = ordered[state.cursor];
    render();
    const speed = Number(elements.speed.value) || 1;
    const gap = Math.max(70, Math.min(1600, (next.timeMs - current.timeMs) / speed));
    state.playTimer = setTimeout(replayNext, gap);
  }

  elements.streamForm.addEventListener("submit", (event) => {
    event.preventDefault();
    const sources = splitSources(elements.streamInput.value);
    if (!sources.length) { elements.sourceMessage.textContent = "Enter at least one absolute SSE endpoint."; return; }
    for (const source of sources) connect(source);
    const url = new URL(window.location.href);
    url.searchParams.delete("stream");
    for (const source of sources) url.searchParams.append("stream", source);
    history.replaceState(null, "", url);
  });
  elements.disconnect.addEventListener("click", disconnectAll);
  elements.frame.addEventListener("error", () => {
    const visible = state.cursor >= 0 ? orderedEvents().slice(0, state.cursor + 1) : [];
    const history = state.selectedLane ? laneEventsAt(visible, state.selectedLane) : [];
    const ascii = lastDefined(history, (event) => event.ascii);
    elements.frame.hidden = true;
    if (ascii) {
      $("ascii-frame").textContent = ascii;
      $("ascii-frame").hidden = false;
      elements.frameEmpty.hidden = true;
    } else {
      $("ascii-frame").hidden = true;
      elements.frameEmpty.hidden = false;
    }
  });
  elements.timeline.addEventListener("input", () => {
    const checkpoints = semanticCheckpointIndexes(orderedEvents());
    stopReplay(); stopFrameReplay(); state.followLive = false;
    state.cursor = checkpoints[Number(elements.timeline.value)] ?? -1;
    render();
  });
  elements.live.addEventListener("click", () => {
    stopReplay(); stopFrameReplay(); state.followLive = true; state.cursor = orderedEvents().length - 1; render();
  });
  elements.play.addEventListener("click", () => {
    stopFrameReplay();
    if (state.playing) { stopReplay(); return; }
    const ordered = orderedEvents();
    const checkpoints = semanticCheckpointIndexes(ordered);
    if (state.cursor >= (checkpoints.at(-1) ?? -1)) state.cursor = -1;
    state.followLive = false; state.playing = true; elements.play.textContent = "Pause"; replayNext();
  });
  elements.frameTimeline.addEventListener("input", () => {
    stopReplay(); stopFrameReplay(); showFrameAt(Number(elements.frameTimeline.value));
  });
  elements.framePlay.addEventListener("click", () => {
    if (state.framePlaying) { stopFrameReplay(); return; }
    stopReplay(); state.framePlaying = true; elements.framePlay.textContent = "Pause video"; replayNextFrame();
  });
  $("trace-full").addEventListener("click", () => { state.traceMode = "full"; render(); });
  $("trace-focus").addEventListener("click", () => { state.traceMode = "focus"; render(); });

  elements.createRollout.addEventListener("click", async () => {
    const base = elements.base.value.replace(/\/$/, "");
    elements.sourceMessage.textContent = "Creating real telemetry rollout…";
    try {
      const infoResponse = await fetch(base + "/info");
      if (infoResponse.ok) {
        const info = await infoResponse.json();
        const actions = Array.isArray(info.action_names) ? info.action_names : [];
        elements.action.replaceChildren(...actions.map((name) => {
          const option = document.createElement("option"); option.value = name; option.textContent = name; return option;
        }));
      }
      const response = await fetch(base + "/rollouts", {
        method: "POST", headers: { "content-type": "application/json" },
        body: JSON.stringify({ seed: Number(elements.seed.value), telemetry: { enabled: true, transport: "sse", detail: "standard", frame: { enabled: true } } })
      });
      if (!response.ok) throw new Error("container returned HTTP " + response.status);
      const rollout = await response.json();
      const rolloutId = rollout.rollout_id;
      if (!rolloutId) throw new Error("container omitted rollout_id");
      const streamPath = rollout.stream && rollout.stream.sse_url ? rollout.stream.sse_url : "/rollouts/" + rolloutId + "/stream";
      const streamUrl = new URL(streamPath, base).toString();
      state.smoke = { base, rolloutId };
      elements.stepRollout.disabled = false;
      elements.streamInput.value = [elements.streamInput.value.trim(), streamUrl].filter(Boolean).join("\n");
      connect(streamUrl);
      elements.sourceMessage.textContent = "Observed rollout " + rolloutId + " created. Manual steps are transport evidence only.";
    } catch (error) {
      elements.sourceMessage.textContent = "Could not create rollout: " + error.message;
      setConnectionState("error", "Container error");
    }
  });

  elements.stepRollout.addEventListener("click", async () => {
    if (!state.smoke.rolloutId) return;
    elements.stepRollout.disabled = true;
    try {
      const response = await fetch(state.smoke.base + "/rollouts/" + encodeURIComponent(state.smoke.rolloutId) + "/step", {
        method: "POST", headers: { "content-type": "application/json" },
        body: JSON.stringify({ action: elements.action.value })
      });
      if (!response.ok) throw new Error("container returned HTTP " + response.status);
    } catch (error) {
      elements.sourceMessage.textContent = "Step failed: " + error.message;
    } finally {
      elements.stepRollout.disabled = false;
    }
  });

  const initialSources = new URL(window.location.href).searchParams.getAll("stream");
  if (initialSources.length) {
    elements.streamInput.value = initialSources.join("\n");
    for (const source of initialSources) connect(source);
  }
  if (requestedRollouts.size) {
    elements.sourceMessage.textContent = "Scoped to " + requestedRollouts.size + " requested rollout" + (requestedRollouts.size === 1 ? "" : "s") + ".";
  }
  render();
})();
