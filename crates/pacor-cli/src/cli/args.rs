//! Command-line argument definitions.

use clap::builder::{PossibleValuesParser, TypedValueParser};
use clap::{Parser, Subcommand, ValueEnum};
use pacor_core::platforms::Platform;
use pacor_core::utils::command::CommandOutputMode;

/// Parser for `--target`.
///
/// The possible values come from [`Platform::ALL`], so adding a target only
/// requires touching the core enum. Using clap's possible-value parser (rather
/// than the bare `FromStr` impl) means the error lists the valid targets and
/// typos get a suggestion.
fn target_parser() -> impl TypedValueParser<Value = Platform> {
    PossibleValuesParser::new(Platform::ALL.map(|platform| platform.as_str())).map(|value| {
        value
            .parse::<Platform>()
            .expect("every possible value is a valid target")
    })
}

#[derive(ValueEnum, Clone, Debug)]
pub enum LogLevel {
    #[value(name = "error")]
    Error,
    #[value(name = "warn")]
    Warn,
    #[value(name = "info")]
    Info,
    #[value(name = "debug")]
    Debug,
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

/// Cross-platform packaging tool for Flutter applications.
#[derive(Parser)]
#[command(name = env!("CARGO_PKG_NAME"))]
#[command(author = "thep0y")]
#[command(version = env!("CARGO_PKG_VERSION"))]
#[command(about = "Cross-platform packaging tool for Flutter applications")]
pub struct Cli {
    /// Subcommand to execute.
    #[command(subcommand)]
    pub subcommand: Commands,

    /// Control how external command output is displayed.
    #[arg(
        long = "command-output",
        global = true,
        default_value = "none",
        value_name = "MODE",
        help = "Command output: none (default), normal or verbose"
    )]
    pub command_output: CommandOutputMode,

    /// Set the logging level.
    #[arg(short, long, global = true, value_enum, default_value = "warn")]
    pub log_level: LogLevel,

    /// Path to the configuration file (defaults to `pacor.toml` in the project root).
    #[arg(
        short,
        long = "config",
        global = true,
        value_parser = clap::value_parser!(std::path::PathBuf)
    )]
    pub config_path: Option<std::path::PathBuf>,
}

/// Actions for `pacor cache`.
#[derive(Subcommand)]
pub enum CacheAction {
    /// Remove every cached tool and template.
    ///
    /// Anything removed is downloaded again the next time it is needed. This
    /// covers the whole cache directory, including tools that are not tied to
    /// the pinned template tag.
    Clean {
        /// List what would be removed without deleting anything.
        #[arg(long)]
        dry_run: bool,
    },
}

/// Available subcommands.
#[derive(Subcommand)]
pub enum Commands {
    /// Build an installer for a target platform.
    Build {
        /// Target platform (defaults to the host platform).
        #[arg(long, value_name = "OS", value_parser = target_parser())]
        target: Option<Platform>,

        /// Override the configured output format(s), e.g. `--format deb`.
        #[arg(long, value_name = "FORMAT")]
        format: Vec<String>,

        /// Reuse existing Flutter build artifacts.
        #[arg(long)]
        use_existing_flutter_build: bool,

        /// Do not clean temporary files after building.
        #[arg(long)]
        skip_cleanup: bool,
    },

    /// Build an Android APK (alias for `build --target android`).
    #[command(hide = true)]
    Android {
        /// Reuse existing Flutter build artifacts.
        #[arg(long)]
        use_existing_flutter_build: bool,

        /// Sign the APK using the configured keystore (not implemented yet).
        #[arg(long)]
        sign_apk: bool,
    },

    /// Remove downloaded tools and templates from the cache.
    ///
    /// They are fetched again on demand. This does not touch the project's build
    /// output or Flutter build artifacts; use `pacor clean` for those.
    Cache {
        #[command(subcommand)]
        action: CacheAction,
    },

    /// Remove the output directory and Flutter build artifacts.
    Clean {
        /// Limit cleanup to a specific target's Flutter build directory.
        #[arg(long, value_name = "OS", value_parser = target_parser())]
        target: Option<Platform>,
    },

    /// Verify the project and build environment.
    Check {
        /// Target platform to validate (defaults to the host platform).
        #[arg(long, value_name = "OS", value_parser = target_parser())]
        target: Option<Platform>,
    },

    /// Create a commented `pacor.toml` configuration file.
    Init {
        /// Overwrite an existing configuration file.
        #[arg(long = "force")]
        force_overwrite: bool,
    },

    /// Print the effective configuration.
    Config {
        /// Print the complete serialized configuration.
        #[arg(long = "full")]
        show_full_config: bool,
    },
}
