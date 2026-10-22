// Copyright 2016-2019 Cargo-Bundle developers <https://github.com/burtonageo/cargo-bundle>
// Copyright 2019-2024 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! Debian package (`.deb`) builder.
//!
//! A `.deb` is an `ar` archive containing three members:
//!
//! ```text
//! foobar_1.2.3_amd64.deb   # actually an ar archive
//!     debian-binary        # format version ("2.0")
//!     control.tar.gz       # control, md5sums, maintainer scripts
//!     data.tar.gz          # the files to install
//! ```
//!
//! Responsibilities are split as follows:
//!
//! - [`data`]: assemble `data.tar.gz` (the payload filesystem layout)
//! - [`control`]: generate `control.tar.gz` (metadata, scripts, md5sums)
//! - [`archive`]: `ar`/`tar`/`gzip` primitives and installed-size accounting
//! - this module: orchestration and the template swap around the Flutter build

mod archive;
mod control;
mod data;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::Result;

use pacor_core::config::{AppConfig, DebianConfig};
use pacor_core::core::arch::Arch;
use pacor_core::core::freedesktop::FreeDesktop;
use pacor_core::utils::fs::{clean_dir, ensure_directory_exists};

use crate::icon::Icon;
use crate::project_templates::{ProjectTemplate, TemplateApplier, TemplateSwap, TemplateVariables};

/// Values baked into the Debian build templates.
///
/// The bundle keeps its directory name — the Flutter project name, which the
/// templates also use as `BINARY_NAME` — so the runner finds its own
/// `libapp.so`, assets and ICU data. Only `Package:` and the documentation
/// directory are hyphenated, because that is all `dpkg` requires.
fn template_variables(app_config: &AppConfig, executable_name: &str) -> TemplateVariables {
    TemplateVariables {
        executable_name: executable_name.to_string(),
        lib_dir: format!("{}/{executable_name}", data::USR_LIB_SUBDIR),
        data_dir: format!("{}/{executable_name}", data::USR_DATA_SUBDIR),
        application_id: application_id(app_config, executable_name),
    }
}

/// Reverse-DNS GTK application id, e.g. `com.example.demo`.
///
/// Reuses the macOS bundle identifier when it is configured so that both
/// platforms identify the application identically; otherwise it is derived from
/// the publisher's domain, falling back to the executable name.
fn application_id(app_config: &AppConfig, executable_name: &str) -> String {
    let normalized = executable_name.replace('_', "-");

    let domain = app_config
        .url()
        .split("://")
        .nth(1)
        .unwrap_or(app_config.url())
        .split('/')
        .next()
        .unwrap_or_default()
        .trim_start_matches("www.");

    let mut parts: Vec<String> = domain
        .split('.')
        .filter(|part| !part.is_empty())
        .map(|part| part.to_ascii_lowercase())
        .collect();
    parts.reverse();

    if parts.is_empty() {
        return format!("com.example.{normalized}");
    }

    parts.push(normalized);
    parts.join(".")
}

/// Everything required to build a `.deb`.
pub struct DebianPackageBuildConfig<'a> {
    pub app_config: &'a AppConfig,
    pub app_executable_name: &'a str,
    pub additional_icons: &'a BTreeMap<u32, Icon>,
    pub flutter_build_path: &'a Path,
    pub project_root: &'a Path,
    pub tools_directory: &'a Path,
    pub output_directory: &'a Path,
    pub desktop: &'a FreeDesktop,
    pub target_architecture: &'a Arch,
    pub package_version: &'a str,
    pub package_description: &'a str,
    pub debian_package_config: DebianConfig,
    pub artifact_name: &'a str,
    pub maintainer: Option<&'a str>,
}

/// Builds Debian packages from a Flutter Linux build.
pub struct DebianPackageBuilder<'a> {
    config: DebianPackageBuildConfig<'a>,
}

impl<'a> DebianPackageBuilder<'a> {
    /// Create a builder from the provided configuration.
    pub fn from_config(config: DebianPackageBuildConfig<'a>) -> Self {
        debug!(
            package = config.app_executable_name,
            "Initializing Debian package builder"
        );
        Self { config }
    }

    /// Debian package name: dpkg forbids underscores.
    fn package_name(&self) -> String {
        self.config.app_executable_name.replace('_', "-")
    }

    /// Replace the project's Linux templates with rendered upstream versions.
    ///
    /// The returned guard keeps the project files replaced until it is dropped;
    /// it restores them even if the build fails.
    pub fn apply_templates(&self) -> Result<TemplateSwap> {
        info!("Applying Debian build templates");

        let applier = TemplateApplier::new(
            self.config.tools_directory,
            "deb",
            template_variables(self.config.app_config, self.config.app_executable_name),
        )?;

        TemplateSwap::apply(
            applier,
            &[
                ProjectTemplate {
                    remote_filename: "my_application_template.cc".to_string(),
                    local_file_path: self
                        .config
                        .project_root
                        .join("linux/runner/my_application.cc"),
                },
                ProjectTemplate {
                    remote_filename: "CMakeLists_template.txt".to_string(),
                    local_file_path: self.config.project_root.join("linux/CMakeLists.txt"),
                },
            ],
        )
    }

    /// Build the `.deb` and return its path.
    pub fn build_package(&self) -> Result<PathBuf> {
        let package_name = self.package_name();
        let output_path = self.config.output_directory.join(self.config.artifact_name);
        info!(
            package = %package_name,
            output = %output_path.display(),
            "Starting Debian package build"
        );

        ensure_directory_exists(self.config.output_directory)?;

        // The staging directory must outlive the whole assembly.
        let staging_guard = tempfile::TempDir::with_prefix("pacor_deb")?;
        let staging_dir = staging_guard.path().join(&package_name);
        clean_dir(&staging_dir)?;

        let data_root = data::assemble_data_root(
            &staging_dir,
            &data::DataContext {
                app_config: self.config.app_config,
                executable_name: self.config.app_executable_name,
                package_name: &package_name,
                flutter_build_path: self.config.flutter_build_path,
                desktop: self.config.desktop,
                icons: self.config.additional_icons,
                extra_files: self.config.debian_package_config.files(),
            },
        )?;

        let control_dir = staging_dir.join("control");
        control::write_control_members(
            &control_dir,
            &data_root,
            &control::ControlContext {
                app_config: self.config.app_config,
                package_name: &package_name,
                version: self.config.package_version,
                description: self.config.package_description,
                architecture: self.config.target_architecture,
                maintainer: self.config.maintainer,
                debian: &self.config.debian_package_config,
            },
        )?;

        // How to build the members of a .deb:
        // http://www.tldp.org/HOWTO/Debian-Binary-Package-Building-HOWTO/x60.html#AEN66
        let debian_binary = staging_dir.join("debian-binary");
        archive::write_file_with_content(&debian_binary, "2.0\n")?;

        debug!("Compressing control and data sections");
        let control_archive = archive::create_gzipped_tarball(&control_dir)?;
        let data_archive = archive::create_gzipped_tarball(&data_root)?;

        archive::create_ar_archive(
            vec![debian_binary, control_archive, data_archive],
            &output_path,
        )?;

        info!(path = %output_path.display(), "Debian package built successfully");
        Ok(output_path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pacor_core::config::AppMetadata;
    use pacor_core::i18n::Locale;
    use std::collections::{HashMap, HashSet};

    fn app_config(url: &str) -> AppConfig {
        AppConfig {
            default_locale: Locale::EnUs,
            i18n: HashMap::from([(
                Locale::EnUs,
                AppMetadata::new(Some("Demo"), Some("Demo"), None, "Publisher", None),
            )]),
            authors: Some(HashSet::new()),
            url: url.to_string(),
        }
    }

    #[test]
    fn template_variables_match_the_installed_layout() {
        let config = app_config("https://example.com");
        let variables = template_variables(&config, "my_app");

        assert_eq!(variables.executable_name, "my_app");
        // Must equal where `data.rs` puts the bundle: usr/<dir>/<executable>.
        assert_eq!(variables.lib_dir, "lib/my_app");
        assert_eq!(variables.data_dir, "share/my_app");
    }

    #[test]
    fn the_package_name_is_hyphenated_but_the_paths_are_not() {
        let config = app_config("https://example.com");
        let variables = template_variables(&config, "my_app");

        // `Package:` and the doc directory use the hyphenated form...
        assert_eq!("my_app".replace('_', "-"), "my-app");
        // ...but the bundle directories keep the name the Flutter build used, or
        // the runner cannot find its own library and assets.
        assert_eq!(variables.lib_dir, "lib/my_app");
        assert_eq!(variables.data_dir, "share/my_app");
    }

    #[test]
    fn application_id_is_derived_from_the_publisher_domain() {
        let config = app_config("https://example.com");
        assert_eq!(application_id(&config, "demo"), "com.example.demo");

        let config = app_config("https://www.pacor-rs.org/app");
        assert_eq!(application_id(&config, "demo"), "org.pacor-rs.demo");

        let config = app_config("https://tools.example.co.uk");
        assert_eq!(application_id(&config, "demo"), "uk.co.example.tools.demo");
    }

    #[test]
    fn application_id_survives_a_missing_or_odd_url() {
        // No host to derive from -> a valid id is still produced.
        let config = app_config("");
        assert_eq!(application_id(&config, "demo"), "com.example.demo");

        let config = app_config("https://example.com");
        assert_eq!(application_id(&config, "my_app"), "com.example.my-app");
    }
}
