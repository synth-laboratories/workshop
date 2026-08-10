# Handoff — Whisper deps / Voice download (fix)

**Resolved:** Whisper now owns a dedicated managed environment at
`~/.synth-desktop/whisper/.venv` and uses one coherent runtime/model format:
`mlx-whisper` reading the downloaded Hugging Face snapshot directly.

## Symptom

Settings → Voice → Download shows:

```text
Failed to install Whisper dependencies:
`/Users/joshuapurtell/.synth-desktop/laguna/.venv/bin/python`: No module named pip
```

Models root (correct): `~/.synth-desktop/models/whisper`  
Broken step: install of `huggingface_hub` / `openai-whisper` into Laguna’s Python before HF snapshot download.

## Why it breaks

Whisper reuses **Laguna’s uv-managed venv**:

- Interpreter: `~/.synth-desktop/laguna/.venv/bin/python`  
  → symlink into `~/.local/share/uv/python/cpython-3.12-…`
- That venv often has **no `pip` module** (uv-created).
- Original Whisper code did `python -m pip install …` and failed hard.

Laguna download itself already assumes the same venv can `import huggingface_hub` (`laguna.rs` `download_model`). Whisper bolted a second, pip-centric installer on top instead of sharing Laguna’s real provisioning story.

## Superseded attempt

In [`src-tauri/src/whisper.rs`](src-tauri/src/whisper.rs) `ensure_python_deps`:

1. Prefer `uv pip install --python <venv> huggingface_hub openai-whisper` (+ best-effort `mlx-whisper` on macOS).
2. Else `python -m ensurepip --upgrade`, then `python -m pip install …`.

Also on the **dev machine**, `ensurepip` was run once by hand while diagnosing, so local pip may already exist — that does **not** mean the shipped path is correct for fresh users / Finder-launched apps (no Homebrew `uv` on `PATH`).

**Why this is the wrong product shape:**

- Couples Whisper to Laguna’s home + whatever happens to be on `PATH`.
- Finder / packaged app may not see Homebrew `uv`.
- Mutating Laguna’s venv for Whisper can surprise Laguna upgrades.
- Dual backends (`mlx-whisper` vs `openai-whisper`) + HF Transformers snapshot vs openai-whisper `.pt` cache mismatch is already noted in-file; dep install and inference formats need one coherent story.

## Chosen architecture

Pick **one** durable approach and document it in code + this handoff:

| Approach | Notes |
|---|---|
| **A. Dedicated Whisper env** | e.g. `~/.synth-desktop/whisper/.venv` via `uv` (or documented bootstrap), not Laguna’s. Cleanest isolation. |
| **B. Shared Laguna env, Laguna-owned** | Extend Laguna’s official env bootstrap so `huggingface_hub` (and any Whisper runtime) is always present; Whisper only *checks*, never invents pip. |
| **C. No Python at all** | whisper.cpp / mlx binary sidecar; download ggml/coreml weights only. Heavier, but matches “local STT” without venv drama. |

Approach **A** is implemented. The app creates the environment with an absolute
Python interpreter path (`SYNTH_WHISPER_BOOTSTRAP_PYTHON`, then Laguna's
interpreter, then `SYNTH_PYTHON`, then `/usr/bin/python3`). `python -m venv`
creates pip inside the new environment, after which all installation commands
target the dedicated interpreter explicitly. This does not depend on Finder's
shell `PATH`, does not require `uv` at runtime, and never mutates Laguna's venv.

The old `openai-whisper` fallback was removed because its `.pt` cache is not the
Transformers-format model shown as downloaded by the UI. On macOS,
`mlx-whisper` now consumes that exact downloaded directory.

UI contract to keep (already landed):

- Settings → Voice catalog: Tiny / Base (Recommended) / Small / Large v3 Turbo  
- Mic: no selected model → Settings Voice; else record → `synthWhisper.transcribe` / `transcribeAudio` → composer text  
- Bridge: `window.synthWhisper` in [`desktopBridge.ts`](src/renderer/src/runtime/desktopBridge.ts)  
- Commands: `whisper_models_*`, `whisper_transcribe`, `whisper_transcribe_base64` in [`whisper.rs`](src-tauri/src/whisper.rs)

## Related surfaces (already merged; don’t re-litigate unless broken)

- Slash menu + skills: Composer `/` + [`SlashCommandMenu.tsx`](src/renderer/src/components/SlashCommandMenu.tsx); skills via `skills_list` / `window.synthSkills`
- Voice UI: [`VoiceRecognitionSettings.tsx`](src/renderer/src/components/VoiceRecognitionSettings.tsx), Settings nav `voice`
- Tests: [`tests/playwright/slash-voice.spec.ts`](tests/playwright/slash-voice.spec.ts) (mocked bridge; does **not** cover real pip/uv)

## Confirm checklist

1. Fresh machine **or** wipe/recreate env under the chosen root — Download Base succeeds without “No module named pip”.
2. Packaged / Finder launch (no interactive shell `PATH`) still works.
3. Mic → local transcript inserts into composer with selected model.
4. Laguna still downloads/runs after Whisper install (no venv corruption).
5. Revert or replace the opportunistic `ensurepip`/`uv` patch once the real path lands; leave an honest error if bootstrap isn’t ready.

## Non-goals for this fix

- Fake “Ready” Whisper UI without a real runtime.
- Synth cloud STT API.
- Shipping Poolside’s exact whisper sizes/labels if our weight format differs — honesty over pixel parity.

## Context pointer

Plan + parallel agent work: slash + full local STT. Voice depth locked to **download + select + mic → local Whisper → text**. Dep install is the remaining correctness hole.
