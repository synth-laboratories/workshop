import { spawn } from "node:child_process";

const children = new Set();
let stopping = false;

function start(command, args, options = {}) {
  const child = spawn(command, args, {
    stdio: "inherit",
    shell: false,
    ...options,
  });
  children.add(child);
  child.on("exit", () => children.delete(child));
  return child;
}

function stop(exitCode = 0) {
  if (stopping) return;
  stopping = true;
  for (const child of children) {
    child.kill("SIGTERM");
  }
  setTimeout(() => process.exit(exitCode), 150).unref();
}

process.on("SIGINT", () => stop(130));
process.on("SIGTERM", () => stop(143));

// Real app entry — the Tauri host supervises/probes the local runtime.
const desktop = start("npm", [
  "run",
  "dev",
  "--workspace",
  "@synth/synth-desktop",
]);

desktop.on("exit", (code) => {
  if (!stopping) stop(code ?? 0);
});
