//! Command-line interface for `pacor`.
//!
//! - [`args`]: clap definitions
//! - [`commands`]: dispatch from a parsed command to a handler
//! - [`handlers`]: one function per command
//! - [`output`]: consistent user-facing output

pub mod args;
pub mod commands;
pub mod handlers;
pub mod output;

pub use args::Cli;
pub use commands::execute_cli_command;
