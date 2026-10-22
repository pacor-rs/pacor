//! # macOS packaging backend
//!
//! Packages a Flutter macOS application into a DMG installer, optionally code
//! signs and notarizes it, and produces a `.app.tar.gz` updater archive.
//!
//! The work is split by responsibility:
//!
//! - `tools`: materialise the embedded `create-dmg` script and templates
//! - `flutter_build`: run `flutter build macos`
//! - `localization`: install `.lproj` resources and update `Info.plist`
//! - `dmg`: build the DMG from the `.app` bundle
//! - `signing`: `codesign`
//! - `notarization`: `notarytool submit` + `stapler staple`
//! - `updater`: `.app.tar.gz` archive
//! - `plist`: minimal plist read/modify/write
//!
//! This module is compiled on every host; whether the macOS toolchain is
//! available is decided by [`MacOSAppBuilder::validate`] at runtime.

#[macro_use]
extern crate tracing;

/// The only output format this backend produces.
pub const SUPPORTED_FORMAT: &str = "dmg";

mod dmg;
mod flutter_build;
mod localization;
mod notarization;
mod plist;
mod signing;
mod tools;
mod updater;

use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow};
use pacor_core::core::{
    arch::Arch,
    builder::BundleBuilder,
    project::{FlutterProject, find_flutter_project_root},
};
use pacor_core::{
    config::{AppConfig, FlutterBuildMode, MacOSBuildConfig, PacorConfig},
    platforms::{BuildArtifact, Platform},
    utils::{command::CommandOutputMode, fs::clean_dir},
};

/// Packages a Flutter macOS application into a DMG installer and an updater
/// archive, with optional code signing and notarization.
pub struct MacOSAppBuilder<'a> {
    app_config: &'a AppConfig,
    macos_config: &'a MacOSBuildConfig,

    flutter_project: FlutterProject,

    project_root: PathBuf,
    flutter_build_dir: PathBuf,
    output_dir: PathBuf,

    tools_dir: PathBuf,
    create_dmg_script_path: PathBuf,

    arch: Arch,
    build_mode: FlutterBuildMode,
}

impl<'a> BundleBuilder for MacOSAppBuilder<'a> {
    type Config = &'a PacorConfig;
    type Output = Vec<BuildArtifact>;

    fn new(config: Self::Config) -> Result<Self> {
        static DEFAULT_MACOS_CONFIG: std::sync::OnceLock<MacOSBuildConfig> =
            std::sync::OnceLock::new();
        let macos_config = config
            .platforms
            .macos
            .as_ref()
            .unwrap_or_else(|| DEFAULT_MACOS_CONFIG.get_or_init(MacOSBuildConfig::default));

        if !macos_config.enabled {
            return Err(anyhow!("macOS platform is not enabled in configuration"));
        }

        let project_root = find_flutter_project_root()?;
        let flutter_project = FlutterProject::load_from_pubspec(&project_root)?;
        let arch = Arch::current()?;
        let build_mode = config.build.flutter_build_mode;
        let flutter_build_dir = flutter_project.get_flutter_build_path_for_mode(
            &project_root,
            Platform::MacOS,
            &arch,
            build_mode,
        );
        let output_dir = config.output_dir()?;

        let tools_dir = pacor_core::tools::platform_cache_dir(Platform::MacOS)
            .ok_or_else(|| anyhow!("failed to determine the user configuration directory"))?;
        let create_dmg_script_path = tools_dir.join("create-dmg.sh");

        info!(
            project_name = flutter_project.name(),
            project_version = flutter_project.version(),
            target_arch = %arch,
            build_mode = %build_mode,
            output_directory = %output_dir.display(),
            "Initializing macOS application builder"
        );

        Ok(Self {
            app_config: &config.app,
            macos_config,
            project_root,
            flutter_project,
            flutter_build_dir,
            output_dir,
            tools_dir,
            create_dmg_script_path,
            arch,
            build_mode,
        })
    }

    fn build(
        &self,
        skip_flutter_build: bool,
        command_output_mode: CommandOutputMode,
    ) -> Result<Self::Output> {
        info!("Starting macOS DMG packaging workflow");

        pacor_core::packaging::prepare_output_directory(&self.output_dir)?;

        let app_bundle_path = self
            .flutter_build_dir
            .join(format!("{}.app", self.flutter_project.name()));
        trace!(app_bundle_path = %app_bundle_path.display(), "Located .app bundle");

        if !skip_flutter_build {
            tools::prepare_packaging_tools(&self.tools_dir, &self.create_dmg_script_path)?;
            flutter_build::build_flutter_app(
                &self.project_root,
                self.build_mode,
                &command_output_mode,
            )?;
            localization::add_localization_resources(&app_bundle_path, self.app_config)?;
        } else {
            info!("Skipping Flutter build as requested");

            if !self.flutter_build_path().exists() {
                return Err(anyhow!("Flutter build output directory does not exist"));
            }
        }

        let filename_base = pacor_core::packaging::artifact_base_name(
            self.flutter_project.name(),
            self.flutter_project.version(),
            &self.arch,
        );

        let dmg_path = dmg::create_dmg_installer(
            &dmg::DmgOptions {
                create_dmg_script: &self.create_dmg_script_path,
                project_root: &self.project_root,
                output_dir: &self.output_dir,
                app_name: self.flutter_project.name(),
                background: self.macos_config.dmg_background.as_deref(),
                window_size: self.macos_config.dmg_window_size,
            },
            &app_bundle_path,
            &filename_base,
            &command_output_mode,
        )?;

        if let Some(identity) = self.macos_config.codesign_identity.as_deref() {
            signing::sign_dmg(identity, &dmg_path, &command_output_mode)?;
        } else {
            debug!("Code signing disabled: no signing identity configured");
        }

        if let Some(profile) = self.macos_config.notarize_credentials.as_deref() {
            notarization::notarize_dmg(profile, &dmg_path, &command_output_mode)?;
        } else {
            debug!("Notarization disabled: no Apple credentials configured");
        }

        let updater_archive_path =
            updater::create_updater_archive(&self.output_dir, &app_bundle_path, &filename_base)?;

        debug!("Generating SHA1 checksum files for build artifacts");
        let dmg_artifact = pacor_core::packaging::create_artifact_with_checksum(dmg_path)?;
        let updater_artifact =
            pacor_core::packaging::create_artifact_with_checksum(updater_archive_path)?;

        info!(
            dmg_checksum = %dmg_artifact.sha1().display(),
            updater_checksum = %updater_artifact.sha1().display(),
            "SHA1 checksum files generated"
        );

        Ok(vec![dmg_artifact, updater_artifact])
    }

    fn clean(&self) -> Result<()> {
        // `--split-debug-info=tmp/` writes symbol files next to the project.
        let symbols_dir = self.project_root.join("tmp");
        if symbols_dir.exists() {
            clean_dir(&symbols_dir)?;
            debug!(path = %symbols_dir.display(), "Removed split debug symbols");
        }
        Ok(())
    }

    fn clean_build_directories(&self) -> &'static [&'static str] {
        &["build/macos"]
    }

    fn artifact_suffixes(&self) -> &'static [&'static str] {
        // The updater archive is `{base}.app.tar.gz`: match the whole tail so
        // that an unrelated `.gz` in `dist/` is not removed.
        &[".dmg", ".app.tar.gz"]
    }

    fn validate(&self) -> Result<()> {
        let required_tools = ["flutter", "xcodebuild", "codesign"];
        let pubspec_path = self.project_root.join("pubspec.yaml");
        let macos_dir = self.project_root.join("macos");
        let required_files = [pubspec_path.as_path(), macos_dir.as_path()];

        pacor_core::packaging::validate_build_environment(
            "macOS",
            &required_tools,
            &required_files,
            &self.output_dir,
        )?;

        Ok(())
    }

    fn project_root(&self) -> &Path {
        &self.project_root
    }

    fn flutter_build_path(&self) -> &Path {
        &self.flutter_build_dir
    }
}
