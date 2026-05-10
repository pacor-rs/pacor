//! Command-line argument parsing and validation for the Pacor packaging tool.
//!
//! This module defines the CLI structure, available commands, and their parameters
//! using the `clap` crate for robust argument parsing and validation.

use clap::{Parser, Subcommand, ValueEnum};

#[derive(ValueEnum, Clone, Debug)]
pub(crate) enum LogLevel {
    /// Error level logging
    #[value(name = "error")]
    Error,
    /// Warning level logging
    #[value(name = "warn")]
    Warn,
    /// Info level logging
    #[value(name = "info")]
    Info,
    /// Debug level logging
    #[value(name = "debug")]
    Debug,
    /// Trace level logging, only available in debug builds
    #[cfg(debug_assertions)]
    #[value(name = "trace")]
    Trace,
}

impl From<&LogLevel> for tracing::Level {
    fn from(level: &LogLevel) -> Self {
        match level {
            LogLevel::Error => tracing::Level::ERROR,
            LogLevel::Warn => tracing::Level::WARN,
            LogLevel::Info => tracing::Level::INFO,
            LogLevel::Debug => tracing::Level::DEBUG,
            #[cfg(debug_assertions)]
            LogLevel::Trace => tracing::Level::TRACE,
        }
    }
}

/// Command-line interface for the Flutter cross-platform packaging tool
#[derive(Parser)]
#[command(name = env!("CARGO_PKG_NAME"))]
#[command(author = "thep0y")]
#[command(version = env!("CARGO_PKG_VERSION"))]
#[command(about = "Cross-platform packaging tool for Flutter applications")]
#[command(
    long_about = "A powerful packaging tool for Flutter applications that supports generating \
                  installation packages in multiple formats including macOS DMG, Windows MSI/NSIS, \
                  Linux AppImage/DEB, Android APK, and more."
)]
pub struct Cli {
    /// Subcommand to execute
    #[command(subcommand)]
    pub(crate) subcommand: Commands,

    /// Control how command output is displayed
    #[arg(
        long = "command-output",
        value_enum,
        default_value = "none",
        help = "Control command output display: none (default), normal, or verbose"
    )]
    pub(crate) command_output: CommandOutputMode,

    /// Set the logging level
    #[arg(
        short,
        long,
        value_enum,
        default_value = "warn",
        help = "Set the logging verbosity level"
    )]
    pub(crate) log_level: LogLevel,

    /// Path to the configuration file
    #[arg(
        short,
        long,
        default_value = "pacor.toml",
        value_parser = clap::value_parser!(std::path::PathBuf)
    )]
    pub(crate) config_path: std::path::PathBuf,
}

/// Available subcommands for the packaging tool
#[derive(Subcommand)]
pub(crate) enum Commands {
    /// Build installation package for the current platform
    Build {
        /// Use existing Flutter build artifacts (skip Flutter build step)
        ///
        /// When enabled, the packaging process will use previously built Flutter artifacts
        /// instead of rebuilding them. This option is not supported on Linux platform,
        /// as Linux requires modifying Flutter configuration files to ensure correct
        /// package directory structure.
        #[arg(long)]
        use_existing_flutter_build: bool,

        /// Skip cleanup of temporary files after build
        #[arg(long)]
        skip_cleanup: bool,
    },

    /// Build Android APK package
    Android {
        /// Use existing Flutter build artifacts (skip Flutter build step)
        #[arg(long)]
        use_existing_flutter_build: bool,

        /// Enable APK signing with configured keystore
        #[arg(long)]
        sign_apk: bool,
    },

    /// Clean build output and temporary files
    Clean,

    /// Verify build environment and dependencies
    Check,

    /// Initialize configuration file
    Init {
        /// Force overwrite of existing configuration file
        #[arg(long)]
        force_overwrite: bool,
    },

    /// Display current configuration
    Config {
        /// Show complete configuration including default values
        #[arg(long)]
        show_full_config: bool,
    },
}

#[derive(Debug, ValueEnum, Clone, Default, PartialEq)]
pub(crate) enum CommandOutputMode {
    /// Don't show any command output
    #[default]
    None,
    /// Show basic command output (stdout/stderr)
    Normal,
    /// Show detailed command output including command line, exit status, etc.
    Verbose,
}
