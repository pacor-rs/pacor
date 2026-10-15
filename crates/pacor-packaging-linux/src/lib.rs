//! # Linux packaging backend
//!
//! Turns a Flutter Linux build into the package formats configured in
//! `[platforms.linux]`. The work is split by responsibility:
//!
//! - [`formats`]: per-format adapters (AppImage, DEB) and the dispatch over `formats`
//! - [`icons`]: icon discovery and the fallback icon set
//! - [`desktop`]: the freedesktop `.desktop` entry
//! - [`flutter`]: the `flutter build linux` invocation
//! - [`appimage`]: AppDir staging, linuxdeploy, artifact normalisation
//! - [`deb`]: `control`/`data` sections and the `ar` container
//! - [`project_templates`]: temporary project file patching with a restore guard
//!
//! This module is compiled on every host; whether the Linux toolchain is
//! available is decided by [`LinuxBuilder::validate`] at runtime.

#[macro_use]
extern crate tracing;

use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow};
use pacor_core::{
    config::{FlutterBuildMode, LinuxConfig, PacorConfig},
    core::{
        arch::Arch,
        builder::BundleBuilder,
        project::{FlutterProject, find_flutter_project_root},
    },
    platforms::{BuildArtifact, Platform},
    utils::{command::CommandOutputMode, fs::clean_dir},
};

mod appimage;
mod deb;
mod desktop;
mod flutter;
mod formats;
mod icon;
mod icons;
mod project_templates;

/// Packages a Flutter Linux build into AppImage and/or DEB artifacts.
pub struct LinuxBuilder<'a> {
    config: &'a PacorConfig,
    linux_config: LinuxConfig,
    flutter_project: FlutterProject,
    project_root: PathBuf,
    flutter_build_path: PathBuf,
    output_dir: PathBuf,
    tools_dir: PathBuf,
    arch: Arch,
    build_mode: FlutterBuildMode,
}

impl<'a> BundleBuilder for LinuxBuilder<'a> {
    type Config = &'a PacorConfig;
    type Output = Vec<BuildArtifact>;

    fn new(config: Self::Config) -> Result<Self> {
        let linux_config = config.platforms.linux.clone().unwrap_or_default();
        if !linux_config.enabled() {
            return Err(anyhow!("Linux platform is not enabled in configuration"));
        }

        let project_root = find_flutter_project_root()?;
        let flutter_project = FlutterProject::load_from_pubspec(&project_root)?;
        let arch = Arch::current()?;
        let build_mode = config.build.flutter_build_mode;
        let flutter_build_path = flutter_project.get_flutter_build_path_for_mode(
            &project_root,
            Platform::Linux,
            &arch,
            build_mode,
        );
        let output_dir = config.output_dir()?;

        let tools_dir = pacor_core::tools::platform_cache_dir(Platform::Linux)
            .ok_or_else(|| anyhow!("failed to determine the user configuration directory"))?;

        info!(
            project = %flutter_project.name(),
            version = %flutter_project.version(),
            arch = %arch,
            mode = %build_mode,
            output_dir = %output_dir.display(),
            tools_dir = %tools_dir.display(),
            "Initializing Linux builder"
        );

        Ok(Self {
            config,
            linux_config,
            flutter_project,
            project_root,
            flutter_build_path,
            output_dir,
            tools_dir,
            arch,
            build_mode,
        })
    }

    fn build(
        &self,
        use_existing_flutter_build: bool,
        command_output_mode: CommandOutputMode,
    ) -> Result<Self::Output> {
        info!("Starting Linux packaging");

        if use_existing_flutter_build {
            // Linux builds require patched project templates, so the Flutter
            // app is always rebuilt here.
            warn!("`--use-existing-flutter-build` is not supported on Linux; rebuilding");
        }

        pacor_core::packaging::prepare_output_directory(&self.output_dir)?;

        let outputs = formats::build_installers(&self.context(), &command_output_mode)?;

        info!(artifacts = outputs.len(), "Linux packaging finished");
        Ok(outputs)
    }

    fn clean(&self) -> Result<()> {
        if self.flutter_build_path.exists() {
            pacor_core::packaging::clean_directory_contents(&self.flutter_build_path)?;
        }

        // `--split-debug-info=tmp/` writes symbols next to the project.
        let symbols_dir = self.project_root.join("tmp");
        if symbols_dir.exists() {
            clean_dir(&symbols_dir)?;
            debug!(path = %symbols_dir.display(), "Removed split debug symbols");
        }

        Ok(())
    }

    fn validate(&self) -> Result<()> {
        let required_tools = ["flutter"];
        let pubspec_path = self.project_root.join("pubspec.yaml");
        let linux_dir = self.project_root.join("linux");
        let required_files = [pubspec_path.as_path(), linux_dir.as_path()];

        pacor_core::packaging::validate_build_environment(
            "Linux",
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
        &self.flutter_build_path
    }
}

impl LinuxBuilder<'_> {
    /// Borrow the resolved inputs the format adapters operate on.
    fn context(&self) -> formats::LinuxBuildContext<'_> {
        formats::LinuxBuildContext {
            app_config: &self.config.app,
            linux_config: &self.linux_config,
            flutter_project: &self.flutter_project,
            project_root: &self.project_root,
            flutter_build_path: &self.flutter_build_path,
            output_dir: &self.output_dir,
            tools_dir: &self.tools_dir,
            arch: &self.arch,
            build_mode: self.build_mode,
        }
    }
}
