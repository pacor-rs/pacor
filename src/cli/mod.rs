//! Command-line interface for the Pacor cross-platform packaging tool.
//!
//! This module provides a modular CLI architecture with separated concerns:
//! - `args`: Command-line argument parsing and validation
//! - `handlers`: Individual command handlers with business logic
//! - `orchestrator`: Build process orchestration and lifecycle management
//! - `output`: Consistent output formatting and user feedback
//! - `commands`: Legacy command execution (being phased out)

pub(crate) mod args;
pub(crate) mod commands;
pub(crate) mod handlers;
pub(crate) mod orchestrator;
pub(crate) mod output;

pub use args::Cli;
pub use commands::execute_cli_command;
