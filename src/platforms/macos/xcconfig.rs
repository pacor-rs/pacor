// !!! THIS MODULE IS NOT BE USED

use std::fs;
use std::path::PathBuf;
use std::{fmt::Display, path::Path};

use serde::{Deserialize, Serialize};

use crate::error::{BundleError, Result};

/// Represents the structured content of `AppInfo.xcconfig`, a Flutter-specific
/// Xcode configuration file used to define app metadata such as name and bundle ID.
///
/// This is **not a general-purpose `.xcconfig` parser**. It only supports the
/// specific keys expected in `AppInfo.xcconfig` generated or used by Flutter tooling.
#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub(super) struct XcodeConfiguration {
    #[serde(skip_serializing_if = "Option::is_none")]
    product_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    product_bundle_identifier: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    product_copyright: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    executable_name: Option<String>,
}

impl TryFrom<&str> for XcodeConfiguration {
    type Error = BundleError;

    /// Parses `AppInfo.xcconfig` content.
    ///
    /// Only recognizes the four standard Flutter keys:
    /// - `PRODUCT_NAME`
    /// - `PRODUCT_BUNDLE_IDENTIFIER`
    /// - `PRODUCT_COPYRIGHT`
    /// - `EXECUTABLE_NAME`
    ///
    /// Ignores empty lines and lines starting with `//`.
    /// All other lines are treated as invalid and cause a parsing error.
    fn try_from(content: &str) -> Result<Self> {
        let mut config = XcodeConfiguration::default();

        for (line_num, line) in content.lines().enumerate() {
            let trimmed = line.trim();

            // Skip comments and empty lines
            if trimmed.is_empty() || trimmed.starts_with("//") {
                continue;
            }

            // Expect format: KEY = VALUE
            if let Some((key, value)) = parse_key_value_assignment(trimmed) {
                match key {
                    "PRODUCT_NAME" => config.product_name = Some(value),
                    "PRODUCT_BUNDLE_IDENTIFIER" => config.product_bundle_identifier = Some(value),
                    "PRODUCT_COPYRIGHT" => config.product_copyright = Some(value),
                    "EXECUTABLE_NAME" => config.executable_name = Some(value),
                    _ => {
                        return Err(BundleError::InvalidXcconfigFormat {
                            line: line_num + 1,
                            content: trimmed.to_string(),
                        });
                    }
                }
            } else {
                return Err(BundleError::InvalidXcconfigFormat {
                    line: line_num + 1,
                    content: trimmed.to_string(),
                });
            }
        }

        Ok(config)
    }
}

/// Parses a line expected to be in the form: `KEY = VALUE`
///
/// Optionally strips trailing `// comment` from the value.
/// Returns `None` if the format is invalid.
fn parse_key_value_assignment(line: &str) -> Option<(&str, String)> {
    // Find the first '=' that is not part of the value (i.e., after the key)
    if let Some(eq_pos) = line.find('=') {
        let key = line[..eq_pos].trim();
        let mut value = line[eq_pos + 1..].trim().to_string();

        // Remove trailing "// comment" if present (for resilience)
        if let Some(comment_pos) = value.find("//") {
            // Only treat as comment if there's whitespace before it (or it's at start of value)
            // But in practice, we just strip from the first "//" onward
            // since values shouldn't contain "//" literally in AppInfo.xcconfig
            value.truncate(comment_pos);
            value = value.trim_end().to_string();
        }

        // Ensure key is non-empty and consists of valid identifier characters
        if !key.is_empty()
            && key
                .chars()
                .all(|c| c.is_ascii_uppercase() || c == '_' || c.is_ascii_digit())
        {
            return Some((key, value));
        }
    }
    None
}

impl Display for XcodeConfiguration {
    /// Serializes the configuration back into `AppInfo.xcconfig` format.
    ///
    /// Includes explanatory comments and maintains a consistent layout.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "// Application-level settings for the Runner target.")?;
        writeln!(f, "//")?;
        writeln!(
            f,
            "// This file is auto-generated from Flutter app metadata (e.g., pubspec.yaml)."
        )?;
        writeln!(
            f,
            "// Do not edit manually unless you know what you're doing."
        )?;
        writeln!(f)?;

        if let Some(name) = &self.product_name {
            writeln!(
                f,
                "// The application's name. By default this is also the title of the Flutter window."
            )?;
            writeln!(f, "PRODUCT_NAME = {}", name)?;
            writeln!(f)?;
        }

        if let Some(bundle_id) = &self.product_bundle_identifier {
            writeln!(f, "// The application's bundle identifier.")?;
            writeln!(f, "PRODUCT_BUNDLE_IDENTIFIER = {}", bundle_id)?;
            writeln!(f)?;
        }

        if let Some(copyright) = &self.product_copyright {
            writeln!(f, "// The copyright displayed in application information.")?;
            writeln!(f, "PRODUCT_COPYRIGHT = {}", copyright)?;
            writeln!(f)?;
        }

        if let Some(exec_name) = &self.executable_name {
            writeln!(
                f,
                "// The name of the executable binary (in Contents/MacOS, can be different from PRODUCT_NAME)."
            )?;
            writeln!(f, "EXECUTABLE_NAME = {}", exec_name)?;
        }

        Ok(())
    }
}

impl XcodeConfiguration {
    /// Loads `AppInfo.xcconfig` from the given file path.
    pub(super) fn from_file<P: AsRef<Path>>(path: P) -> Result<Self> {
        let content = fs::read_to_string(path)?;
        Self::try_from(content.as_str())
    }

    /// Saves this configuration to the given file path as `AppInfo.xcconfig`.
    ///
    /// Returns the path to the backup file if one was created.
    pub(super) fn save_to_file<P: AsRef<Path>>(&self, path: P) -> Result<Option<PathBuf>> {
        let path = path.as_ref();
        if !path.exists() {
            fs::write(path, self.to_string())?;
            return Ok(None);
        }

        let backup = path.with_file_name("AppInfo.xcconfig.bak");
        // Backup existing file
        fs::rename(path, &backup)?;
        fs::write(path, self.to_string())?;
        Ok(Some(backup))
    }

    /// Sets the application display name (`PRODUCT_NAME`).
    pub(super) fn set_product_name(&mut self, name: &str) {
        self.product_name = Some(name.to_string());
    }

    /// Sets the executable binary name (`EXECUTABLE_NAME`).
    pub(super) fn set_executable_name(&mut self, name: &str) {
        self.executable_name = Some(name.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reject_unknown_key() {
        let content = "UNKNOWN_SETTING = value";
        let err = XcodeConfiguration::try_from(content).unwrap_err();
        assert!(matches!(err, BundleError::InvalidXcconfigFormat { .. }));
    }

    #[test]
    fn test_reject_malformed_line() {
        let content = "PRODUCT_NAME"; // missing =
        let err = XcodeConfiguration::try_from(content).unwrap_err();
        assert!(matches!(err, BundleError::InvalidXcconfigFormat { .. }));
    }

    #[test]
    fn test_empty_config() {
        let config = XcodeConfiguration::default();
        let output = config.to_string();
        // Should only contain header comments and no settings
        assert!(output.contains("Application-level settings"));
        assert!(!output.contains("PRODUCT_NAME"));
    }
}
