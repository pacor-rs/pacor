//! Packaging backend registry for `pacor`.
//!
//! This crate is the only place that knows about all concrete backends. It maps
//! a [`Platform`] to a boxed [`BundleBuilder`], so `pacor-cli` never depends on
//! a specific packaging crate. Every backend is compiled on every host; whether
//! it can actually run is decided at runtime by its `validate` step.

use anyhow::{Result, bail};

use pacor_core::config::{InstallerFormat, LinuxPackageFormat, PacorConfig};
use pacor_core::core::builder::BundleBuilder;
use pacor_core::platforms::{BuildArtifact, Platform};

/// A boxed backend for the current configuration.
pub type BoxedBuilder<'a> =
    Box<dyn BundleBuilder<Config = &'a PacorConfig, Output = Vec<BuildArtifact>> + 'a>;

/// Create the packaging backend that handles `target`.
pub fn packager_for<'a>(target: Platform, config: &'a PacorConfig) -> Result<BoxedBuilder<'a>> {
    match target {
        Platform::Linux => Ok(Box::new(pacor_packaging_linux::LinuxBuilder::new(config)?)),
        Platform::MacOS => Ok(Box::new(pacor_packaging_macos::MacOSAppBuilder::new(
            config,
        )?)),
        Platform::Windows => Ok(Box::new(pacor_packaging_windows::WindowsBuilder::new(
            config,
        )?)),
        Platform::Android => Ok(Box::new(pacor_packaging_android::AndroidBuilder::new(
            config,
        )?)),
    }
}

/// All backends that are available in this build, for `pacor check`.
pub fn available_targets() -> &'static [Platform] {
    &Platform::ALL
}

/// Apply `--format` overrides onto `config` for `target`.
///
/// `--format` cannot be validated by the argument parser, because the set of
/// valid values depends on `--target` (only Linux takes several formats, macOS
/// and Android take exactly one, Windows maps to a single installer format).
/// The knowledge therefore lives here, next to the target dispatch, rather than
/// in the CLI.
pub fn apply_format_overrides(
    config: &mut PacorConfig,
    target: Platform,
    formats: &[String],
) -> Result<()> {
    if formats.is_empty() {
        return Ok(());
    }

    match target {
        Platform::Linux => {
            let parsed: std::collections::HashSet<LinuxPackageFormat> = formats
                .iter()
                .map(|format| format.parse::<LinuxPackageFormat>())
                .collect::<std::result::Result<_, _>>()
                .map_err(anyhow::Error::msg)?;

            config
                .platforms
                .linux
                .get_or_insert_with(Default::default)
                .formats = parsed;
        }

        Platform::Windows => {
            // The Windows configuration holds a single installer format, so
            // accepting several values and silently ignoring them would hide a
            // user mistake.
            let [format] = formats else {
                bail!(
                    "`--target windows` takes a single `--format` (got {})",
                    formats.join(", ")
                );
            };

            let format = format
                .parse::<InstallerFormat>()
                .map_err(anyhow::Error::msg)?;

            config
                .platforms
                .windows
                .get_or_insert_with(Default::default)
                .installer_format = format;
        }

        Platform::MacOS => require_single_format(formats, pacor_packaging_macos::SUPPORTED_FORMAT)?,
        Platform::Android => {
            require_single_format(formats, pacor_packaging_android::SUPPORTED_FORMAT)?
        }
    }

    Ok(())
}

/// These backends produce exactly one format; accept it and reject anything else.
fn require_single_format(formats: &[String], supported: &str) -> Result<()> {
    match formats {
        [only] if only == supported => Ok(()),
        _ => bail!(
            "unsupported format for this target: {requested} (supported: {supported})",
            requested = formats.join(", ")
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pacor_core::config::{AppConfig, AppMetadata};
    use pacor_core::i18n::Locale;
    use std::collections::{HashMap, HashSet};

    fn config() -> PacorConfig {
        PacorConfig {
            app: AppConfig {
                default_locale: Locale::EnUs,
                i18n: HashMap::from([(
                    Locale::EnUs,
                    AppMetadata::new(Some("Demo"), Some("Demo"), None, "Publisher", None),
                )]),
                authors: Some(HashSet::new()),
                url: "https://example.com".to_string(),
            },
            build: Default::default(),
            platforms: Default::default(),
        }
    }

    fn formats(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn no_override_leaves_the_configuration_untouched() {
        let mut config = config();
        apply_format_overrides(&mut config, Platform::Linux, &[]).unwrap();

        assert!(config.platforms.linux.is_none());
        assert!(config.platforms.windows.is_none());
    }

    #[test]
    fn linux_accepts_several_formats_and_creates_the_section_when_absent() {
        let mut config = config();
        apply_format_overrides(&mut config, Platform::Linux, &formats(&["deb", "appimage"]))
            .unwrap();

        let linux = config.platforms.linux.expect("section should be created");
        assert!(linux.formats.contains(&LinuxPackageFormat::Deb));
        assert!(linux.formats.contains(&LinuxPackageFormat::AppImage));
        assert!(!linux.formats.contains(&LinuxPackageFormat::Rpm));
    }

    #[test]
    fn linux_rejects_an_unknown_format_and_names_the_valid_ones() {
        let mut config = config();
        let error = apply_format_overrides(&mut config, Platform::Linux, &formats(&["rpm2"]))
            .unwrap_err()
            .to_string();

        assert!(error.contains("rpm2"), "{error}");
        assert!(error.contains("appimage"), "{error}");
        // A failed override must not partially apply.
        assert!(config.platforms.linux.is_none());
    }

    #[test]
    fn windows_maps_to_a_single_installer_format() {
        let mut config = config();
        apply_format_overrides(&mut config, Platform::Windows, &formats(&["msi"])).unwrap();

        assert_eq!(
            config.platforms.windows.unwrap().installer_format,
            InstallerFormat::Msi
        );
    }

    #[test]
    fn windows_rejects_several_formats_instead_of_silently_ignoring_them() {
        let mut config = config();
        let error =
            apply_format_overrides(&mut config, Platform::Windows, &formats(&["nsis", "msi"]))
                .unwrap_err()
                .to_string();

        assert!(error.contains("single `--format`"), "{error}");
        assert!(error.contains("nsis, msi"), "{error}");
    }

    #[test]
    fn single_format_backends_accept_their_format_only() {
        let mut config = config();
        assert!(apply_format_overrides(&mut config, Platform::MacOS, &formats(&["dmg"])).is_ok());
        assert!(apply_format_overrides(&mut config, Platform::Android, &formats(&["apk"])).is_ok());

        let error = apply_format_overrides(&mut config, Platform::MacOS, &formats(&["apk"]))
            .unwrap_err()
            .to_string();
        assert!(error.contains("supported: dmg"), "{error}");

        let error = apply_format_overrides(&mut config, Platform::Android, &formats(&["dmg"]))
            .unwrap_err()
            .to_string();
        assert!(error.contains("supported: apk"), "{error}");
    }
}
