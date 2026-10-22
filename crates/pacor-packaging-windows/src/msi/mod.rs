//! MSI installer generation.
//!
//! The MSI backend produces **per-machine** packages: the payload installs below
//! `%ProgramFiles%`, the registry entries live in `HKLM`, and installing needs
//! administrator rights. That is the opposite of the NSIS installer, which is
//! per-user and needs no elevation; a project should pick one of the two rather
//! than ship both (`docs/MSI.md` explains the trade-off).
//!
//! One locale, one MSI: `light` rejects several localization files that define
//! the same identifiers (`LGHT0100`), so a multi-language project gets one
//! artifact per configured locale instead of one shared file.
//!
//! The pipeline, per build:
//!
//! 1. fetch the pinned WiX toolset and its templates into the cache;
//! 2. harvest the build output into `payload.wxs` (deterministic component GUIDs);
//! 3. render `main.wxs` and one `.wxl` per locale;
//! 4. `candle -arch <x64|arm64>` compiles the sources, `light` links one MSI per locale;
//! 5. `light`'s exit code decides success — it writes the MSI *before* reporting
//!    ICE errors, so a file existing proves nothing.

mod generator;
pub(super) mod harvest;
pub(super) mod toolchain;

use std::path::{Path, PathBuf};

use anyhow::Result;

use pacor_core::{
    config::{AppConfig, WindowsConfig},
    core::{arch::Arch, project::FlutterProject},
    error::{BuildError, PlatformError},
    packaging::{artifact_name, create_artifact_with_checksum},
    platforms::BuildArtifact,
    utils::command::{CommandOutputMode, execute_command, macros::command_args},
};

use crate::msi::generator::MsiGenerator;

/// ICE warnings that are expected for an application installer and therefore
/// suppressed; every other ICE result stays a failure.
///
/// * `ICE60`: Flutter assets include `.otf` fonts, which MSI expects to carry a
///   language column because they could be installed as system fonts;
/// * `ICE61`: `AllowSameVersionUpgrades` makes the upgrade range include the
///   current version, which is exactly what a `+build`-only bump needs.
const SUPPRESSED_ICE: &[&str] = &["ICE60", "ICE61"];

pub(super) struct MsiBuilder<'a> {
    app_config: &'a AppConfig,
    windows_config: &'a WindowsConfig,
    flutter_project: &'a FlutterProject,

    /// Architecture the payload was built for; part of the artifact name.
    arch: &'a Arch,

    /// Directory holding the downloaded WiX toolset and the pinned templates.
    wix_dir: &'a Path,

    icon_path: &'a Path,
    executable_name: &'a str,
}

/// The complete argument vector of one `candle` invocation.
///
/// Split out of [`MsiBuilder::compile_source`] so a test can assert it whole: `-arch` decides
/// whether the package is registered as a 64-bit ARM or an Intel one, and a wrong value does
/// not fail the build — it produces an installer that installs and repairs against the wrong
/// platform. `-out` is given a file name because that is the documented form for a single
/// input, and the `-d` variables are what keep machine-specific paths out of the generated
/// `payload.wxs`.
fn candle_args(
    arch: &Arch,
    temp_dir: &Path,
    source: &Path,
    object_name: &str,
    build_output_dir: &Path,
    icon_path: &Path,
) -> Vec<std::ffi::OsString> {
    let mut args = command_args!(capacity = 12, "-nologo", "-arch", arch.as_str());
    args.push("-ext".into());
    args.push("WixUIExtension".into());
    args.push(format!("-dSourceDir={}", build_output_dir.display()).into());
    args.push(format!("-dIconPath={}", icon_path.display()).into());
    args.push("-out".into());
    args.push(temp_dir.join(object_name).into_os_string());
    args.push(source.as_os_str().into());
    args
}
impl<'a> MsiBuilder<'a> {
    pub(super) fn new(
        app_config: &'a AppConfig,
        windows_config: &'a WindowsConfig,
        flutter_project: &'a FlutterProject,
        arch: &'a Arch,
        wix_dir: &'a Path,
        icon_path: &'a Path,
        executable_name: &'a str,
    ) -> Self {
        Self {
            app_config,
            windows_config,
            flutter_project,
            arch,
            wix_dir,
            icon_path,
            executable_name,
        }
    }

    /// Builds one MSI per configured locale.
    pub(super) fn build(
        &self,
        build_output_dir: &Path,
        installer_output_dir: &Path,
        command_output_mode: &CommandOutputMode,
    ) -> Result<Vec<BuildArtifact>> {
        pacor_core::packaging::validate_directory_permissions(
            installer_output_dir,
            "MSI installer output",
        )?;

        let toolset = toolchain::ensure(self.wix_dir)?;

        let upgrade_code = self.windows_config.resolved_upgrade_code(
            self.flutter_project.name(),
            self.app_config.publisher().unwrap_or_default(),
        );
        tracing::debug!(
            upgrade_code = %upgrade_code,
            configured = self.windows_config.msi_upgrade_code.is_some(),
            "Resolved the MSI upgrade code"
        );

        let generator = MsiGenerator {
            app_config: self.app_config,
            windows_config: self.windows_config,
            app_name: self.flutter_project.name(),
            version: self.flutter_project.version(),
            executable_name: self.executable_name,
            icon_path: self.icon_path,
            upgrade_code: &upgrade_code,
            wix_dir: self.wix_dir,
            payload_component_group: harvest::PAYLOAD_COMPONENT_GROUP,
        };

        let temp_dir = tempfile::TempDir::with_prefix("pacor-msi-")?;
        let scripts = generator.write_sources(temp_dir.path(), build_output_dir)?;

        // The payload is locale-independent, so it is compiled once and reused
        // by every localization; only `main.wxs` carries the product language.
        self.compile_source(
            &toolset,
            temp_dir.path(),
            &scripts.payload,
            "payload.wixobj",
            build_output_dir,
            command_output_mode,
        )?;

        let mut artifacts = Vec::with_capacity(scripts.locales.len());
        for locale_scripts in &scripts.locales {
            let locale = &locale_scripts.locale;
            let culture = locale.as_wix_culture();
            let object_name = format!("{culture}-main.wixobj");
            self.compile_source(
                &toolset,
                temp_dir.path(),
                &locale_scripts.main,
                &object_name,
                build_output_dir,
                command_output_mode,
            )?;

            let file_name = artifact_name(
                self.flutter_project.name(),
                self.flutter_project.version(),
                self.arch,
                locale.as_str(),
                "msi",
            );
            let installer_path = installer_output_dir.join(file_name);

            self.link_installer(
                &toolset,
                temp_dir.path(),
                locale_scripts,
                &object_name,
                &installer_path,
                command_output_mode,
            )?;

            let artifact = create_artifact_with_checksum(installer_path)?;
            tracing::info!(
                path = %artifact.file().display(),
                locale = %locale,
                "MSI installer built successfully"
            );
            artifacts.push(artifact);
        }

        Ok(artifacts)
    }

    /// `candle -arch <x64|arm64>`: compiles one source file into one object.
    ///
    /// `-out` is given a file name because that is the documented form for a
    /// single input, and the `-d` variables are what keep machine-specific paths
    /// out of the generated `payload.wxs`.
    fn compile_source(
        &self,
        toolset: &toolchain::WixToolset,
        temp_dir: &Path,
        source: &Path,
        object_name: &str,
        build_output_dir: &Path,
        command_output_mode: &CommandOutputMode,
    ) -> Result<()> {
        let args = candle_args(
            self.arch,
            temp_dir,
            source,
            object_name,
            build_output_dir,
            self.icon_path,
        );

        tracing::info!(source = %source.display(), "Compiling the MSI sources with candle");
        execute_command(
            &toolset.candle,
            &args,
            None,
            None::<&[(&str, &str)]>,
            command_output_mode,
        )
        .map_err(|error| PlatformError::BuildFailed {
            platform: "Windows".to_string(),
            reason: format!("candle failed for {}: {error}", source.display()),
        })?;

        Ok(())
    }

    /// `light`: links one locale's objects into its own MSI.
    fn link_installer(
        &self,
        toolset: &toolchain::WixToolset,
        temp_dir: &Path,
        locale_scripts: &generator::MsiLocaleScripts,
        main_object: &str,
        installer_path: &Path,
        command_output_mode: &CommandOutputMode,
    ) -> Result<()> {
        let culture = locale_scripts.locale.as_wix_culture();
        let culture_file = locale_scripts.culture.as_path();
        let mut args = command_args!(capacity = 14, "-nologo", "-ext", "WixUIExtension");
        args.push(format!("-cultures:{culture}").into());
        args.push("-loc".into());
        args.push(culture_file.as_os_str().into());
        for ice in SUPPRESSED_ICE {
            args.push(format!("-sice:{ice}").into());
        }
        // `light` otherwise drops a `.wixpdb` binder database next to the MSI.
        // It is a debugging aid, so it belongs in the temporary directory rather
        // than among the artifacts the user is handed.
        args.push("-pdbout".into());
        args.push(temp_dir.join(format!("{culture}.wixpdb")).into_os_string());
        args.push("-out".into());
        args.push(installer_path.as_os_str().into());
        args.push(temp_dir.join(main_object).into_os_string());
        args.push(temp_dir.join("payload.wixobj").into_os_string());

        tracing::info!(culture, "Linking the MSI with light...");
        execute_command(
            &toolset.light,
            &args,
            None,
            None::<&[(&str, &str)]>,
            command_output_mode,
        )
        .map_err(|error| PlatformError::BuildFailed {
            platform: "Windows".to_string(),
            reason: format!("light failed: {error}"),
        })?;

        // Belt and braces: `light` writes the file before it reports ICE
        // failures, so a missing file is worth its own message.
        if !installer_path.is_file() {
            return Err(BuildError::ArtifactGeneration {
                artifact: installer_path.display().to_string(),
                reason: "light did not produce the MSI".to_string(),
            }
            .into());
        }

        Ok(())
    }

    /// Where the WiX toolset and templates for this build are cached.
    pub(super) fn cache_dir(tools_dir: &Path) -> PathBuf {
        tools_dir.join("wix").join(pacor_core::tools::TEMPLATE_REF)
    }

    /// Whether the toolset still has to be downloaded.
    pub(super) fn is_cached(tools_dir: &Path) -> bool {
        toolchain::is_cached(&Self::cache_dir(tools_dir))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use pacor_core::i18n::Locale;

    use std::fs;

    use crate::test_support::{app_config, demo_project, require_wix_toolset, write_minimal_icon};

    /// `candle` was told `-arch x64` whatever the payload was, so an MSI for an ARM64 build
    /// declared Intel64: Windows then installs it as a 32-bit-on-ARM package and repairs it
    /// from the wrong source, instead of the build failing.
    #[test]
    fn candle_is_told_the_architecture_of_the_payload() {
        use pacor_core::core::arch::Arch;

        let temp_dir = Path::new("temp");
        let source = Path::new("payload.wxs");
        let build_output_dir = Path::new("build/windows/arm64/runner/Release");
        let icon_path = Path::new("app_icon.ico");
        let object_path = temp_dir.join("payload.wixobj");

        let args = candle_args(
            &Arch::ARM64,
            temp_dir,
            source,
            "payload.wixobj",
            build_output_dir,
            icon_path,
        );
        let rendered: Vec<String> = args
            .iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();

        assert_eq!(
            rendered,
            vec![
                "-nologo".to_string(),
                "-arch".to_string(),
                "arm64".to_string(),
                "-ext".to_string(),
                "WixUIExtension".to_string(),
                format!("-dSourceDir={}", build_output_dir.display()),
                format!("-dIconPath={}", icon_path.display()),
                "-out".to_string(),
                object_path.display().to_string(),
                source.display().to_string(),
            ]
        );
    }

    #[test]
    fn the_cache_directory_carries_the_pinned_tag() {
        let dir = MsiBuilder::cache_dir(Path::new("/cache/windows"));

        assert!(dir.parent().unwrap().ends_with("wix"), "{}", dir.display());
        assert_eq!(
            dir.file_name().unwrap(),
            pacor_core::tools::TEMPLATE_REF,
            "the tag is part of the path, which is what invalidates stale copies"
        );
    }

    #[test]
    fn the_ice_whitelist_stays_deliberately_short() {
        // Every entry is a decision documented in docs/MSI.md; a growing list
        // would mean the ICE suite is no longer a safety net.
        assert_eq!(SUPPRESSED_ICE, ["ICE60", "ICE61"]);
    }

    /// Builds a real MSI for two locales, then checks it the way CI would.
    ///
    /// Ignored by default because it needs a local WiX toolset plus the pinned
    /// templates; see [`require_wix_toolset`] and `docs/MSI.md`. The assertions
    /// cover what the spike established the hard way: `light` has to exit 0
    /// (ICE included), the artifact name carries the locale, and an
    /// administrative install has to reproduce the build output file for file.
    #[test]
    #[ignore = "requires a WiX toolset; set PACOR_MSI_TOOLCHAIN_DIR"]
    fn an_msi_is_built_end_to_end() {
        use pacor_core::config::WindowsConfig;
        use pacor_core::core::arch::Arch;

        // Copy the prepared toolset so the test can add the extraction marker
        // without touching the caller's directory.
        let toolset = require_wix_toolset();
        let temp = tempfile::TempDir::new().unwrap();
        let cache = temp.path().join("wix-cache");
        fs::create_dir_all(&cache).unwrap();
        pacor_core::utils::fs::copy_dir(&toolset, &cache).unwrap();
        fs::write(cache.join(".extracted"), "").unwrap();

        // The build output is a synthetic payload: a real Flutter application
        // would make the test slow and its contents irrelevant.
        let project_root = temp.path().join("project");
        let build_dir = project_root.join("build/windows/x64/runner/Release");
        fs::create_dir_all(build_dir.join("data/flutter_assets")).unwrap();
        fs::write(build_dir.join("my_app.exe"), b"stub executable").unwrap();
        fs::write(build_dir.join("data/app.so"), b"stub library").unwrap();
        fs::write(
            build_dir.join("data/flutter_assets/AssetManifest.bin"),
            b"stub",
        )
        .unwrap();

        let icon_path = temp.path().join("app_icon.ico");
        write_minimal_icon(&icon_path);

        let output_dir = temp.path().join("dist");
        fs::create_dir_all(&output_dir).unwrap();

        let app_config = app_config(&[Locale::EnUs, Locale::ZhCn]);
        // The Chinese metadata in `test_support` carries a `©`, which code page
        // 936 cannot represent; UTF-8 keeps the fixture representative of an
        // application with real-world copyright text.
        let windows_config = WindowsConfig {
            msi_codepage: Some(65001),
            ..WindowsConfig::default()
        };
        let builder = MsiBuilder::new(
            &app_config,
            &windows_config,
            demo_project(),
            &Arch::X64,
            &cache,
            &icon_path,
            "my_app.exe",
        );

        let artifacts = builder
            .build(&build_dir, &output_dir, &CommandOutputMode::None)
            .expect("the MSI build must succeed");

        assert_eq!(artifacts.len(), 2, "one MSI per locale");

        for (artifact, locale) in artifacts.iter().zip(["en-US", "zh-CN"]) {
            assert_eq!(
                artifact.file(),
                &output_dir.join(format!("my_app-1.0.0+1-x64-{locale}.msi")),
            );
            assert!(
                fs::metadata(artifact.file()).unwrap().len() > 0,
                "the MSI is empty"
            );
            assert!(artifact.sha1().is_file(), "the checksum is missing");

            // `light` drops a binder database next to its output unless it is
            // told otherwise, and it must not end up among the artifacts.
            assert!(
                !artifact.file().with_extension("wixpdb").exists(),
                "the binder database leaked into the output directory"
            );

            assert_extracted_payload_matches(artifact.file(), &build_dir, "my_app");
        }
    }

    /// Runs an administrative install and compares it with `build_dir`.
    ///
    /// An administrative install (`msiexec /a`) unpacks the package without
    /// changing the machine, which makes it the one MSI check that needs no
    /// administrator rights — a real installation does.
    fn assert_extracted_payload_matches(msi: &Path, build_dir: &Path, app_name: &str) {
        let target = tempfile::TempDir::new().unwrap();
        let status = std::process::Command::new("msiexec.exe")
            .arg("/a")
            .arg(msi)
            .arg("/qn")
            .arg(format!("TARGETDIR={}", target.path().display()))
            .status()
            .expect("msiexec must be runnable");

        assert!(status.success(), "msiexec /a failed with {status}");

        let installed = target.path().join(app_name);
        let mut expected = relative_files(build_dir);
        let mut actual = relative_files(&installed);
        expected.sort();
        actual.sort();

        assert_eq!(
            actual, expected,
            "the extracted payload does not match the build output"
        );
    }

    /// Every file below `root`, as paths relative to it with `/` separators.
    fn relative_files(root: &Path) -> Vec<String> {
        let mut files = Vec::new();
        let mut pending = vec![root.to_path_buf()];

        while let Some(directory) = pending.pop() {
            for entry in fs::read_dir(&directory).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    pending.push(path);
                } else {
                    files.push(
                        path.strip_prefix(root)
                            .expect("walked paths live below the root")
                            .to_string_lossy()
                            .replace('\\', "/"),
                    );
                }
            }
        }

        files
    }
}
