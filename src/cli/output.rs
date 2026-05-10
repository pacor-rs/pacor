//! Output formatting for the Pacor CLI.
//!
//! This module provides consistent output formatting across all CLI commands,
//! including colored output, progress indicators, and structured result display.

use colored::Colorize;
use tracing::info;

use crate::cli::args::CommandOutputMode;
use crate::platforms::BuildArtifact;

/// Handles formatted output for CLI commands.
///
/// The OutputFormatter provides a consistent interface for displaying
/// information to users with appropriate styling and verbosity levels.
pub(crate) struct OutputFormatter {
    output_mode: CommandOutputMode,
}

impl OutputFormatter {
    /// Create a new output formatter with the specified output mode.
    pub(crate) fn new(output_mode: CommandOutputMode) -> Self {
        Self { output_mode }
    }

    /// Print a formatted section header.
    pub(crate) fn print_section_header(&self, header_text: &str) {
        if self.output_mode != CommandOutputMode::None {
            println!("{}", format!("=== {} ===", header_text).blue().bold());
        }
    }

    /// Display a success message in green.
    pub(crate) fn display_success(&self, message: &str) {
        if self.output_mode != CommandOutputMode::None {
            println!("{}", message.green());
        }
    }

    /// Display an informational message in cyan.
    pub(crate) fn display_info(&self, message: &str) {
        if self.output_mode != CommandOutputMode::None {
            println!("{}", message.cyan());
        }
    }

    /// Display an error message in red.
    pub(crate) fn display_error(&self, message: &str) {
        if self.output_mode != CommandOutputMode::None {
            eprintln!("{}", message.red());
        }
    }

    /// Display successful build completion message with output files.
    pub(crate) fn display_build_success(&self, build_outputs: &[BuildArtifact]) {
        if self.output_mode == CommandOutputMode::None {
            return;
        }

        println!();
        self.print_section_header("Build Completed Successfully");

        if build_outputs.is_empty() {
            println!("No output files generated");
            return;
        }

        println!("Output files:");
        for artifact in build_outputs {
            println!(" - {}", artifact.file().display().to_string().green());
            println!(" - {}", artifact.sha1().display().to_string().green());
        }

        info!("Build outputs displayed successfully");
    }

    /// Display successful Android build completion with APK files.
    pub(crate) fn display_android_build_success(&self, build_outputs: &[BuildArtifact]) {
        if self.output_mode == CommandOutputMode::None {
            return;
        }

        println!();
        self.print_section_header("Android Build Completed Successfully");
        println!("Output files:");

        // Android builds typically produce APK and SHA1 checksum pairs
        for build_artifact in build_outputs {
            println!(" - {}", build_artifact.file().display().to_string().green());
            println!(" - {}", build_artifact.sha1().display().to_string().green());
        }

        info!("Android build outputs displayed successfully");
    }

    // /// Display a key-value pair with consistent formatting.
    // pub(crate) fn display_key_value(&self, key: &str, value: &str) {
    //     if self.output_mode != CommandOutputMode::None {
    //         println!("{}: {}", key.blue().bold(), value);
    //     }
    // }
}
