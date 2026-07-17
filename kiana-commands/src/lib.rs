mod local_state;
pub mod registry;
pub mod types;

// Command modules
pub mod advisor;
pub mod audit;
pub mod auth;
pub mod auto_mode;
pub mod brief;
pub mod checkpoint;
pub mod checks;
pub mod clear;
pub mod commit;
pub mod compact;
pub mod completion;
pub mod config;
pub mod context;
pub mod cost;
pub mod diff;
pub mod doctor;
pub mod eda;
mod eda_netlist;
pub mod eval;
pub mod evidence;
pub mod exit;
pub mod export;
pub mod feedback;
pub mod help;
pub mod hooks;
pub mod init;
pub mod license;
pub mod login;
pub mod logout;
pub mod mcp;
pub mod memory;
pub mod model;
pub mod output_style;
pub mod permissions;
pub mod plugin;
pub mod plugin_commands;
pub mod project;
pub mod release;
pub mod reload_plugins;
pub mod report;
pub mod review;
pub mod session;
pub mod skills;
pub mod stats;
pub mod status;
#[allow(dead_code)]
mod swarm_process_identity;
pub mod tasks;
pub mod theme;
pub mod trust;
pub mod usage;
pub mod validate;
pub mod version;
pub mod vim;

pub use registry::{create_default_command_registry, CommandRegistry};
pub use types::{
    Command, CommandContext, CommandResult, CommandRoute, CommandType, COMMAND_ARGV_APP_STATE_KEY,
};

#[cfg(test)]
pub(crate) fn test_env_lock() -> &'static std::sync::Mutex<()> {
    static LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
    LOCK.get_or_init(|| std::sync::Mutex::new(()))
}
