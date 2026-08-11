# Updates and release channels

## What v0.1 ships

v0.1 has **no in-app updater**. It ships a passive version check:

- On opening Settings → About, the app fetches `https://usesynth.ai/releases/{channel}/latest.json` (5s timeout) and compares `version` against its own.
- If newer, About shows an "Update available · v{X}" affordance that opens the fixed public download page. Updating = downloading and reinstalling.
- The manifest is trusted **only for the version number**. The download action always opens `https://usesynth.ai/download`; a poisoned manifest cannot redirect the install source.
- Network failure, a missing manifest, or an unparsable version all read as "no update known". There is no error surface and no nag.
- Implementation: `src-tauri/src/update_check.rs` (`update_status` / `update_open_download` commands), `window.synthUpdates` bridge, About card in `SettingsPage.tsx`.

### Manifest contract (per channel)

`GET https://usesynth.ai/releases/stable/latest.json`

```json
{ "version": "0.1.1" }
```

Extra fields are ignored today. Hosting this file is a deploy-time task and a Gate F prerequisite once the passive check ships — an absent manifest is safe (reads as no update) but wastes the feature.

## Channels: stable and nightly

- **Channel is baked at build time** via `SYNTH_DESKTOP_CHANNEL` (default `stable`) and shown in About's build line. A build only ever reads its own channel's manifest. There is no in-app channel switching — switching is install-the-other-app.
- **Nightly is a separate app**: distinct bundle identifier (`com.synth.desktop.nightly`), its own app-data directory. This is a hard requirement, not a preference: schema migrations are one-way (migration 8 drops the legacy table after import), so a nightly must never open — and silently upgrade — a stable profile's database.
- **Versioning:** stable `0.1.x`; nightly `0.2.0-nightly.YYYYMMDD`. The version comparator understands prerelease ordering (a release outranks its own prereleases; nightlies order by date).
- **Gates per channel:** only stable passes Gate F/P and the 37-item manual matrix. Nightly is still signed/notarized and passes the evals secret-scan gate, but is exempt from the full matrix (recorded in GATE_SEQUENCE.md).
- **Update policy:** stable manifests only ever advertise stable versions; nightly manifests advertise nightlies. Promoting a nightly to stable is a stable-channel release, not a channel hop.

## Post-friends: the real in-app updater

First workstream after the friends release (prerequisite for public launch — "re-download to update" plus a billing product is unacceptable the first time a billing fix must ship fast):

- `tauri-plugin-updater` with signed update artifacts (`createUpdaterArtifacts`), per-channel manifest endpoints, background download, and a "Restart to update" affordance.
- A dedicated updater signing keypair, separate from Apple signing. Custody matters: losing it strands every installed app on manual updates forever. Key generation, storage, and the release-signing step must be written into LAUNCH_OPS before the plugin lands.
- Once the updater exists, Rehearsal F's upgrade step tests the actual updater path (prior seeded install → in-app update → data intact), not reinstall.
- WP8 packaging then produces updater artifacts alongside the `.dmg`, and the gate binds receipts to both.

## Rollback semantics

The Tauri updater never downgrades (it compares versions), and the passive check inherits the same rule. Rolling back a bad stable release therefore means: build a **higher-versioned** release from the reverted source (e.g. bad `0.1.2` → publish `0.1.3` = `0.1.1`'s tree), repoint `latest.json`, and replace the download artifact. Download-removal alone only protects new installs.

Downgrade guard: an older build refuses (fails closed) a database stamped with a newer schema version rather than misreading it. This is the migration-safety complement to the no-downgrade rule.
