//! One table of what Desktop claims about each local optimizer runtime.
//!
//! These facts used to be restated in five places — `catalog_spec()`, the
//! install manifest writer, the plugin catalog entry, the channel-version
//! constants, and the in-process test fake — with no test asserting they
//! agreed. Nothing was actually contradictory, but a version had no single
//! place to be read or changed, which is why the version floor had nowhere to
//! live and why "which runtime is installed?" cost a source dive.
//!
//! Two things this deliberately is not:
//!
//! - **Not evidence about a runtime.** Everything here is Desktop's own claim.
//!   `templates` and `bounded_recipes` are host vocabulary that no runtime is
//!   asked to confirm, and the version fields say what Desktop expects to find,
//!   not what is installed. The runtime-authored half comes from the capability
//!   handshake, and only that half proves anything.
//! - **Not the app version.** That belongs to the build; read it from
//!   `tauri.conf.json` rather than restating a number here.
//!
//! The struct rendered in Settings → About is this struct. A hand-maintained
//! list over there would be a sixth copy and the drift would be invisible again.

/// The Desktop build's own version, owned by `Cargo.toml`.
///
/// Never restate this as a literal. Six MCP servers used to carry their own
/// hardcoded version — four of them still reporting `0.1.0` long after the app
/// reached 0.4 — because there was nothing to point at.
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Release channels a runtime can be pinned to. The host installs a different
/// version per channel, so a single floor says nothing about the other one.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReleaseChannel {
    Official,
    Dev,
}

/// What Desktop expects of one local optimizer runtime.
#[derive(Clone, Copy, Debug)]
pub struct RuntimeContract {
    /// Stable id used by the plugin surface and the registry.
    pub runtime_id: &'static str,
    /// Distribution name, where one exists.
    pub package: &'static str,
    /// Pinned version on the official channel.
    pub official: &'static str,
    /// Pinned version on the dev channel.
    pub dev: &'static str,
    /// Oldest version Desktop will install or select on the official channel.
    ///
    /// A floor is install-time UX, not a safety property: it explains a refusal
    /// before a download rather than after a failed handshake. The handshake
    /// remains the gate.
    pub min_supported: &'static str,
    /// Same, for the dev channel.
    ///
    /// Separate because the channels are cut independently, and a floor set
    /// from the official line would make the dev channel uninstallable every
    /// time official moved first — turning install-time UX into an outage for
    /// anyone tracking dev. When a dev cut lags, the handshake is what refuses
    /// it, which is the gate that was always doing the real work.
    pub min_supported_dev: &'static str,
    /// Child/host ownership handshake generation.
    pub ownership_protocol: u8,
    /// Workshop release line this runtime is pinned against.
    pub workshop_compat: &'static str,
    /// Algorithm ids Desktop expects this runtime to serve. The runtime's own
    /// claim is what gets enforced; this is the expectation it is checked for.
    pub algorithms: &'static [&'static str],
    /// Visual template ids. Host vocabulary — never requested from a runtime.
    pub templates: &'static [&'static str],
    /// Recipe ids Desktop offers. Host vocabulary — never requested either.
    pub bounded_recipes: &'static [&'static str],
    pub recipe_schema: &'static str,
    /// Whether Desktop installs and version-manages this runtime at all.
    ///
    /// `false` means there is no installer to hang a manifest, digest, or floor
    /// on, so the version fields are aspirational and About must say so rather
    /// than print a number nobody can verify.
    pub provisioned_by_desktop: bool,
}

impl RuntimeContract {
    pub fn version_for(&self, channel: ReleaseChannel) -> &'static str {
        match channel {
            ReleaseChannel::Official => self.official,
            ReleaseChannel::Dev => self.dev,
        }
    }

    /// The floor that applies to a version, inferred from its own shape.
    ///
    /// A `.dev` suffix is what makes a build a dev-channel build — the publish
    /// workflow enforces exactly that — so the version says which floor it is
    /// answerable to without threading channel state through every call site.
    pub fn floor_for(&self, version: &str) -> &'static str {
        if version.contains(".dev") {
            self.min_supported_dev
        } else {
            self.min_supported
        }
    }

    /// Is `version` at or above the floor for its own channel?
    ///
    /// Compared as dotted numeric segments so `0.2.10` outranks `0.2.9`; a
    /// lexical compare gets that backwards. Any trailing pre-release suffix
    /// (`0.2.9.dev20260814`) contributes its leading digits and then stops, so
    /// a dev build of a release sorts alongside it rather than below every
    /// numeric version.
    pub fn meets_floor(&self, version: &str) -> bool {
        !version_is_older(version, self.floor_for(version))
    }
}

fn version_is_older(candidate: &str, floor: &str) -> bool {
    let candidate = numeric_segments(candidate);
    let floor = numeric_segments(floor);
    let width = candidate.len().max(floor.len());
    for index in 0..width {
        let left = candidate.get(index).copied().unwrap_or(0);
        let right = floor.get(index).copied().unwrap_or(0);
        if left != right {
            return left < right;
        }
    }
    false
}

fn numeric_segments(version: &str) -> Vec<u64> {
    version
        .split('.')
        .map(|segment| {
            let digits: String = segment.chars().take_while(char::is_ascii_digit).collect();
            digits.parse().unwrap_or(0)
        })
        .collect()
}

/// The Desktop-managed GEPA sidecar.
pub const OPTIMIZERS: RuntimeContract = RuntimeContract {
    runtime_id: "optimizers",
    package: "synth-optimizers",
    official: "0.2.19",
    // Behind official: this cut predates both required routes. It still
    // installs — its own channel's floor is what it is measured against — and
    // then fails the handshake, which is the honest place for that failure.
    // Blocking the install instead would take the dev channel offline to
    // report a problem the gate already reports precisely.
    dev: "0.2.9.dev20260814",
    // 0.2.19 carries ownership protocol v2 and preserves
    // the required routes and legacy-workspace migration, and identifies the
    // running Rust service with the same version as the Python distribution.
    min_supported: "0.2.19",
    // No dev cut carries the required routes yet; the handshake refuses one
    // that cannot serve them. Raise this when the dev channel is cut again.
    min_supported_dev: "0.2.9.dev20260814",
    ownership_protocol: 2,
    workshop_compat: "0.4.0",
    algorithms: &["gepa", "sft", "cispo"],
    templates: &[
        "optimizer.gepa.live.v1",
        "optimizer.sft.live.v1",
        "optimizer.run.v1",
    ],
    bounded_recipes: &[
        "sft.qwen35-2b.mlx.v1",
        "cispo.banking77.mlx.v1",
        "cispo.slime.hosted.v1",
    ],
    recipe_schema: "gepa.recipe.v1",
    provisioned_by_desktop: true,
};

/// The local container-evaluation runtime.
///
/// Desktop provisions it from the same 0.2.19 `synth-optimizers` install as
/// GEPA, writing a digest-pinned manifest under `data_root()/runtime/eval`.
pub const EVAL: RuntimeContract = RuntimeContract {
    runtime_id: "eval",
    package: "synth-optimizers[eval]",
    official: "0.2.19",
    dev: "0.2.19",
    min_supported: "0.2.19",
    min_supported_dev: "0.2.19",
    ownership_protocol: 2,
    workshop_compat: "0.4.0",
    algorithms: &["eval"],
    templates: &["optimizer.eval.live.v1", "optimizer.run.v1"],
    bounded_recipes: &[
        "eval.craftax.code-policy.smoke.v1",
        "eval.gamebench.craftax-code-policy.confirm.v1",
        "eval.gamebench.llm-policy.confirm.v1",
        "eval.mlx.local-policy.smoke.v1",
    ],
    recipe_schema: "eval.worker-manifest.v1",
    provisioned_by_desktop: true,
};

pub const ALL: &[RuntimeContract] = &[OPTIMIZERS, EVAL];

/// One About row. Serialised from the same table the code enforces — a
/// hand-maintained list in the renderer would be another copy, and the drift it
/// exists to expose would be invisible again.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeContractView {
    pub runtime_id: String,
    pub package: String,
    /// Version found on disk, when Desktop manages the install.
    pub installed: Option<String>,
    /// Version Desktop pins for the active channel.
    pub expected: String,
    pub min_supported: String,
    pub ownership_protocol: u8,
    pub release_channel: String,
    pub workshop_compat: String,
    pub algorithms: Vec<String>,
    /// False when the installed version is below the floor, or absent.
    pub meets_floor: bool,
    /// False when nothing in Desktop installs this runtime, so `expected` is a
    /// statement of intent rather than a fact and About should say so.
    pub managed: bool,
}

impl RuntimeContract {
    pub fn view(&self, channel: ReleaseChannel, installed: Option<String>) -> RuntimeContractView {
        let meets_floor = installed
            .as_deref()
            .map(|version| self.meets_floor(version))
            .unwrap_or(!self.provisioned_by_desktop);
        RuntimeContractView {
            runtime_id: self.runtime_id.into(),
            package: self.package.into(),
            installed,
            expected: self.version_for(channel).into(),
            min_supported: self.min_supported.into(),
            ownership_protocol: self.ownership_protocol,
            release_channel: match channel {
                ReleaseChannel::Official => "official".into(),
                ReleaseChannel::Dev => "dev".into(),
            },
            workshop_compat: self.workshop_compat.into(),
            algorithms: self.algorithms.iter().map(|id| (*id).to_owned()).collect(),
            meets_floor,
            managed: self.provisioned_by_desktop,
        }
    }
}

