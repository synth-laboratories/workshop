"use strict";
var __defProp = Object.defineProperty;
var __getOwnPropDesc = Object.getOwnPropertyDescriptor;
var __getOwnPropNames = Object.getOwnPropertyNames;
var __hasOwnProp = Object.prototype.hasOwnProperty;
var __export = (target, all) => {
  for (var name in all)
    __defProp(target, name, { get: all[name], enumerable: true });
};
var __copyProps = (to, from, except, desc) => {
  if (from && typeof from === "object" || typeof from === "function") {
    for (let key of __getOwnPropNames(from))
      if (!__hasOwnProp.call(to, key) && key !== except)
        __defProp(to, key, { get: () => from[key], enumerable: !(desc = __getOwnPropDesc(from, key)) || desc.enumerable });
  }
  return to;
};
var __toCommonJS = (mod) => __copyProps(__defProp({}, "__esModule", { value: true }), mod);

// visuals/chrome/BenchmarkStatus.tsx
var BenchmarkStatus_exports = {};
__export(BenchmarkStatus_exports, {
  BenchmarkStatus: () => BenchmarkStatus
});
module.exports = __toCommonJS(BenchmarkStatus_exports);
var import_jsx_runtime = require("react/jsx-runtime");
var number = (value) => value === null ? "\u2014" : value.toLocaleString(void 0, { maximumFractionDigits: 2 });
var clock = (value) => `${number(value.elapsed_seconds)}/${number(value.limit_seconds)}s (${value.scope})`;
function BenchmarkStatus({ rows }) {
  if (!rows.length) return null;
  const work = rows.some((row) => row.work.elapsed_seconds !== null || row.work.limit_seconds !== null);
  const verify = rows.some((row) => row.verify.elapsed_seconds !== null || row.verify.limit_seconds !== null);
  const calls = rows.some((row) => row.calls !== null || row.call_limit !== null || row.call_limit_unbounded);
  const steps = rows.some((row) => row.steps !== null || row.step_limit !== null);
  return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("section", { "aria-label": "Benchmark status", style: { overflowX: "auto" }, children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("table", { className: "sv-table", children: [
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("thead", { children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("tr", { children: [
      /* @__PURE__ */ (0, import_jsx_runtime.jsx)("th", { scope: "col", children: "Task / lane" }),
      /* @__PURE__ */ (0, import_jsx_runtime.jsx)("th", { scope: "col", children: "State" }),
      work && /* @__PURE__ */ (0, import_jsx_runtime.jsx)("th", { scope: "col", children: "Work" }),
      verify && /* @__PURE__ */ (0, import_jsx_runtime.jsx)("th", { scope: "col", children: "Verify" }),
      calls && /* @__PURE__ */ (0, import_jsx_runtime.jsx)("th", { scope: "col", children: "Calls" }),
      steps && /* @__PURE__ */ (0, import_jsx_runtime.jsx)("th", { scope: "col", children: "Steps" }),
      /* @__PURE__ */ (0, import_jsx_runtime.jsx)("th", { scope: "col", children: "Score" }),
      /* @__PURE__ */ (0, import_jsx_runtime.jsx)("th", { scope: "col", children: "Usage" }),
      /* @__PURE__ */ (0, import_jsx_runtime.jsx)("th", { scope: "col", children: "Stop / evidence" })
    ] }) }),
    /* @__PURE__ */ (0, import_jsx_runtime.jsx)("tbody", { children: rows.map((row) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("tr", { children: [
      /* @__PURE__ */ (0, import_jsx_runtime.jsx)("th", { scope: "row", children: row.lane }),
      /* @__PURE__ */ (0, import_jsx_runtime.jsx)("td", { children: row.phase }),
      work && /* @__PURE__ */ (0, import_jsx_runtime.jsx)("td", { children: clock(row.work) }),
      verify && /* @__PURE__ */ (0, import_jsx_runtime.jsx)("td", { children: clock(row.verify) }),
      calls && /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("td", { children: [
        number(row.calls),
        "/",
        row.call_limit_unbounded ? "\u221E" : number(row.call_limit),
        " (",
        row.limit_scope,
        ")"
      ] }),
      steps && /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("td", { children: [
        number(row.steps),
        "/",
        number(row.step_limit)
      ] }),
      /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("td", { children: [
        row.scientific_status === "ungraded" ? "UNGRADED" : number(row.score),
        /* @__PURE__ */ (0, import_jsx_runtime.jsx)("br", {}),
        /* @__PURE__ */ (0, import_jsx_runtime.jsx)("small", { children: row.scientific_status })
      ] }),
      /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("td", { title: `${row.usage.source} \xB7 ${row.usage.coverage}`, children: [
        number(row.usage.total_tokens),
        " tokens \xB7 $",
        number(row.usage.cost_usd),
        "/$",
        number(row.spend_limit_usd),
        " \xB7 ",
        row.usage.status,
        /* @__PURE__ */ (0, import_jsx_runtime.jsx)("br", {}),
        /* @__PURE__ */ (0, import_jsx_runtime.jsx)("small", { children: row.spend_enforcement })
      ] }),
      /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("td", { children: [
        row.stop_reason || "\u2014",
        /* @__PURE__ */ (0, import_jsx_runtime.jsx)("br", {}),
        /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("small", { children: [
          "Observed ",
          row.observed_at
        ] }),
        row.artifact_path && /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("details", { children: [
          /* @__PURE__ */ (0, import_jsx_runtime.jsx)("summary", { children: "Evidence location" }),
          /* @__PURE__ */ (0, import_jsx_runtime.jsx)("code", { children: row.artifact_path })
        ] })
      ] })
    ] }, row.lane)) })
  ] }) });
}
// Annotate the CommonJS export names for ESM import in node:
0 && (module.exports = {
  BenchmarkStatus
});
