//! # Pacor
//!
//! A powerful cross-platform packaging tool for Flutter applications.
//!
//! Supports generating:
//! - macOS DMG installers
//! - Windows NSIS/MSI installers
//! - Linux AppImage/DEB installers
//! - Android APK installers
//!
//! ## Usage Examples
//!
//! ```bash
//! # Initialize configuration
//! pacor init
//!
//! # Build for current platform
//! pacor build
//!
//! # Build for Android
//! pacor android
//! ```

mod cli;
mod config;
mod core;
mod error;
mod i18n;
mod log;
mod platforms;
mod utils;

pub use cli::{Cli, execute_cli_command};

#[macro_use]
extern crate tracing;
