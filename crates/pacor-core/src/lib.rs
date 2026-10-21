//! # pacor-core
//!
//! Domain model, configuration and shared infrastructure for the `pacor`
//! Flutter packaging tool.
//!
//! This crate is deliberately free of any concrete packaging format: platform
//! backends live in the `pacor-packaging-*` crates and depend on this crate.
//! Because of that, the configuration schema and all target descriptors are
//! compiled on **every** host, so a single `pacor.toml` describes every
//! platform and can be parsed anywhere.

#[macro_use]
extern crate tracing;

pub mod config;
pub mod core;
pub mod error;
pub mod i18n;
pub mod packaging;
pub mod platforms;
pub mod tools;
pub mod utils;

// Cache probe: this pull request is closed without merging.
