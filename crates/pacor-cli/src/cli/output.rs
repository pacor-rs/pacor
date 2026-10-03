//! Consistent user-facing output.

use colored::Colorize;

use pacor_core::platforms::BuildArtifact;
use pacor_core::utils::command::CommandOutputMode;

/// Formats command output, honouring the requested verbosity.
pub struct OutputFormatter {
    enabled: bool,
}

impl OutputFormatter {
    pub fn new(output_mode: CommandOutputMode) -> Self {
        Self {
            enabled: output_mode != CommandOutputMode::None,
        }
    }

    pub fn section(&self, header: &str) {
        if self.enabled {
            println!("{}", format!("=== {header} ===").blue().bold());
        }
    }

    pub fn success(&self, message: &str) {
        if self.enabled {
            println!("{}", message.green());
        }
    }

    pub fn info(&self, message: &str) {
        if self.enabled {
            println!("{}", message.cyan());
        }
    }

    pub fn error(&self, message: &str) {
        if self.enabled {
            eprintln!("{}", message.red());
        }
    }

    pub fn build_success(&self, title: &str, artifacts: &[BuildArtifact]) {
        if !self.enabled {
            return;
        }

        println!();
        self.section(title);

        if artifacts.is_empty() {
            println!("No output files generated");
            return;
        }

        println!("Output files:");
        for artifact in artifacts {
            println!(" - {}", artifact.file().display().to_string().green());
            println!(" - {}", artifact.sha1().display().to_string().green());
        }
    }
}

/// Render a byte count for humans, e.g. `1.4 MiB`.
///
/// Binary units, because this reports on-disk tool sizes.
pub fn format_size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KiB", "MiB", "GiB"];

    let mut value = bytes as f64;
    let mut unit = 0;

    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }

    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_are_rendered_in_binary_units() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(512), "512 B");
        assert_eq!(format_size(1024), "1.0 KiB");
        assert_eq!(format_size(1536), "1.5 KiB");
        assert_eq!(format_size(1024 * 1024), "1.0 MiB");
        assert_eq!(format_size(13 * 1024 * 1024 + 512 * 1024), "13.5 MiB");
    }
}
