//! Android (APK / Android App Bundle) bundler for Flutter projects.
//!
//! This module implements the `BundleBuilder` trait to turn `flutter build apk`
//! and `flutter build appbundle` output into artifacts in the configured output
//! directory. Which of the two is produced is configuration, not a build mode:
//!
//! - `formats = ["apk"]` (the default) writes one APK per configured ABI, which
//!   a device installs directly — websites, MDM, self-update. See [`apk`].
//! - `formats = ["apk", "aab"]` additionally writes the single Android App
//!   Bundle Google Play publishes from. See [`bundle`]. It is opt-in because it
//!   cannot be installed directly and projects that only ship APKs should not
//!   pay for building one.
//!
//! In both cases the artifacts are signed *before* the `.sha1` next to each one
//! is generated, because signing rewrites the file. APKs go through `apksigner`
//! ([`signing`]); a bundle is a JAR and goes through `jarsigner` ([`jarsigner`]).
//! Without a configured keystore the artifact keeps whatever signature Gradle
//! produced, so pacor inspects it and warns when it carries the Android debug
//! certificate.
//!
//! Android always builds release: the build mode is part of the file name
//! rather than of the output directory, so `[build] flutter_build_mode` does not
//! apply here. `[app.i18n]` does not apply either — neither artifact has an
//! installer UI.
//!
//! The builder assumes a standard Flutter Android project layout
//! (`android/` next to `pubspec.yaml`) and needs no tool of its own: `flutter`
//! and, when signing, the Android SDK's `apksigner` plus a JDK's `jarsigner`
//! are the only tools involved.

#[macro_use]
extern crate tracing;

mod apk;
mod bundle;
mod certificate;
mod flutter;
mod jarsigner;
mod jdk;
mod signing;

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use pacor_core::{
    config::{AndroidConfig, PacorConfig},
    core::{
        builder::BundleBuilder,
        project::{FlutterProject, find_flutter_project_root},
    },
    error::ConfigurationError,
    platforms::BuildArtifact,
    utils::{
        command::CommandOutputMode,
        fs::{clean_dir, ensure_directory_exists},
    },
};

pub use pacor_core::core::abi::TargetAbi;

/// Builder for Android platform bundles (APKs and/or an App Bundle).
pub struct AndroidBuilder<'a> {
    android_config: AndroidConfig,
    project_root: PathBuf,
    flutter_build_path: PathBuf,
    output_directory: PathBuf,
    flutter_project: FlutterProject,
    _config: std::marker::PhantomData<&'a PacorConfig>,
}

impl<'a> BundleBuilder for AndroidBuilder<'a> {
    type Config = &'a PacorConfig;
    type Output = Vec<BuildArtifact>;

    fn new(config: Self::Config) -> Result<Self> {
        let android_config = config.platforms.android.clone().unwrap_or_default();

        if !android_config.enabled {
            return Err(ConfigurationError::PlatformNotEnabled {
                platform: "Android".to_string(),
            }
            .into());
        }

        let project_root = find_flutter_project_root()?;
        let flutter_project = FlutterProject::load_from_pubspec(&project_root)?;

        // Deliberately no `Arch::current()` here: APKs land in a single
        // directory whose name does not depend on the host architecture, and
        // asking for the host architecture would make an Android build fail on
        // hosts pacor otherwise supports.
        let flutter_build_path = FlutterProject::android_build_path(&project_root);
        let output_directory = config.output_dir()?;

        Ok(Self {
            android_config,
            project_root,
            flutter_project,
            flutter_build_path,
            output_directory,
            _config: std::marker::PhantomData,
        })
    }

    /// Validates the builder configuration and build environment.
    fn validate(&self) -> Result<()> {
        // Only `flutter` is required. `adb` belongs to debugging and
        // installation, and demanding it made packing fail on machines whose
        // Android SDK is complete but whose `platform-tools` is not on `PATH`.
        let required_tools = ["flutter"];
        let pubspec_path = self.project_root.join("pubspec.yaml");
        let android_path = self.project_root.join("android");
        let required_files = [pubspec_path.as_path(), android_path.as_path()];

        pacor_core::packaging::validate_build_environment(
            "Android",
            &required_tools,
            &required_files,
            &self.output_directory,
        )?;

        validate_formats(&self.android_config)?;
        validate_abis(&self.android_config)?;

        // Validate the signing configuration, including that the tool each
        // selected format needs is reachable, so that a misconfigured keystore
        // fails before Gradle runs.
        if self.android_config.should_sign_apk() {
            if self.android_config.builds_apk() {
                signing::ApkSigner::resolve(&self.android_config, &self.project_root)?;
            }
            if self.android_config.builds_bundle() {
                jarsigner::BundleSigner::resolve(&self.android_config, &self.project_root)?;
            }
        }

        Ok(())
    }

    /// Builds the configured Android artifacts.
    ///
    /// If `skip_flutter_build` is true, the Flutter build step is skipped and
    /// the artifacts already present in Flutter's build directories are
    /// packaged.
    fn build(
        &self,
        skip_flutter_build: bool,
        command_output: CommandOutputMode,
    ) -> Result<Self::Output> {
        info!(
            formats = %self.android_config.format_list(),
            "Starting Android build"
        );

        // `validate` normally ran first; repeating the checks keeps `build` from
        // silently packaging nothing when it is called on its own.
        validate_formats(&self.android_config)?;
        validate_abis(&self.android_config)?;

        // Ensure output directory exists and validate permissions
        pacor_core::packaging::validate_directory_permissions(
            &self.output_directory,
            "Android build output",
        )?;
        ensure_directory_exists(&self.output_directory)?;

        if skip_flutter_build {
            info!("Skipping Flutter app build as requested");
        } else if self.android_config.builds_apk() {
            flutter::build_apk(&self.project_root, &self.android_config, &command_output)?;
        }

        // The bundle is built after the APKs, so a failure in the first
        // (cheaper) Gradle run surfaces before the expensive one.
        if !skip_flutter_build && self.android_config.builds_bundle() {
            flutter::build_app_bundle(&self.project_root, &self.android_config, &command_output)?;
        }

        let mut artifacts = Vec::new();
        if self.android_config.builds_apk() {
            artifacts.extend(self.package_apks(&command_output)?);
        }
        if self.android_config.builds_bundle() {
            artifacts.push(self.package_bundle(&command_output)?);
        }

        info!(
            count = artifacts.len(),
            "Android build completed successfully"
        );
        Ok(artifacts)
    }

    /// Removes the split debug symbols Flutter wrote to `tmp/`.
    ///
    /// They are what `flutter symbolize` needs to turn a stack trace from a
    /// released build back into method names, so they are kept until the
    /// backend's cleanup step — which `--skip-cleanup` skips — instead of being
    /// deleted inside the build, where nothing could preserve them.
    fn clean(&self) -> Result<()> {
        let symbols_dir = self.project_root.join(flutter::SYMBOLS_DIR);
        if symbols_dir.exists() {
            clean_dir(&symbols_dir)?;
            debug!(path = %symbols_dir.display(), "Removed split debug symbols");
        }

        Ok(())
    }

    /// Returns the root path of the Flutter project.
    fn project_root(&self) -> &Path {
        &self.project_root
    }

    /// Returns the Flutter build output path for Android.
    fn flutter_build_path(&self) -> &Path {
        &self.flutter_build_path
    }
}

impl AndroidBuilder<'_> {
    /// Copies every configured ABI's APK into the output directory, signing each
    /// one and generating its checksum.
    fn package_apks(&self, command_output: &CommandOutputMode) -> Result<Vec<BuildArtifact>> {
        let signer = if self.android_config.should_sign_apk() {
            Some(signing::ApkSigner::resolve(
                &self.android_config,
                &self.project_root,
            )?)
        } else {
            None
        };

        let mut artifacts = Vec::with_capacity(self.android_config.target_abi.len());

        for abi in &self.android_config.target_abi {
            let source =
                apk::find_apk(&self.flutter_build_path, abi, self.android_config.flavor())?;
            let target = self.output_directory.join(apk::artifact_file_name(
                self.flutter_project.name(),
                self.flutter_project.version(),
                abi,
            ));

            std::fs::copy(&source, &target).with_context(|| {
                format!(
                    "failed to copy {} to {}",
                    source.display(),
                    target.display()
                )
            })?;
            debug!(
                from = %source.display(),
                to = %target.display(),
                "Collected the APK for {abi}"
            );

            match signer.as_ref() {
                Some(signer) => signer.sign(&target, command_output)?,
                None => self.report_apk_signer(&target),
            }

            // Signing rewrites the file, so the checksum has to be generated
            // after it — the same ordering the Windows backend uses.
            artifacts.push(pacor_core::packaging::create_artifact_with_checksum(
                target,
            )?);
        }

        Ok(artifacts)
    }

    /// Copies the Android App Bundle into the output directory, signing it and
    /// generating its checksum.
    fn package_bundle(&self, command_output: &CommandOutputMode) -> Result<BuildArtifact> {
        let source = bundle::find_bundle(
            &FlutterProject::android_bundle_path(&self.project_root),
            self.android_config.flavor(),
        )?;
        let target = self.output_directory.join(bundle::artifact_file_name(
            self.flutter_project.name(),
            self.flutter_project.version(),
        ));

        std::fs::copy(&source, &target).with_context(|| {
            format!(
                "failed to copy {} to {}",
                source.display(),
                target.display()
            )
        })?;
        debug!(
            from = %source.display(),
            to = %target.display(),
            "Collected the Android App Bundle"
        );

        if self.android_config.should_sign_apk() {
            jarsigner::BundleSigner::resolve(&self.android_config, &self.project_root)?
                .sign(&target, command_output)?;
        } else {
            self.report_bundle_signer(&target);
        }

        // Signed already, so the checksum covers the published bytes.
        let artifact = pacor_core::packaging::create_artifact_with_checksum(target)?;
        Ok(artifact)
    }

    /// Reports who signed an APK pacor did not sign itself.
    ///
    /// Flutter's own project template points release builds at the debug
    /// keystore, so an unsigned-by-pacor artifact is debug signed by default:
    /// Google Play rejects it, and a properly signed release can never upgrade
    /// over it. That is worth saying out loud instead of discovering it at
    /// upload time.
    fn report_apk_signer(&self, apk: &Path) {
        match signing::inspect_apk(&self.android_config, &self.project_root, apk) {
            certificate::Signer::Debug(subject) => warn!(
                apk = %apk.display(),
                "APK carries the Android debug certificate ({subject}); Google Play rejects it and a \
                 properly signed release cannot upgrade over it. Set \
                 `platforms.android.keystore_file` and `platforms.android.key_alias` to sign with \
                 your own key"
            ),
            certificate::Signer::Named(subject) => {
                info!(apk = %apk.display(), "APK signer: {subject}");
            }
            certificate::Signer::Unverified(reason) => {
                warn!(apk = %apk.display(), "apksigner could not verify the APK: {reason}");
            }
            certificate::Signer::Unknown => {
                debug!(apk = %apk.display(), "APK signer could not be inspected");
            }
        }
    }

    /// Reports who signed an App Bundle pacor did not sign itself.
    ///
    /// A bundle is only ever uploaded, so an unsigned or debug-signed one is
    /// rejected at upload time — the last chance to notice is here.
    fn report_bundle_signer(&self, bundle: &Path) {
        match jarsigner::inspect_bundle(&self.android_config, bundle) {
            certificate::Signer::Debug(subject) => warn!(
                bundle = %bundle.display(),
                "App Bundle carries the Android debug certificate ({subject}); Google Play rejects \
                 it. Set `platforms.android.keystore_file` and `platforms.android.key_alias` to sign \
                 with your upload key"
            ),
            certificate::Signer::Named(subject) => {
                info!(bundle = %bundle.display(), "App Bundle signer: {subject}");
            }
            certificate::Signer::Unverified(reason) => {
                warn!(
                    bundle = %bundle.display(),
                    "the App Bundle does not verify ({reason}); Google Play requires an upload-key \
                     signature. Set `platforms.android.keystore_file` to have pacor sign it"
                );
            }
            certificate::Signer::Unknown => {
                debug!(bundle = %bundle.display(), "App Bundle signer could not be inspected");
            }
        }
    }
}

/// Rejects a configuration that would build nothing at all.
///
/// Returns the pacor error itself so the `?` at the call sites picks the
/// conversion; `anyhow`'s blanket conversion would be ambiguous otherwise.
fn validate_formats(config: &AndroidConfig) -> std::result::Result<(), ConfigurationError> {
    if config.formats().is_empty() {
        return Err(ConfigurationError::ValidationFailed {
            field: "platforms.android.formats".to_string(),
            reason: "list at least one format, for example `formats = [\"apk\"]` (or \
                     `[\"apk\", \"aab\"]` to also publish on Google Play)"
                .to_string(),
        });
    }

    Ok(())
}

/// Rejects an ABI list that would make pacor package nothing.
///
/// `target_abi` used to fall back to `arm64-v8a` while Flutter, given no
/// `--target-platform`, built every ABI — so `target_abi = []` produced a
/// silently incomplete release.
fn validate_abis(config: &AndroidConfig) -> std::result::Result<(), ConfigurationError> {
    if config.target_abi.is_empty() {
        return Err(ConfigurationError::ValidationFailed {
            field: "platforms.android.target_abi".to_string(),
            reason: "list at least one ABI, for example `target_abi = [\"arm64-v8a\"]`".to_string(),
        });
    }

    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use pacor_core::config::AndroidPackageFormat;

    #[test]
    fn an_empty_format_list_is_a_configuration_error() {
        let config: AndroidConfig = toml::from_str("formats = []").unwrap();

        let error = validate_formats(&config).unwrap_err().to_string();

        assert!(error.contains("platforms.android.formats"), "{error}");
        assert!(error.contains("at least one format"), "{error}");
        assert!(
            error.contains("aab"),
            "the error mentions the Play option: {error}"
        );
    }

    #[test]
    fn an_empty_abi_list_is_a_configuration_error() {
        let config: AndroidConfig = toml::from_str("target_abi = []").unwrap();

        let error = validate_abis(&config).unwrap_err().to_string();

        assert!(error.contains("platforms.android.target_abi"), "{error}");
        assert!(error.contains("at least one ABI"), "{error}");
    }

    #[test]
    fn the_default_configuration_is_valid() {
        validate_formats(&AndroidConfig::default()).unwrap();
        validate_abis(&AndroidConfig::default()).unwrap();
    }

    /// A cross-platform `pacor.toml` may omit `target_abi`/`formats`; the
    /// defaults then have to be valid, and must not silently add a bundle.
    #[test]
    fn a_section_without_those_keys_uses_the_defaults() {
        let config: AndroidConfig = toml::from_str("enabled = true\nflavor = \"prod\"").unwrap();

        validate_formats(&config).unwrap();
        validate_abis(&config).unwrap();
        assert_eq!(
            config.target_abi,
            std::collections::BTreeSet::from([TargetAbi::Arm64])
        );
        assert_eq!(
            config.formats(),
            &std::collections::BTreeSet::from([AndroidPackageFormat::Apk])
        );
    }

    /// The build paths are Flutter's, independent of the host.
    #[test]
    fn the_flutter_build_paths_are_the_flutter_output_directories() {
        assert_eq!(
            FlutterProject::android_build_path(Path::new("/project")),
            Path::new("/project/build/app/outputs/flutter-apk")
        );
        assert_eq!(
            FlutterProject::android_bundle_path(Path::new("/project")),
            Path::new("/project/build/app/outputs/bundle")
        );
    }
}
