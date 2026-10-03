//! `pacor` — cross-platform packaging tool for Flutter applications.
//!
//! See [`cli`] for the command-line surface and [`commands`](cli::commands)
//! for command dispatch. The library is intentionally thin: all domain logic
//! lives in `pacor-core` and `pacor-packaging`.

#[macro_use]
extern crate tracing;

pub mod cli;
mod log;

pub use cli::{Cli, execute_cli_command};
