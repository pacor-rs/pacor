//! CLI command execution and dispatch.
//!
//! This module serves as the entry point for CLI command execution,
//! initializing logging and dispatching to appropriate command handlers.

use anyhow::Result;
use colored::Colorize;
use tracing::Level;

use crate::cli::args::{Cli, CommandOutputMode, Commands};
use crate::cli::handlers::{
    BuildContext, handle_android_command, handle_build_command, handle_check_command,
    handle_clean_command, handle_config_command, handle_init_command,
};
use crate::error::PlatformError;
use crate::log::init_tracing;

/// Entry point for CLI command execution.
///
/// Initializes logging based on debug flag and dispatches to appropriate command handlers.
/// This function serves as a bridge between the legacy command structure and the new
/// modular handler architecture.
pub fn execute_cli_command(cli_args: Cli) -> Result<()> {
    initialize_logging(&cli_args)?;

    let target_os = std::env::consts::OS;
    if target_os != "windows" && target_os != "linux" && target_os != "macos" {
        return Err(PlatformError::UnsupportedPlatform {
            platform: target_os.to_string(),
        }
        .into());
    }

    match &cli_args.subcommand {
        Commands::Build {
            use_existing_flutter_build,
            skip_cleanup,
        } => {
            let build_context = BuildContext::new(
                cli_args.command_output,
                *use_existing_flutter_build,
                *skip_cleanup,
            );
            handle_build_command(build_context)
        }

        Commands::Android {
            use_existing_flutter_build,
            sign_apk: should_sign_apk,
        } => {
            let build_context = BuildContext::new(
                cli_args.command_output,
                *use_existing_flutter_build,
                false, // Android builds don't use skip_cleanup
            );
            handle_android_command(build_context, *should_sign_apk)
        }

        Commands::Clean => handle_clean_command(),

        Commands::Check => handle_check_command(),

        Commands::Init {
            force_overwrite: should_force_overwrite,
        } => handle_init_command(*should_force_overwrite),

        Commands::Config {
            show_full_config: should_show_full_config,
        } => handle_config_command(*should_show_full_config),
    }
}

/// Initialize logging system based on CLI arguments.
fn initialize_logging(cli_args: &Cli) -> Result<()> {
    let log_level: Level = (&cli_args.log_level).into();
    init_tracing(log_level);

    if cli_args.command_output == CommandOutputMode::Verbose {
        println!(
            "Log level set to {}",
            format!("{}", log_level).blue().bold()
        );
    }

    Ok(())
}
