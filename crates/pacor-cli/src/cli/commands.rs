//! Dispatch from a parsed command to its handler.

use anyhow::Result;
use tracing::Level;

use super::args::{CacheAction, Cli, Commands, LogLevel};
use super::handlers::{self, BuildRequest};
use crate::log::init_tracing;

/// Entry point for CLI execution.
pub fn execute_cli_command(cli: Cli) -> Result<()> {
    init_logging(&cli);

    match cli.subcommand {
        Commands::Build {
            target,
            format,
            use_existing_flutter_build,
            skip_cleanup,
        } => handlers::build(BuildRequest {
            target,
            formats: format,
            use_existing_flutter_build,
            skip_cleanup,
            command_output: cli.command_output,
            config_path: cli.config_path,
        }),

        Commands::Android {
            use_existing_flutter_build,
        } => handlers::build(BuildRequest {
            target: Some(pacor_core::platforms::Platform::Android),
            formats: Vec::new(),
            use_existing_flutter_build,
            skip_cleanup: false,
            command_output: cli.command_output,
            config_path: cli.config_path,
        }),

        Commands::Cache { action } => match action {
            CacheAction::Clean { dry_run } => handlers::cache_clean(dry_run),
        },

        Commands::Clean { target } => handlers::clean(target, cli.config_path.as_deref()),

        Commands::Check { target } => handlers::check(target, cli.config_path.as_deref()),

        Commands::Init { force_overwrite } => {
            handlers::init(force_overwrite, cli.config_path.as_deref())
        }

        Commands::Config { show_full_config } => {
            handlers::show(show_full_config, cli.config_path.as_deref())
        }
    }
}

fn init_logging(cli: &Cli) {
    let level: Level = match cli.log_level {
        LogLevel::Error => Level::ERROR,
        LogLevel::Warn => Level::WARN,
        LogLevel::Info => Level::INFO,
        LogLevel::Debug => Level::DEBUG,
        #[cfg(debug_assertions)]
        LogLevel::Trace => Level::TRACE,
    };
    init_tracing(level);
}
