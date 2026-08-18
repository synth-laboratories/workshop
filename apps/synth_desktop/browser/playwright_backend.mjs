#!/usr/bin/env node
import { chromium } from "playwright";
import crypto from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import readline from "node:readline";

const PROTOCOL_VERSION = "workshop.browser.v1";
const DEFAULT_MAX_CHARS = 16_000;
const HARD_MAX_CHARS = 20_000;
const sessions = new Map();
const environmentAllowedOrigins = new Set(
  (process.env.SYNTH_BROWSER_ALLOWED_ORIGINS ?? "http://localhost,http://127.0.0.1")
    .split(",")
    .map((value) => value.trim())
    .filter(Boolean),
);

function allowedOrigins() {
  const origins = new Set(environmentAllowedOrigins);
  const policyFile = process.env.SYNTH_BROWSER_POLICY_FILE;
  if (!policyFile) return origins;
  try {
    const policy = JSON.parse(fs.readFileSync(policyFile, "utf8"));
    for (const origin of policy.allowedOrigins ?? []) origins.add(String(origin));
  } catch (error) {
    if (error?.code !== "ENOENT") throw new Error(`browser_policy_invalid: ${error.message}`);
  }
  return origins;
}

function id(prefix) {
  return `${prefix}_${crypto.randomUUID().replaceAll("-", "")}`;
}

function safeName(value) {
  const safe = String(value ?? "default").replace(/[^a-zA-Z0-9._-]/g, "_");
  if (!safe || safe === "." || safe === "..") throw new Error("invalid profile name");
  return safe;
}

function profileRoot() {
  return process.env.SYNTH_BROWSER_PROFILE_ROOT
    ? path.resolve(process.env.SYNTH_BROWSER_PROFILE_ROOT)
    : path.join(os.homedir(), "Library", "Application Support", "Synth Desktop", "browser-profiles");
}

function originOf(url) {
  if (url === "about:blank" || url.startsWith("data:")) return url.split("#", 1)[0];
  try { return new URL(url).origin; } catch { return "unknown"; }
}

function requireAllowed(url) {
  const parsed = new URL(url);
  if (!["http:", "https:"].includes(parsed.protocol)) throw new Error("navigation allows only HTTP(S) URLs");
  const localBase = `${parsed.protocol}//${parsed.hostname}`;
  const approvedOrigins = allowedOrigins();
  const approved = approvedOrigins.has(parsed.origin)
    || (["localhost", "127.0.0.1", "::1"].includes(parsed.hostname) && approvedOrigins.has(localBase));
  if (!approved) {
    throw new Error(`origin_not_approved: ${parsed.origin}; approve it in Workshop browser settings`);
  }
}

function bounded(text, args = {}) {
  const maxChars = Math.max(256, Math.min(Number(args.max_chars ?? args.maxChars ?? DEFAULT_MAX_CHARS), HARD_MAX_CHARS));
  const cursor = Math.max(0, Number(args.cursor ?? 0));
  const end = Math.min(text.length, cursor + maxChars);
  return {
    text: text.slice(cursor, end),
    maxChars,
    truncated: end < text.length,
    continuationCursor: end < text.length ? end : null,
  };
}

async function audit(session, event, details = {}) {
  const row = { at: new Date().toISOString(), sessionId: session.id, event, ...details };
  await fs.promises.appendFile(session.auditPath, `${JSON.stringify(row)}\n`, { mode: 0o600 });
}

function stateForPage(session, page) {
  for (const state of session.tabs.values()) if (state.page === page) return state;
  const state = { id: id("tab"), page, nav: 0, refs: new Map(), refRevision: "", closed: false };
  session.tabs.set(state.id, state);
  page.on("framenavigated", (frame) => {
    if (frame === page.mainFrame()) {
      state.nav += 1;
      state.refs.clear();
      state.refRevision = "";
    }
  });
  page.on("close", () => { state.closed = true; });
  page.on("dialog", async (dialog) => {
    await audit(session, "dialog.dismissed", { tabId: state.id, type: dialog.type(), message: dialog.message().slice(0, 500) });
    await dialog.dismiss().catch(() => {});
  });
  return state;
}

async function installRevisionTracking(context) {
  await context.addInitScript(() => {
    globalThis.__workshopMutationRevision = 0;
    const start = () => new MutationObserver(() => { globalThis.__workshopMutationRevision += 1; })
      .observe(document, { subtree: true, childList: true, attributes: true, characterData: true });
    if (document.documentElement) start(); else addEventListener("DOMContentLoaded", start, { once: true });
  });
}

async function revision(state) {
  const mutation = await state.page.evaluate(() => Number(globalThis.__workshopMutationRevision ?? 0)).catch(() => 0);
  const digest = crypto.createHash("sha256").update(state.page.url()).digest("hex").slice(0, 10);
  return `${state.nav}.${mutation}.${digest}`;
}

async function meta(session, state, extra = {}) {
  return {
    sessionId: session.id,
    tabId: state.id,
    documentRevision: await revision(state),
    origin: originOf(state.page.url()),
    truncated: false,
    stale: false,
    continuationCursor: null,
    ...extra,
  };
}

function response(result, metaValue = null) {
  return { protocolVersion: PROTOCOL_VERSION, meta: metaValue, result };
}

function getSession(args) {
  const session = sessions.get(args.session_id ?? args.sessionId);
  if (!session) throw new Error("unknown or closed browser session");
  return session;
}

function getTab(session, args) {
  const state = session.tabs.get(args.tab_id ?? args.tabId);
  if (!state || state.closed || state.page.isClosed()) throw new Error("unknown or closed tab; tab IDs are never reused");
  return state;
}

async function scanPage(state, rootSelector = "body") {
  return state.page.locator(rootSelector).evaluate((root) => {
    const visible = (el) => {
      const style = getComputedStyle(el);
      const rect = el.getBoundingClientRect();
      return style.visibility !== "hidden" && style.display !== "none" && rect.width > 0 && rect.height > 0;
    };
    const implicitRole = (el) => {
      const tag = el.tagName.toLowerCase();
      if (/^h[1-6]$/.test(tag)) return "heading";
      if (tag === "a" && el.hasAttribute("href")) return "link";
      if (tag === "button") return "button";
      if (tag === "textarea") return "textbox";
      if (tag === "select") return "combobox";
      if (tag === "input") {
        const type = (el.getAttribute("type") || "text").toLowerCase();
        if (["button", "submit", "reset"].includes(type)) return "button";
        if (type === "checkbox") return "checkbox";
        if (type === "radio") return "radio";
        return "textbox";
      }
      if (tag === "li") return "listitem";
      if (tag === "p") return "paragraph";
      return "";
    };
    const nameOf = (el, role) => {
      if (el.matches("input[type=password]")) return el.getAttribute("aria-label") || "Password";
      const labelled = el.getAttribute("aria-label") || el.getAttribute("title") || el.getAttribute("alt");
      if (labelled) return labelled.trim();
      if (role === "textbox") {
        const label = el.labels?.[0]?.innerText || el.getAttribute("placeholder");
        return (label || "").trim();
      }
      return (el.innerText || el.textContent || el.getAttribute("value") || "").replace(/\s+/g, " ").trim();
    };
    const cssPath = (el) => {
      const parts = [];
      while (el && el.nodeType === Node.ELEMENT_NODE && el !== document.documentElement) {
        let part = el.tagName.toLowerCase();
        if (el.id && /^[a-zA-Z][\w-]*$/.test(el.id)) { parts.unshift(`${part}#${CSS.escape(el.id)}`); break; }
        const siblings = [...el.parentElement.children].filter((candidate) => candidate.tagName === el.tagName);
        if (siblings.length > 1) part += `:nth-of-type(${siblings.indexOf(el) + 1})`;
        parts.unshift(part);
        el = el.parentElement;
      }
      return parts.join(" > ");
    };
    const rows = [];
    for (const el of root.querySelectorAll("[role],h1,h2,h3,h4,h5,h6,a[href],button,input,textarea,select,p,li")) {
      if (!visible(el)) continue;
      const role = el.getAttribute("role") || implicitRole(el);
      const name = nameOf(el, role).slice(0, 500);
      if (!role || !name) continue;
      rows.push({ role, name, selector: cssPath(el), sensitive: el.matches("input[type=password]") });
      if (rows.length >= 2_000) break;
    }
    return rows;
  });
}

async function observe(session, state, args, rows = null) {
  const currentRevision = await revision(state);
  const scanned = rows ?? await scanPage(state);
  state.refs.clear();
  state.refRevision = currentRevision;
  const lines = [];
  let elementCount = 0;
  for (const row of scanned) {
    const elementId = `e${elementCount + 1}`;
    state.refs.set(elementId, row);
    elementCount += 1;
    lines.push(`[${elementId}] ${row.role} "${row.name.replaceAll('"', "'")}"`);
  }
  const limited = bounded(lines.join("\n") + (lines.length ? "\n" : ""), args);
  return response(
    { text: limited.text, elementCount, maxChars: limited.maxChars },
    await meta(session, state, { truncated: limited.truncated || scanned.length >= 2_000, continuationCursor: limited.continuationCursor }),
  );
}

async function resolveTarget(session, state, target) {
  if (!target) throw new Error("target is required");
  if (target.ref) {
    const ref = target.ref;
    const current = await revision(state);
    if (ref.session_id !== session.id || ref.tab_id !== state.id || ref.document_revision !== current || state.refRevision !== current) {
      throw new Error("stale_ref: session, tab, or document changed; take a new snapshot or query");
    }
    const stored = state.refs.get(ref.element_id);
    if (!stored) throw new Error("stale_ref: element is not in the current server-side reference map");
    return { locator: state.page.locator(stored.selector), descriptor: stored };
  }
  if (target.locator) {
    const { role, name, exact = false } = target.locator;
    if (!role || !name) throw new Error("semantic locator requires role and name");
    const locator = state.page.getByRole(role, { name, exact });
    const count = await locator.count();
    if (count !== 1) throw new Error(`ambiguous_locator: expected exactly one match, found ${count}`);
    return { locator, descriptor: { role, name } };
  }
  throw new Error("target must contain ref or locator");
}

function consequential(descriptor) {
  return /^(send|publish|purchase|buy|delete|remove|submit|confirm|place order|transfer)$/i.test(String(descriptor?.name ?? "").trim());
}

async function act(operation, args) {
  const session = getSession(args);
  const state = getTab(session, args);
  if (operation === "browser_press" && !args.target) {
    await state.page.keyboard.press(String(args.key ?? ""));
    await audit(session, operation, { tabId: state.id, origin: originOf(state.page.url()), key: args.key });
    return response({ ok: true }, await meta(session, state));
  }
  const { locator, descriptor } = await resolveTarget(session, state, args.target);
  if (consequential(descriptor) && process.env.SYNTH_BROWSER_ALLOW_CONSEQUENTIAL !== "1") {
    throw new Error(`confirmation_required: ${descriptor.name}; approve this exact action in Workshop`);
  }
  if (operation === "browser_click") await locator.click();
  else if (operation === "browser_fill") await locator.fill(String(args.value ?? ""));
  else if (operation === "browser_press") await locator.press(String(args.key ?? ""));
  else if (operation === "browser_upload") {
    const roots = (process.env.SYNTH_BROWSER_UPLOAD_ROOTS ?? "").split(path.delimiter).filter(Boolean).map((root) => path.resolve(root));
    const files = (args.file_paths ?? args.filePaths ?? []).map((file) => fs.realpathSync(file));
    if (!files.length || !roots.length || files.some((file) => !roots.some((root) => file === root || file.startsWith(`${root}${path.sep}`)))) {
      throw new Error("upload_refused: select explicit files from a Workshop-approved upload root");
    }
    await locator.setInputFiles(files);
  }
  await audit(session, operation, { tabId: state.id, origin: originOf(state.page.url()), role: descriptor.role, name: descriptor.name });
  return response({ ok: true }, await meta(session, state));
}

async function handle(operation, args = {}) {
  if (operation === "browser_create_session") {
    const profileName = safeName(args.profile ?? "default");
    const dir = path.join(profileRoot(), profileName);
    await fs.promises.mkdir(dir, { recursive: true, mode: 0o700 });
    const context = await chromium.launchPersistentContext(dir, {
      headless: process.env.SYNTH_BROWSER_HEADLESS === "1",
      acceptDownloads: true,
      downloadsPath: path.join(dir, "downloads"),
    });
    await installRevisionTracking(context);
    const session = { id: id("browser_session"), profileName, dir, context, tabs: new Map(), auditPath: path.join(dir, "audit.jsonl") };
    sessions.set(session.id, session);
    context.on("page", (page) => { const tab = stateForPage(session, page); void audit(session, "popup", { tabId: tab.id, url: page.url() }); });
    const page = context.pages()[0] ?? await context.newPage();
    const state = stateForPage(session, page);
    await audit(session, "session.created", { profileName });
    return response({ sessionId: session.id, profile: profileName, tabId: state.id }, await meta(session, state));
  }
  const session = getSession(args);
  if (operation === "browser_close_session") {
    await audit(session, "session.closed");
    await session.context.close();
    sessions.delete(session.id);
    return response({ closed: true });
  }
  if (operation === "browser_list_tabs") {
    const tabs = [];
    for (const state of session.tabs.values()) if (!state.closed && !state.page.isClosed()) tabs.push({ tabId: state.id, title: await state.page.title(), url: state.page.url(), meta: await meta(session, state) });
    return response({ tabs });
  }
  if (operation === "browser_new_tab") {
    const page = await session.context.newPage();
    const state = stateForPage(session, page);
    if (args.url) { requireAllowed(args.url); await page.goto(args.url); }
    await audit(session, "tab.created", { tabId: state.id, url: page.url() });
    return response({ tabId: state.id, url: page.url() }, await meta(session, state));
  }
  const state = getTab(session, args);
  if (operation === "browser_close_tab") {
    await audit(session, "tab.closed", { tabId: state.id, url: state.page.url() });
    await state.page.close();
    return response({ closed: true }, await meta(session, state).catch(() => ({ sessionId: session.id, tabId: state.id, documentRevision: "closed", origin: "closed", truncated: false, stale: false, continuationCursor: null })));
  }
  if (operation === "browser_navigate") {
    requireAllowed(args.url);
    await audit(session, "navigation.requested", { tabId: state.id, from: state.page.url(), to: args.url });
    await state.page.goto(args.url, { waitUntil: "domcontentloaded" });
    await state.page.waitForLoadState("networkidle", { timeout: 3_000 }).catch(() => {});
    return response({ url: state.page.url(), title: await state.page.title() }, await meta(session, state));
  }
  if (operation === "browser_back") {
    await state.page.goBack({ waitUntil: "domcontentloaded" });
    await audit(session, operation, { tabId: state.id, origin: originOf(state.page.url()) });
    return response({ url: state.page.url(), title: await state.page.title() }, await meta(session, state));
  }
  if (operation === "browser_snapshot") return observe(session, state, args);
  if (operation === "browser_query") {
    const role = args.role;
    const name = String(args.name ?? "").toLowerCase();
    const rows = (await scanPage(state)).filter((row) => (!role || row.role === role) && (!name || row.name.toLowerCase().includes(name)));
    return observe(session, state, args, rows);
  }
  if (operation === "browser_subtree") {
    const { locator } = await resolveTarget(session, state, args.target);
    const text = await locator.evaluate((root) => (root.innerText || root.textContent || "").replace(/\s+/g, " ").trim());
    const limited = bounded(text, args);
    return response({ text: limited.text, maxChars: limited.maxChars }, await meta(session, state, { truncated: limited.truncated, continuationCursor: limited.continuationCursor }));
  }
  if (["browser_click", "browser_fill", "browser_press", "browser_upload"].includes(operation)) return act(operation, args);
  if (operation === "browser_scroll") {
    const dx = Number(args.delta_x ?? args.deltaX ?? 0);
    const dy = Number(args.delta_y ?? args.deltaY ?? 700);
    if (args.target) await (await resolveTarget(session, state, args.target)).locator.evaluate((el, [x, y]) => el.scrollBy(x, y), [dx, dy]);
    else await state.page.mouse.wheel(dx, dy);
    await audit(session, operation, { tabId: state.id, origin: originOf(state.page.url()), dx, dy });
    return response({ ok: true }, await meta(session, state));
  }
  if (operation === "browser_screenshot") {
    const dir = path.join(session.dir, "screenshots");
    await fs.promises.mkdir(dir, { recursive: true, mode: 0o700 });
    const output = path.join(dir, `${Date.now()}-${state.id}.png`);
    await state.page.screenshot({ path: output, fullPage: Boolean(args.full_page ?? args.fullPage) });
    await audit(session, operation, { tabId: state.id, origin: originOf(state.page.url()), output });
    return response({ path: output }, await meta(session, state));
  }
  if (operation === "browser_download") {
    const { locator, descriptor } = await resolveTarget(session, state, args.target);
    if (consequential(descriptor)) throw new Error("confirmation_required: consequential downloads require Workshop approval");
    const [download] = await Promise.all([state.page.waitForEvent("download"), locator.click()]);
    const dir = path.join(session.dir, "downloads");
    await fs.promises.mkdir(dir, { recursive: true, mode: 0o700 });
    const output = path.join(dir, safeName(download.suggestedFilename()));
    await download.saveAs(output);
    await audit(session, operation, { tabId: state.id, origin: originOf(state.page.url()), output });
    return response({ path: output, suggestedFilename: download.suggestedFilename() }, await meta(session, state));
  }
  throw new Error(`unknown browser operation ${operation}`);
}

const input = readline.createInterface({ input: process.stdin, crlfDelay: Infinity });
input.on("line", async (line) => {
  let message;
  try {
    message = JSON.parse(line);
    const value = await handle(message.operation, message.arguments ?? {});
    process.stdout.write(`${JSON.stringify({ id: message.id, ok: true, response: value })}\n`);
  } catch (error) {
    process.stdout.write(`${JSON.stringify({ id: message?.id, ok: false, error: String(error?.message ?? error) })}\n`);
  }
});

async function shutdown() {
  await Promise.all([...sessions.values()].map((session) => session.context.close().catch(() => {})));
  process.exit(0);
}
process.on("SIGTERM", shutdown);
process.on("SIGINT", shutdown);
