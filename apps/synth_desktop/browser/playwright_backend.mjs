#!/usr/bin/env node
import crypto from "node:crypto";
import fs from "node:fs";
import { createRequire } from "node:module";
import os from "node:os";
import path from "node:path";
import readline from "node:readline";

const require = createRequire(import.meta.url);
const playwrightModule = process.env.SYNTH_BROWSER_RUNTIME_ROOT
  ? path.join(process.env.SYNTH_BROWSER_RUNTIME_ROOT, "node_modules", "playwright")
  : "playwright";
const { chromium } = require(playwrightModule);

const PROTOCOL_VERSION = "workshop.browser.v1";
const DEFAULT_MAX_CHARS = 16_000;
const HARD_MAX_CHARS = 20_000;
const sessions = new Map();
const pendingActions = new Map();
const blockedNavigations = new WeakMap();
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

function uploadRoots() {
  const roots = new Set((process.env.SYNTH_BROWSER_UPLOAD_ROOTS ?? "").split(path.delimiter).filter(Boolean));
  const policyFile = process.env.SYNTH_BROWSER_POLICY_FILE;
  if (policyFile) {
    try {
      const policy = JSON.parse(fs.readFileSync(policyFile, "utf8"));
      for (const root of policy.uploadRoots ?? []) roots.add(String(root));
    } catch (error) {
      if (error?.code !== "ENOENT") throw new Error(`browser_policy_invalid: ${error.message}`);
    }
  }
  return [...roots].map((root) => fs.realpathSync(root));
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
  const state = { id: id("tab"), page, nav: 0, actionRevision: 0, refs: new Map(), refRevision: "", lastKnownRevision: "", dialogs: new Map(), closed: false, owned: true };
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
    const dialogId = id("dialog");
    state.dialogs.set(dialogId, { dialog, createdAt: Date.now(), documentRevision: state.lastKnownRevision });
    await audit(session, "dialog.opened", { tabId: state.id, dialogId, type: dialog.type(), message: dialog.message().slice(0, 500) });
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

async function installNavigationGuard(scope) {
  const handler = async (route) => {
    const request = route.request();
    const frame = request.frame();
    if (request.isNavigationRequest() && (!frame || frame === frame.page().mainFrame())) {
      const url = request.url();
      if (url !== "about:blank" && !url.startsWith("data:")) {
        try { requireAllowed(url); }
        catch {
          if (frame) blockedNavigations.set(frame.page(), url);
          await route.abort("blockedbyclient");
          return;
        }
      }
    }
    await route.continue();
  };
  await scope.route("**/*", handler);
  return handler;
}

async function revision(state) {
  const mutation = await state.page.evaluate(() => Number(globalThis.__workshopMutationRevision ?? 0)).catch(() => 0);
  const digest = crypto.createHash("sha256").update(state.page.url()).digest("hex").slice(0, 10);
  state.lastKnownRevision = `${state.nav}.${mutation}.${state.actionRevision}.${digest}`;
  return state.lastKnownRevision;
}

async function meta(session, state, extra = {}) {
  const documentRevision = extra.documentRevision ?? await revision(state);
  return {
    sessionId: session.id,
    tabId: state.id,
    documentRevision,
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

function consequential(operation, descriptor, args) {
  if (operation === "browser_upload") return true;
  if (operation === "browser_press" && /^(enter|return)$/i.test(String(args.key ?? ""))) return true;
  return /\b(send|publish|purchase|buy|delete|remove|submit|confirm|place order|transfer|pay|checkout|post|share)\b/i.test(String(descriptor?.name ?? "").trim());
}

async function act(operation, args, prepared = false) {
  const session = getSession(args);
  const state = getTab(session, args);
  if (operation === "browser_press" && !args.target) {
    if (!prepared && process.env.SYNTH_BROWSER_REQUIRE_HOST_APPROVAL === "1") {
      throw new Error("host_preparation_required: browser key presses must pass Workshop approval");
    }
    await state.page.keyboard.press(String(args.key ?? ""));
    await audit(session, operation, { tabId: state.id, origin: originOf(state.page.url()), key: args.key });
    return response({ ok: true }, await meta(session, state));
  }
  const { locator, descriptor } = await resolveTarget(session, state, args.target);
  if (!prepared && process.env.SYNTH_BROWSER_REQUIRE_HOST_APPROVAL === "1") {
    throw new Error("host_preparation_required: browser actions must pass Workshop approval");
  }
  if (!prepared && consequential(operation, descriptor, args) && process.env.SYNTH_BROWSER_ALLOW_CONSEQUENTIAL !== "1") {
    throw new Error(`confirmation_required: ${descriptor.name}; approve this exact action in Workshop`);
  }
  let suspendedRevision = null;
  if (operation === "browser_click") {
    blockedNavigations.delete(state.page);
    const clickRevision = await revision(state);
    let removeDialogListener = () => {};
    const dialogOpened = new Promise((resolve) => {
      const listener = () => {
        state.page.off("dialog", listener);
        resolve(true);
      };
      removeDialogListener = () => state.page.off("dialog", listener);
      state.page.on("dialog", listener);
    });
    const click = locator.click();
    const pausedOnDialog = await Promise.race([click.then(() => false), dialogOpened]);
    removeDialogListener();
    if (pausedOnDialog) {
      click.catch(() => {});
      suspendedRevision = clickRevision;
    }
    const blocked = blockedNavigations.get(state.page);
    if (blocked) throw new Error(`navigation_blocked: ${originOf(blocked)} is not approved`);
    if (/^https?:/.test(state.page.url())) requireAllowed(state.page.url());
  }
  else if (operation === "browser_fill") await locator.fill(String(args.value ?? ""));
  else if (operation === "browser_press") await locator.press(String(args.key ?? ""));
  else if (operation === "browser_upload") {
    const roots = uploadRoots();
    const files = (args.file_paths ?? args.filePaths ?? []).map((file) => fs.realpathSync(file));
    if (!files.length || !roots.length || files.some((file) => !roots.some((root) => file === root || file.startsWith(`${root}${path.sep}`)))) {
      throw new Error("upload_refused: select explicit files from a Workshop-approved upload root");
    }
    await locator.setInputFiles(files);
  }
  // MutationObserver delivery can lag behind Playwright resolving an input
  // action. Fail closed synchronously: every successful input invalidates all
  // previously issued element refs even when the page appears unchanged.
  state.actionRevision += 1;
  await audit(session, operation, { tabId: state.id, origin: originOf(state.page.url()), role: descriptor.role, name: descriptor.name });
  return response({ ok: true }, await meta(session, state, suspendedRevision ? { documentRevision: suspendedRevision } : {}));
}

async function prepareAction(args) {
  const operation = String(args.operation ?? "");
  if (!["browser_click", "browser_fill", "browser_press", "browser_upload", "browser_download", "browser_scroll", "browser_handle_dialog"].includes(operation)) {
    throw new Error(`unsupported prepared action ${operation}`);
  }
  const actionArgs = args.arguments ?? {};
  const session = getSession(actionArgs);
  const state = getTab(session, actionArgs);
  let descriptor = { role: "page", name: operation === "browser_press" ? String(actionArgs.key ?? "") : "page" };
  let documentRevision;
  if (operation === "browser_handle_dialog") {
    const pending = state.dialogs.get(String(actionArgs.dialog_id ?? actionArgs.dialogId ?? ""));
    if (!pending) throw new Error("stale_dialog: dialog is unknown or already handled");
    descriptor = { role: "dialog", name: pending.dialog.message().slice(0, 500) };
    documentRevision = pending.documentRevision;
  } else if (actionArgs.target) descriptor = (await resolveTarget(session, state, actionArgs.target)).descriptor;
  documentRevision ??= await revision(state);
  const actionToken = id("action");
  for (const [existingToken, pending] of pendingActions) {
    if (pending.expiresAt < Date.now()) pendingActions.delete(existingToken);
  }
  if (pendingActions.size >= 1_000) throw new Error("too_many_pending_actions: close stale sessions and retry");
  pendingActions.set(actionToken, { operation, arguments: actionArgs, sessionId: session.id, tabId: state.id, documentRevision, expiresAt: Date.now() + 5 * 60_000 });
  const actionDetails = {};
  if (operation === "browser_press") actionDetails.key = String(actionArgs.key ?? "");
  if (operation === "browser_fill") {
    const value = String(actionArgs.value ?? "");
    actionDetails.valueLength = value.length;
    actionDetails.valueSha256 = crypto.createHash("sha256").update(value).digest("hex");
  }
  if (operation === "browser_upload") actionDetails.filePaths = [...(actionArgs.file_paths ?? actionArgs.filePaths ?? [])].map(String);
  if (operation === "browser_scroll") {
    actionDetails.deltaX = Number(actionArgs.delta_x ?? actionArgs.deltaX ?? 0);
    actionDetails.deltaY = Number(actionArgs.delta_y ?? actionArgs.deltaY ?? 700);
  }
  if (operation === "browser_handle_dialog") {
    actionDetails.accept = Boolean(actionArgs.accept);
    const prompt = String(actionArgs.prompt_text ?? actionArgs.promptText ?? "");
    if (prompt) actionDetails.promptTextSha256 = crypto.createHash("sha256").update(prompt).digest("hex");
  }
  return response({
    actionToken,
    consequential: operation === "browser_handle_dialog" ? Boolean(actionArgs.accept) : consequential(operation, descriptor, actionArgs),
    role: descriptor.role,
    name: descriptor.name,
    origin: originOf(state.page.url()),
    tabId: state.id,
    documentRevision,
    actionDetails,
  }, await meta(session, state, { documentRevision }));
}

async function commitAction(args) {
  const token = String(args.action_token ?? args.actionToken ?? "");
  const pending = pendingActions.get(token);
  pendingActions.delete(token);
  if (!pending) throw new Error("stale_action_token: action token is unknown or already used");
  if (pending.expiresAt < Date.now()) throw new Error("stale_action_token: action approval expired");
  const session = getSession({ session_id: pending.sessionId });
  const state = getTab(session, { tab_id: pending.tabId });
  if (pending.operation === "browser_handle_dialog") {
    const dialogId = String(pending.arguments.dialog_id ?? pending.arguments.dialogId ?? "");
    const entry = state.dialogs.get(dialogId);
    state.dialogs.delete(dialogId);
    if (!entry) throw new Error("stale_dialog: dialog is unknown or already handled");
    if (entry.documentRevision !== pending.documentRevision) throw new Error("stale_action_token: document changed after dialog action preparation");
    if (pending.arguments.accept) await entry.dialog.accept(String(pending.arguments.prompt_text ?? pending.arguments.promptText ?? ""));
    else await entry.dialog.dismiss();
    state.actionRevision += 1;
    await audit(session, pending.operation, { tabId: state.id, dialogId, accepted: Boolean(pending.arguments.accept), approved: true });
    return response({ handled: true, accepted: Boolean(pending.arguments.accept) }, await meta(session, state));
  }
  if (await revision(state) !== pending.documentRevision) throw new Error("stale_action_token: document changed after action preparation");
  if (pending.operation === "browser_download") {
    const { locator, descriptor } = await resolveTarget(session, state, pending.arguments.target);
    const [download] = await Promise.all([state.page.waitForEvent("download"), locator.click()]);
    const dir = path.join(session.dir, "downloads");
    await fs.promises.mkdir(dir, { recursive: true, mode: 0o700 });
    const output = path.join(dir, safeName(download.suggestedFilename()));
    await download.saveAs(output);
    state.actionRevision += 1;
    await audit(session, pending.operation, { tabId: state.id, origin: originOf(state.page.url()), role: descriptor.role, name: descriptor.name, output, approved: true });
    return response({ path: output, suggestedFilename: download.suggestedFilename() }, await meta(session, state));
  }
  if (pending.operation === "browser_scroll") {
    const dx = Number(pending.arguments.delta_x ?? pending.arguments.deltaX ?? 0);
    const dy = Number(pending.arguments.delta_y ?? pending.arguments.deltaY ?? 700);
    if (pending.arguments.target) await (await resolveTarget(session, state, pending.arguments.target)).locator.evaluate((el, [x, y]) => el.scrollBy(x, y), [dx, dy]);
    else await state.page.mouse.wheel(dx, dy);
    state.actionRevision += 1;
    await audit(session, pending.operation, { tabId: state.id, origin: originOf(state.page.url()), dx, dy, approved: true });
    return response({ ok: true }, await meta(session, state));
  }
  return act(pending.operation, pending.arguments, true);
}

async function handle(operation, args = {}) {
  if (operation === "browser_prepare_action") return prepareAction(args);
  if (operation === "browser_commit_action") return commitAction(args);
  if (operation === "browser_claim_chrome") {
    if (process.env.SYNTH_BROWSER_ENABLE_CHROME_CLAIM !== "1") {
      throw new Error("chrome_claim_disabled: enable the explicit Workshop Chrome claim setting first");
    }
    const endpoint = new URL(String(args.cdp_endpoint ?? args.cdpEndpoint ?? "http://127.0.0.1:9222"));
    if (endpoint.protocol !== "http:" || !["127.0.0.1", "localhost", "::1"].includes(endpoint.hostname)) {
      throw new Error("chrome_claim_refused: CDP endpoint must be loopback HTTP");
    }
    let websocket;
    try {
      const discovery = await fetch(new URL("/json/version", endpoint.origin), {
        signal: AbortSignal.timeout(3_000),
      });
      if (!discovery.ok) throw new Error(`HTTP ${discovery.status}`);
      websocket = new URL(String((await discovery.json()).webSocketDebuggerUrl ?? ""));
    } catch (error) {
      throw new Error(`chrome_claim_unavailable: CDP discovery failed: ${error.message}`);
    }
    if (!["ws:", "wss:"].includes(websocket.protocol) || !["127.0.0.1", "localhost", "::1"].includes(websocket.hostname)) {
      throw new Error("chrome_claim_refused: CDP discovery returned a non-loopback WebSocket");
    }
    const browser = await chromium.connectOverCDP(websocket.href, { timeout: 5_000 });
    const contexts = browser.contexts();
    if (contexts.length !== 1) throw new Error(`chrome_claim_ambiguous: expected one Chrome context, found ${contexts.length}`);
    const context = contexts[0];
    const titleContains = String(args.title_contains ?? args.titleContains ?? "").toLowerCase();
    const urlContains = String(args.url_contains ?? args.urlContains ?? "").toLowerCase();
    if (!titleContains && !urlContains) throw new Error("chrome_claim_refused: specify title_contains or url_contains");
    const matches = [];
    for (const page of context.pages()) {
      const title = await page.title();
      if ((!titleContains || title.toLowerCase().includes(titleContains)) && (!urlContains || page.url().toLowerCase().includes(urlContains))) matches.push(page);
    }
    if (matches.length !== 1) throw new Error(`chrome_claim_ambiguous: expected exactly one matching tab, found ${matches.length}`);
    requireAllowed(matches[0].url());
    const navigationGuard = await installNavigationGuard(matches[0]);
    const profileName = "claimed-chrome";
    const dir = path.join(profileRoot(), profileName);
    await fs.promises.mkdir(dir, { recursive: true, mode: 0o700 });
    const session = { id: id("browser_session"), profileName, dir, context, connection: browser, claimed: true, claimedPage: matches[0], navigationGuard, tabs: new Map(), auditPath: path.join(dir, "audit.jsonl") };
    sessions.set(session.id, session);
    const state = stateForPage(session, matches[0]);
    state.owned = false;
    await audit(session, "chrome.claimed", { tabId: state.id, url: state.page.url(), endpoint: endpoint.origin });
    return response({ sessionId: session.id, profile: profileName, tabId: state.id, claimed: true }, await meta(session, state));
  }
  if (operation === "browser_create_session") {
    const profileName = safeName(args.profile ?? "default");
    const dir = path.join(profileRoot(), profileName);
    await fs.promises.mkdir(dir, { recursive: true, mode: 0o700 });
    const context = await chromium.launchPersistentContext(dir, {
      headless: process.env.SYNTH_BROWSER_HEADLESS === "1",
      executablePath: process.env.SYNTH_BROWSER_EXECUTABLE || chromium.executablePath(),
      acceptDownloads: true,
      downloadsPath: path.join(dir, "downloads"),
    });
    await installNavigationGuard(context);
    await installRevisionTracking(context);
    const session = { id: id("browser_session"), profileName, dir, context, connection: null, claimed: false, tabs: new Map(), auditPath: path.join(dir, "audit.jsonl") };
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
    if (session.claimed) {
      await Promise.all([...session.tabs.values()].filter((tab) => tab.owned && !tab.page.isClosed()).map((tab) => tab.page.close().catch(() => {})));
      if (!session.claimedPage.isClosed()) await session.claimedPage.unroute("**/*", session.navigationGuard).catch(() => {});
    } else await session.context.close();
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
    if (session.claimed) await installNavigationGuard(page);
    const state = stateForPage(session, page);
    if (args.url) { requireAllowed(args.url); await page.goto(args.url); }
    await audit(session, "tab.created", { tabId: state.id, url: page.url() });
    return response({ tabId: state.id, url: page.url() }, await meta(session, state));
  }
  const state = getTab(session, args);
  if (operation === "browser_list_dialogs") {
    const dialogs = [];
    for (const [dialogId, entry] of state.dialogs) dialogs.push({ dialogId, type: entry.dialog.type(), message: entry.dialog.message().slice(0, 500), hasDefaultValue: Boolean(entry.dialog.defaultValue()) });
    return response({ dialogs }, await meta(session, state, { documentRevision: state.lastKnownRevision }));
  }
  if (operation === "browser_audit") {
    const limit = Math.max(1, Math.min(Number(args.limit ?? 100), 500));
    let lines = [];
    try { lines = (await fs.promises.readFile(session.auditPath, "utf8")).trim().split("\n").filter(Boolean).slice(-limit).map((line) => JSON.parse(line)); }
    catch (error) { if (error?.code !== "ENOENT") throw error; }
    return response({ events: lines }, await meta(session, state));
  }
  if (operation === "browser_close_tab") {
    if (session.claimed && !state.owned) throw new Error("claimed_tab_preserved: Workshop will not close the user's claimed Chrome tab");
    await audit(session, "tab.closed", { tabId: state.id, url: state.page.url() });
    await state.page.close();
    return response({ closed: true }, await meta(session, state).catch(() => ({ sessionId: session.id, tabId: state.id, documentRevision: "closed", origin: "closed", truncated: false, stale: false, continuationCursor: null })));
  }
  if (operation === "browser_navigate") {
    requireAllowed(args.url);
    blockedNavigations.delete(state.page);
    await audit(session, "navigation.requested", { tabId: state.id, from: state.page.url(), to: args.url });
    await state.page.goto(args.url, { waitUntil: "domcontentloaded" });
    const blocked = blockedNavigations.get(state.page);
    if (blocked) throw new Error(`navigation_blocked: ${originOf(blocked)} is not approved`);
    if (/^https?:/.test(state.page.url())) requireAllowed(state.page.url());
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
  if (operation === "browser_handle_dialog") throw new Error("host_preparation_required: dialogs must pass Workshop action preparation");
  if (operation === "browser_scroll") {
    if (process.env.SYNTH_BROWSER_REQUIRE_HOST_APPROVAL === "1") throw new Error("host_preparation_required: browser scroll must pass Workshop action preparation");
    const dx = Number(args.delta_x ?? args.deltaX ?? 0);
    const dy = Number(args.delta_y ?? args.deltaY ?? 700);
    if (args.target) await (await resolveTarget(session, state, args.target)).locator.evaluate((el, [x, y]) => el.scrollBy(x, y), [dx, dy]);
    else await state.page.mouse.wheel(dx, dy);
    state.actionRevision += 1;
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
    if (process.env.SYNTH_BROWSER_REQUIRE_HOST_APPROVAL === "1") throw new Error("host_preparation_required: browser downloads must pass Workshop approval");
    if (consequential(operation, descriptor, args)) throw new Error("confirmation_required: consequential downloads require Workshop approval");
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

let shuttingDown = false;
async function shutdown() {
  if (shuttingDown) return;
  shuttingDown = true;
  await Promise.all([...sessions.values()].filter((session) => !session.claimed).map((session) => session.context.close().catch(() => {})));
  process.exit(0);
}
input.on("close", shutdown);
process.on("SIGTERM", shutdown);
process.on("SIGINT", shutdown);
