(() => {
  const MISSING = "—";
  function escape(value) {
    return String(value ?? "").replace(/[&<>"']/g, (ch) => ({
      "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;",
    })[ch]);
  }
  function num(value) {
    return typeof value === "number" && Number.isFinite(value);
  }
  function fmtCost(value) {
    if (!num(value)) return MISSING;
    return value < 0.01 ? `$${value.toFixed(4)}` : `$${value.toFixed(3)}`;
  }
  function fmtMin(seconds) {
    if (!num(seconds)) return MISSING;
    return `${(seconds / 60).toFixed(1)} min`;
  }
  function fmtMean(value) {
    return num(value) ? value.toFixed(2) : MISSING;
  }
  function filmBody(frame) {
    if (frame && frame.image) {
      return `<img class="sv-film-img" alt="Craftax observation" src="${escape(frame.image)}">`;
    }
    return `<pre>${escape((frame && frame.map) || MISSING)}</pre>`;
  }
  function paintFilm(article, frame) {
    const img = article.querySelector("img.sv-film-img");
    const pre = article.querySelector("pre");
    const meta = article.querySelector(".sv-count");
    if (frame && frame.image && img) {
      img.src = frame.image;
    } else if (pre) {
      pre.textContent = (frame && frame.map) || MISSING;
    }
    if (meta) meta.textContent = `t${frame.call ?? "—"} · ${((frame && frame.actions) || []).join(" ") || "idle"} · reward ${frame.reward ?? MISSING}${frame.unlocks && frame.unlocks.length ? ` · unlocked ${frame.unlocks.join(", ")}` : ""}`;
  }
  const SEED_R = { low: 3.5, medium: 5.5, high: 8 };
  const MEAN_R = { low: 5, medium: 7, high: 10 };

  function mount(root, data) {
    root.innerHTML = "";
    root.classList.add("sv-compare");
    const efforts = data.efforts || ["low", "medium", "high"];
    const state = { axis: "cost", theme: 0, frame: 0, playing: false, timer: 0 };
    const shell = document.createElement("div");
    root.append(shell);

    function stop() {
      if (state.timer) window.clearInterval(state.timer);
      state.timer = 0;
      state.playing = false;
    }

    function render() {
      const points = data.points || [];
      const axis = state.axis === "time" ? "wall_s" : "cost";
      const means = (data.means || []).filter((row) => num(row.reward) && num(row[axis]));
      const axisLabel = state.axis === "time" ? "wall time (min)" : "cost / rollout ($)";
      const xMax = Math.max(...[...points, ...means].map((p) => Number(p[axis]) || 0), 0.001);
      const yMax = Math.max(...[...points, ...means].map((p) => Number(p.reward) || 0), 1);
      const xOf = (p) => 56 + ((Number(p[axis]) || 0) / xMax) * 300;
      const yOf = (p) => 168 - ((Number(p.reward) || 0) / yMax) * 140;
      const seedR = (row) => SEED_R[effortOf(row)] || SEED_R.medium;
      const meanR = (row) => MEAN_R[effortOf(row)] || MEAN_R.medium;
      const rates = (data.achievements || []).filter((row) => row.highlight);
      const theme = (data.themes || [])[state.theme] || { clips: [] };
      const maxFrames = Math.max(0, ...theme.clips.map((clip) => (clip.frames || []).length));
      if (state.frame >= maxFrames) state.frame = 0;
      const scale = data.effortScale || [];
      const scaleMax = Math.max(1, ...scale.map((cell) => (num(cell.mean) ? cell.mean : 0)));
      const xEffort = (effort) => 70 + (Math.max(0, efforts.indexOf(effort)) / Math.max(efforts.length - 1, 1)) * 280;
      const yEffort = (mean) => 168 - (mean / scaleMax) * 140;

      shell.innerHTML = `
        <p class="sv-kicker">Craftax contrast · paired seeds 0–9 · effort low / medium / high · Nemotron omitted</p>
        <h3 class="sv-title">${escape(data.title || "Two models, two receipts")}</h3>
        <p class="sv-lede">${escape(data.lede || "")}</p>

        <section class="sv-section">
          <div class="sv-section-head">
            <h3>Score vs effort</h3>
            <span>low · medium · high · missing is ${MISSING}, not 0</span>
          </div>
          <p class="sv-note">Only medium is scored. Low and high are empty slots so a later sweep can show whether reward scales with reasoning effort.</p>
          <svg class="sv-pareto" viewBox="0 0 400 210" role="img" aria-label="Mean reward versus reasoning effort">
            ${[28, 63, 98, 133, 168].map((y) => `<line x1="48" y1="${y}" x2="368" y2="${y}" stroke="#ece7df" />`).join("")}
            ${efforts.map((effort) => `<line x1="${xEffort(effort)}" y1="28" x2="${xEffort(effort)}" y2="168" stroke="${effort === "medium" ? "#d6d3d1" : "#f5f0e6"}" stroke-dasharray="${effort === "medium" ? "0" : "4 4"}" />`).join("")}
            ${(data.arms || []).map((arm) => {
              const cells = efforts.map((effort) => scale.find((cell) => cell.arm === arm.id && cell.effort === effort) || { arm: arm.id, effort, mean: null });
              const present = cells.filter((cell) => num(cell.mean));
              const line = present.length >= 2
                ? `<polyline fill="none" stroke="${arm.color}" stroke-width="1.5" points="${present.map((cell) => `${xEffort(cell.effort)},${yEffort(cell.mean)}`).join(" ")}" />`
                : "";
              const marks = cells.map((cell) => {
                const x = xEffort(cell.effort);
                if (num(cell.mean)) {
                  return `<g>
                    <title>${escape(arm.label)} · ${escape(cell.effort)} · mean ${fmtMean(cell.mean)} · ${cell.scored ?? MISSING}/${cell.n ?? MISSING} scored</title>
                    <rect x="${x - 6}" y="${yEffort(cell.mean) - 6}" width="12" height="12" fill="${arm.color}" stroke="#1c1917" stroke-width="1.25" transform="rotate(45 ${x} ${yEffort(cell.mean)})" />
                  </g>`;
                }
                return `<g>
                  <title>${escape(arm.label)} · ${escape(cell.effort)} · not run</title>
                  <text x="${x}" y="${arm.id === "oss20" ? 84 : 104}" text-anchor="middle" fill="${arm.color}" font-size="16">${MISSING}</text>
                </g>`;
              }).join("");
              return line + marks;
            }).join("")}
            ${efforts.map((effort) => `<text x="${xEffort(effort)}" y="202" text-anchor="middle" fill="${effort === "medium" ? "#1c1917" : "#78716c"}" font-size="11">${escape(effort)}</text>`).join("")}
            <text x="16" y="108" text-anchor="middle" fill="#78716c" font-size="11" transform="rotate(-90 16 108)">mean reward</text>
          </svg>
          <div class="sv-effort-grid">
            <div class="sv-effort-head"><span>model</span>${efforts.map((effort) => `<span>${escape(effort)}</span>`).join("")}</div>
            ${(data.arms || []).map((arm) => `<div class="sv-effort-row">
              <strong>${escape(arm.label)}</strong>
              ${efforts.map((effort) => {
                const cell = scale.find((row) => row.arm === arm.id && row.effort === effort);
                const present = num(cell && cell.mean);
                return `<div class="${present ? "" : "sv-empty"}"><b>${fmtMean(cell && cell.mean)}</b><small>${present ? `${cell.scored}/${cell.n} · ${fmtCost(cell.cost)}` : "not run"}</small></div>`;
              }).join("")}
            </div>`).join("")}
          </div>
        </section>

        <section class="sv-section">
          <div class="sv-section-head">
            <h3>Reward vs ${escape(state.axis === "time" ? "time" : "cost")}</h3>
            <div class="sv-density" role="group" aria-label="Pareto axis">
              <button type="button" data-axis="cost" class="${state.axis === "cost" ? "active" : ""}">cost</button>
              <button type="button" data-axis="time" class="${state.axis === "time" ? "active" : ""}">time</button>
            </div>
          </div>
          <p class="sv-note">Color is the model. Larger markers are more effort. Lines connect low → medium → high once those cells exist. Only medium is scored today.</p>
          <svg class="sv-pareto" viewBox="0 0 400 210" role="img" aria-label="Reward versus ${escape(axisLabel)}; marker size is reasoning effort">
            ${[28, 63, 98, 133, 168].map((y) => `<line x1="48" y1="${y}" x2="368" y2="${y}" stroke="#ece7df" />`).join("")}
            ${(data.arms || []).map((arm) => {
              const chain = efforts
                .map((effort) => means.find((row) => (row.arm || "") === arm.id && effortOf(row) === effort))
                .filter(Boolean);
              if (chain.length < 2) return "";
              return `<polyline fill="none" stroke="${arm.color}" stroke-width="1.5" stroke-linecap="round" points="${chain.map((row) => `${xOf(row)},${yOf(row)}`).join(" ")}" />`;
            }).join("")}
            ${points.map((p) => `<g>
              <title>${escape(p.label)} · ${escape(effortOf(p))} · reward ${p.reward} · ${state.axis === "time" ? fmtMin(p.wall_s) : fmtCost(p.cost)} · invalid ${p.invalids ?? MISSING}</title>
              <circle cx="${xOf(p)}" cy="${yOf(p)}" r="${seedR(p)}" fill="${p.color}" fill-opacity="0.72" stroke="#fff" stroke-width="1.25" />
            </g>`).join("")}
            ${means.map((p) => {
              const r = meanR(p);
              return `<g>
                <title>${escape(p.label)} · ${escape(effortOf(p))} · reward ${fmtMean(p.reward)}</title>
                <rect x="${xOf(p) - r}" y="${yOf(p) - r}" width="${r * 2}" height="${r * 2}" fill="${p.color}" stroke="#1c1917" stroke-width="1.25" transform="rotate(45 ${xOf(p)} ${yOf(p)})" />
              </g>`;
            }).join("")}
            <text x="208" y="202" text-anchor="middle" fill="#78716c" font-size="11">${escape(axisLabel)}</text>
            <text x="16" y="108" text-anchor="middle" fill="#78716c" font-size="11" transform="rotate(-90 16 108)">reward</text>
          </svg>
          <div class="sv-legend">
            ${(data.arms || []).map((arm) => `<span><i style="background:${arm.color}"></i>${escape(arm.label)}</span>`).join("")}
            <span class="sv-size">${efforts.map((effort) => {
              const scored = (data.effortScale || []).some((cell) => cell.effort === effort && num(cell.mean));
              return `<b class="${scored ? "" : "sv-empty"}" title="${scored ? escape(effort) : `${effort} not run`}"><svg width="${SEED_R[effort] * 2 + 6}" height="${SEED_R.high * 2 + 4}" viewBox="0 0 ${SEED_R[effort] * 2 + 6} ${SEED_R.high * 2 + 4}" aria-hidden="true"><circle cx="${SEED_R[effort] + 3}" cy="${SEED_R.high + 2}" r="${SEED_R[effort]}" fill="${scored ? "#57534e" : "none"}" stroke="#57534e" stroke-width="1.25" stroke-dasharray="${scored ? "0" : "2 2"}" /></svg>${escape(effort)}</b>`;
            }).join("")}<em>larger = more effort</em></span>
          </div>
        </section>

        <section class="sv-section">
          <div class="sv-section-head">
            <h3>Achievements that actually split</h3>
            <span>medium effort · n=10 paired · exclusive or ≥20pp gap · wood/sapling floor omitted</span>
          </div>
          <p class="sv-note">Nothing here is a p&lt;0.05 claim. These are the only achievements that were not shared noise. Low and high stay ${MISSING} until those rollouts exist.</p>
          <div class="sv-rates">
            ${rates.map((row) => {
              const delta = Math.round((row.oss120 - row.oss20) * 100);
              const side = row.oss20 > row.oss120 ? "20B" : row.oss120 > row.oss20 ? "120B" : "tie";
              return `<div class="sv-rate">
                <strong>${escape(row.name)}</strong>
                <div class="sv-dual">
                  <span>${Math.round(row.oss20 * 10)}/10</span>
                  <b style="width:${Math.max(row.oss20, 0.04) * 100}%;background:${(data.arms[0] || {}).color || "#c2410c"}"></b>
                </div>
                <div class="sv-dual">
                  <span>${Math.round(row.oss120 * 10)}/10</span>
                  <b style="width:${Math.max(row.oss120, 0.04) * 100}%;background:${(data.arms[1] || {}).color || "#1d4ed8"}"></b>
                </div>
                <em class="sv-chip">${escape(row.exclusive ? `${side} only` : `${delta > 0 ? "+" : ""}${delta}pp`)}</em>
              </div>`;
            }).join("")}
          </div>
        </section>

        <section class="sv-section">
          <div class="sv-section-head">
            <h3>Theme rollouts</h3>
            <span>text observation · env PNG at LLM-call boundaries · ASCII if a frame is missing</span>
          </div>
          <nav class="sv-tabs" aria-label="Contrast themes">
            ${(data.themes || []).map((theme, index) => `<button type="button" data-theme="${index}" class="${state.theme === index ? "active" : ""}">${escape(theme.title)}</button>`).join("")}
          </nav>
          <p class="sv-lede">${escape(theme.story || "")}</p>
          <div class="sv-film-controls">
            <button type="button" data-play="1">${state.playing ? "Pause" : "Play"}</button>
            <input data-role="scrub" type="range" min="0" max="${Math.max(maxFrames - 1, 0)}" value="${state.frame}" aria-label="Frame" />
            <span class="sv-count">frame ${state.frame + 1} / ${maxFrames || 1}</span>
          </div>
          <div class="sv-films">
            ${theme.clips.map((clip) => {
              const frame = clip.frames[Math.min(state.frame, clip.frames.length - 1)] || clip.frames[clip.frames.length - 1] || {};
              return `<article class="sv-film">
                <header><strong>${escape(clip.label)}</strong><span>${escape(clip.kicker || "")}</span></header>
                ${filmBody(frame)}
                <p class="sv-count">t${frame.call ?? "—"} · ${escape((frame.actions || []).join(" ") || "idle")} · reward ${frame.reward ?? MISSING}${frame.unlocks && frame.unlocks.length ? ` · unlocked ${escape(frame.unlocks.join(", "))}` : ""}</p>
              </article>`;
            }).join("")}
          </div>
        </section>

        <section class="sv-section">
          <div class="sv-section-head">
            <h3>Invalid actions and other issues</h3>
            <span>medium effort · 90 vs 14 invalids · 20B also talks 7.6× more</span>
          </div>
          <div class="sv-invalids">
            ${(data.invalids || []).map((row) => `<div class="sv-inv">
              <span>s${row.seed}</span>
              <b style="height:${Math.max(row.oss20, 1) / (data.invalidMax || 1) * 72}px;background:${(data.arms[0] || {}).color || "#c2410c"}" title="20B ${row.oss20}"></b>
              <b style="height:${Math.max(row.oss120, 1) / (data.invalidMax || 1) * 72}px;background:${(data.arms[1] || {}).color || "#1d4ed8"}" title="120B ${row.oss120}"></b>
            </div>`).join("")}
          </div>
          <ul class="sv-issues">
            ${(data.issues || []).map((item) => `<li>${escape(item)}</li>`).join("")}
          </ul>
        </section>

        <section class="sv-section">
          <div class="sv-section-head"><h3>CoT that shows the split</h3><span>medium effort · sealed reasoning, truncated</span></div>
          <div class="sv-cots">
            ${(data.cots || []).map((row) => `<article class="sv-cot">
              <h4>${escape(row.title)}</h4>
              <div class="sv-cot-grid">
                <blockquote><strong>${escape(row.left.label)}</strong><p>${escape(row.left.body)}</p></blockquote>
                <blockquote><strong>${escape(row.right.label)}</strong><p>${escape(row.right.body)}</p></blockquote>
              </div>
            </article>`).join("")}
          </div>
        </section>
      `;

      shell.querySelectorAll("[data-axis]").forEach((button) => button.addEventListener("click", () => {
        state.axis = button.getAttribute("data-axis");
        render();
      }));
      shell.querySelectorAll("[data-theme]").forEach((button) => {
        button.addEventListener("click", () => {
          stop();
          state.theme = Number(button.getAttribute("data-theme"));
          state.frame = 0;
          render();
        });
      });
      shell.querySelector("[data-play]")?.addEventListener("click", () => {
        if (state.playing) {
          stop();
          render();
          return;
        }
        state.playing = true;
        render();
        const clipLen = Math.max(...((((data.themes || [])[state.theme] || {}).clips || []).map((clip) => (clip.frames || []).length)), 1);
        state.timer = window.setInterval(() => {
          state.frame = (state.frame + 1) % clipLen;
          const input = shell.querySelector("[data-role=scrub]");
          const clips = ((data.themes || [])[state.theme] || {}).clips || [];
          shell.querySelectorAll(".sv-film").forEach((article, index) => {
            const frames = clips[index]?.frames || [];
            const frame = frames[Math.min(state.frame, Math.max(frames.length - 1, 0))] || {};
            paintFilm(article, frame);
          });
          if (input) input.value = String(state.frame);
          const label = shell.querySelector(".sv-film-controls .sv-count");
          if (label) label.textContent = `frame ${state.frame + 1} / ${clipLen}`;
        }, 650);
      });
      shell.querySelector("[data-role=scrub]")?.addEventListener("input", (event) => {
        state.frame = Number(event.target.value);
        const clips = ((data.themes || [])[state.theme] || {}).clips || [];
        const clipLen = Math.max(...clips.map((clip) => (clip.frames || []).length), 1);
        shell.querySelectorAll(".sv-film").forEach((article, index) => {
          const frames = clips[index]?.frames || [];
          const frame = frames[Math.min(state.frame, Math.max(frames.length - 1, 0))] || {};
          paintFilm(article, frame);
        });
        const label = shell.querySelector(".sv-film-controls .sv-count");
        if (label) label.textContent = `frame ${state.frame + 1} / ${clipLen}`;
      });
    }

    render();
  }

  window.SynthCompareStory = { mount };
})();
