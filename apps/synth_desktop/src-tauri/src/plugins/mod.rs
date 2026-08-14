//! Built-in Workshop product plugins. Optimizers is the first registered module.

mod policy;
mod registry;
mod service;
mod types;

pub use registry::{optimizers_plugin_enabled, PluginRegistry};
pub(crate) use service::PluginService;
pub use types::{PluginNotReady, PluginStatus, OPTIMIZERS_PLUGIN_ID};
