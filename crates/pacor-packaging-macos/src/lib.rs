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
//! - `plist`: edits the built `Info.plist` in place, keeping what it does not model
//!
//! This module is compiled on every host; whether the macOS toolchain is
//! available is decided by [`MacOSAppBuilder::validate`] at runtime.
//!
//! The user-facing description of this workflow is `docs/MACOS.md`.

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

impl MacOSAppBuilder<'_> {
    /// The entitlements file the bundle has to be re-signed with.
    ///
    /// Entitlements live inside the signature, so re-signing without them drops the
    /// sandbox and network access the app was built with — an app that then fails at
    /// runtime for a reason nothing in the build log mentions. Configuration wins;
    /// otherwise the file Flutter generated for this build mode is used, which is the
    /// one Xcode would have signed with.
    fn entitlements_path(&self) -> Result<Option<PathBuf>> {
        if let Some(configured) = self.macos_config.codesign_entitlements.as_deref() {
            let path = self.project_root.join(configured);
            if !path.is_file() {
                return Err(anyhow!(
                    "`codesign_entitlements` points at `{}`, which is not a file",
                    path.display()
                ));
            }
            return Ok(Some(path));
        }

        let file_name = match self.build_mode {
            FlutterBuildMode::Release => "Release.entitlements",
            FlutterBuildMode::Debug | FlutterBuildMode::Profile => "DebugProfile.entitlements",
        };
        let generated = self
            .project_root
            .join("macos")
            .join("Runner")
            .join(file_name);

        Ok(generated.is_file().then_some(generated))
    }
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

        // Installing the packaging tools and adding localization resources are pacor's own
        // steps, not Flutter's. Inside the branch below they were skipped by
        // `--use-existing-flutter-build`, which then ran `bash` against a script that had
        // never been written on a cold cache, and packaged a bundle without its `.lproj`
        // resources.
        tools::prepare_packaging_tools(&self.tools_dir, &self.create_dmg_script_path)?;

        if !skip_flutter_build {
            flutter_build::build_flutter_app(
                &self.project_root,
                self.build_mode,
                &command_output_mode,
            )?;
        } else {
            info!("Skipping Flutter build as requested");

            if !self.flutter_build_path().exists() {
                return Err(anyhow!("Flutter build output directory does not exist"));
            }
        }

        // Localization rewrites the bundle, so it has to run before `codesign` seals it.
        localization::add_localization_resources(&app_bundle_path, self.app_config)?;
        localization::write_bundle_metadata(&app_bundle_path, self.macos_config)?;

        let filename_base = pacor_core::packaging::artifact_base_name(
            self.flutter_project.name(),
            self.flutter_project.version(),
            &self.arch,
        );

        // The bundle is signed before the image is built from it: a signature seals the
        // bytes it contains, so signing afterwards would seal an unsigned copy.
        if let Some(identity) = self.macos_config.codesign_identity.as_deref() {
            let entitlements = self.entitlements_path()?;
            signing::sign_app_bundle(
                identity,
                &app_bundle_path,
                entitlements.as_deref(),
                &command_output_mode,
            )?;
        } else {
            debug!("Code signing disabled: no signing identity configured");
        }

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
        let required_tools = required_tools_for(self.macos_config);
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

/// The tools a build with `macos_config` actually invokes.
///
/// `validate` required `codesign` unconditionally, so an unsigned build failed on a machine
/// without one, while `hdiutil` — without which `create-dmg` cannot make an image — was never
/// checked at all. Notarization is the only step that needs `xcrun`, and it is optional like
/// signing.
fn required_tools_for(macos_config: &MacOSBuildConfig) -> Vec<&'static str> {
    let mut tools = vec!["flutter", "xcodebuild", "hdiutil"];
    if macos_config.codesign_identity.is_some() {
        tools.push("codesign");
    }
    if macos_config.notarize_credentials.is_some() {
        tools.push("xcrun");
    }
    tools
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// A fixture project, its configuration, and the scratch directory holding both.
    ///
    /// The Flutter build directory is deliberately left uncreated: both regressions below are
    /// about what pacor does *around* a build it was told to skip, so the tests stop at the
    /// first step that would need a real toolchain.
    struct Fixture {
        directory: tempfile::TempDir,
        config: PacorConfig,
        macos_config: MacOSBuildConfig,
    }

    impl Fixture {
        fn new() -> Self {
            let directory = tempfile::tempdir().unwrap();
            let project_root = directory.path().join("project");
            fs::create_dir_all(&project_root).unwrap();
            fs::write(
                project_root.join("pubspec.yaml"),
                "name: demo_app\nversion: 1.0.0+1\n",
            )
            .unwrap();

            Self {
                directory,
                config: PacorConfig::default(),
                macos_config: MacOSBuildConfig::default(),
            }
        }

        fn project_root(&self) -> PathBuf {
            self.directory.path().join("project")
        }

        fn build_dir(&self) -> PathBuf {
            self.project_root()
                .join("build/macos/Build/Products/Release")
        }

        fn bundle_path(&self) -> PathBuf {
            self.build_dir().join("demo_app.app")
        }

        fn tools_dir(&self) -> PathBuf {
            self.directory.path().join("tools")
        }

        fn output_dir(&self) -> PathBuf {
            self.directory.path().join("dist")
        }

        fn builder(&self) -> MacOSAppBuilder<'_> {
            MacOSAppBuilder {
                app_config: &self.config.app,
                macos_config: &self.macos_config,
                flutter_project: FlutterProject::load_from_pubspec(&self.project_root()).unwrap(),
                project_root: self.project_root(),
                flutter_build_dir: self.build_dir(),
                output_dir: self.output_dir(),
                tools_dir: self.tools_dir(),
                create_dmg_script_path: self.tools_dir().join("create-dmg.sh"),
                arch: Arch::current().unwrap(),
                build_mode: FlutterBuildMode::Release,
            }
        }
    }

    #[test]
    fn skipping_the_flutter_build_still_installs_the_packaging_tools() {
        let fixture = Fixture::new();
        let builder = fixture.builder();

        // The build directory is absent, so the build stops there: after pacor's own
        // preparation, before anything that would need `bash` or `hdiutil`.
        let Err(error) = builder.build(true, CommandOutputMode::None) else {
            panic!("a missing Flutter build directory must fail the build");
        };
        let error = error.to_string();

        assert!(
            error.contains("Flutter build output directory does not exist"),
            "{error}"
        );
        assert!(
            builder.create_dmg_script_path.is_file(),
            "the create-dmg script must be installed even when Flutter is not run: {}",
            builder.create_dmg_script_path.display()
        );
    }

    #[test]
    fn skipping_the_flutter_build_still_localizes_the_bundle() {
        let fixture = Fixture::new();
        fs::create_dir_all(fixture.build_dir()).unwrap();
        // A bundle with no `Info.plist`: localization is what reads it, so failing on it is
        // how this test observes that the reused-build path localizes at all.
        fs::create_dir_all(fixture.bundle_path().join("Contents/Resources")).unwrap();
        let builder = fixture.builder();

        let Err(error) = builder.build(true, CommandOutputMode::None) else {
            panic!("a bundle without Info.plist must fail the build");
        };
        let error = error.to_string();

        assert!(
            error.contains("Info.plist"),
            "a reused bundle must be localized before it is signed: {error}"
        );
    }

    #[test]
    fn validation_asks_only_for_the_tools_the_configured_build_uses() {
        let unsigned = MacOSBuildConfig {
            codesign_identity: None,
            notarize_credentials: None,
            ..Default::default()
        };
        assert_eq!(
            required_tools_for(&unsigned),
            ["flutter", "xcodebuild", "hdiutil"]
        );

        let signed = MacOSBuildConfig {
            codesign_identity: Some("Developer ID Application: Example".to_string()),
            ..unsigned.clone()
        };
        assert_eq!(
            required_tools_for(&signed),
            ["flutter", "xcodebuild", "hdiutil", "codesign"]
        );

        let notarized = MacOSBuildConfig {
            notarize_credentials: Some("AC_PASSWORD".to_string()),
            ..signed
        };
        assert_eq!(
            required_tools_for(&notarized),
            ["flutter", "xcodebuild", "hdiutil", "codesign", "xcrun"]
        );
    }
}
