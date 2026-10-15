//! NSIS (Nullsoft Scriptable Install System) toolchain handling.
//!
//! Compiling the pinned template needs three things in the right place:
//!
//! * `template.nsi`, read from pacor's cache;
//! * the per-language string files from the pinned `pacor-rs/tools` tag, which
//!   define the `SECTION_*`, `DESC_*`, `SHORTCUT_*` and `MSG_*` LangStrings the
//!   template uses (NSIS itself does not provide those);
//! * the `nsProcess` plugin, which is *not* part of NSIS: `nsProcess.nsh` must
//!   be reachable by `!include "nsProcess.nsh"` and the DLL must sit in
//!   `<NSISDIR>\Plugins\x86-unicode`, the only plugin directory `makensis`
//!   searches for the Unicode installer the template compiles.
//!
//! Downloading these files into the cache is therefore not the whole job: the
//! plugin has to be staged into whichever NSIS installation is used.

use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    thread,
};

use anyhow::{Context, Result};

use pacor_core::{
    config::AppConfig,
    core::{arch::Arch, project::FlutterProject},
    error::{BuildError, Error, PlatformError},
    platforms::BuildArtifact,
    utils::{
        archive::extract_zip,
        command::{CommandOutputMode, execute_command, find_command_path, macros::command_args},
        downloader::download_file,
        fs::ensure_directory_exists,
    },
};

use crate::nsis::generator::{NSISGenerator, nsis_lang_code};

mod generator;

/// NSIS distribution used when no usable `makensis` is installed.
///
/// The same build is used by the templates' author, so the pinned templates are
/// compiled by a known NSIS version with the Unicode plugin layout they expect.
const NSIS_DOWNLOAD_URL: &str =
    "https://github.com/tauri-apps/binary-releases/releases/download/nsis-3/nsis-3.zip";

/// Template files needed to compile an installer, as
/// `(path in the pinned tag, path inside pacor's cache)`.
const NSIS_TEMPLATE_FILES: &[(&str, &str)] = &[
    ("template.nsi", "template.nsi"),
    ("Plugins/nsProcess.nsh", "Plugins/nsProcess.nsh"),
    ("Plugins/nsProcess.dll", "Plugins/nsProcess.dll"),
];

/// Plugin directory `makensis` reads for a `Unicode True` installer.
const NSIS_PLUGIN_DIR: &str = "Plugins/x86-unicode";

/// Include file the template pulls in with `!include "nsProcess.nsh"`.
const NSPROCESS_INCLUDE: &str = "nsProcess.nsh";

/// Plugin DLL the template calls through that include.
const NSPROCESS_DLL: &str = "nsProcess.dll";

pub(super) struct NsisBuilder<'a> {
    /// Common builder functionality
    app_config: &'a AppConfig,
    project_root: &'a Path,
    flutter_project: &'a FlutterProject,
    nsis_install_dir: &'a Path,
    executable_name: &'a str,
    default_legal_copyright: &'a str,
    icon_path: &'a Path,
}

impl<'a> NsisBuilder<'a> {
    pub(super) fn new(
        app_config: &'a AppConfig,
        project_root: &'a Path,
        flutter_project: &'a FlutterProject,
        nsis_install_dir: &'a Path,
        executable_name: &'a str,
        icon_path: &'a Path,
        default_legal_copyright: &'a str,
    ) -> Self {
        Self {
            app_config,
            project_root,
            flutter_project,
            nsis_install_dir,
            executable_name,
            icon_path,
            default_legal_copyright,
        }
    }

    /// Template files of the pinned tag that an installer needs, as
    /// `(path in the tag, path inside pacor's cache)`.
    ///
    /// Besides the template and the plugin this covers the language files for
    /// every configured locale: they define the `SECTION_*`, `DESC_*` and
    /// `MSG_*` LangStrings the template uses, and the rendered script includes
    /// them from the cache. Two locales can share one file (`en-US` and `en-GB`
    /// are both `English`), so the list is de-duplicated.
    fn required_template_files(&self) -> Vec<(String, String)> {
        let mut files: Vec<(String, String)> = NSIS_TEMPLATE_FILES
            .iter()
            .map(|(remote, local)| ((*remote).to_string(), (*local).to_string()))
            .collect();

        let languages: BTreeSet<&'static str> =
            self.app_config.i18n().keys().map(nsis_lang_code).collect();

        for lang_code in languages {
            let path = format!("languages/{lang_code}.nsh");
            files.push((path.clone(), path));
        }

        files
    }

    /// Downloads the files of the pinned tag that an installer needs.
    ///
    /// Files that already exist are kept: the tag is immutable, and the cache
    /// directory carries the tag in its path, so a stale copy is impossible.
    fn ensure_template_files(&self) -> Result<()> {
        info!(
            repository = pacor_core::tools::TOOLS_REPOSITORY,
            tag = pacor_core::tools::TEMPLATE_REF,
            "Preparing NSIS templates"
        );

        let cache_dir = self.nsis_install_dir.to_path_buf();

        let pending: Vec<(String, PathBuf)> = self
            .required_template_files()
            .into_iter()
            .filter_map(|(remote, local)| {
                let destination = cache_dir.join(&local);

                if destination.exists() {
                    debug!(path = %destination.display(), "Template file already cached");
                    return None;
                }

                let url = pacor_core::tools::template_url(&format!("windows/nsis/{remote}"));
                Some((url, destination))
            })
            .collect();

        if pending.is_empty() {
            return Ok(());
        }

        // Create every parent directory before downloading, so a failure is
        // reported instead of dropping a file that the script will need.
        for (_, destination) in &pending {
            if let Some(parent) = destination.parent() {
                ensure_directory_exists(parent)?;
            }
        }

        debug!(files = pending.len(), "Downloading NSIS template files");

        thread::scope(|scope| {
            let handles: Vec<_> = pending
                .into_iter()
                .map(|(url, destination)| scope.spawn(move || download_to(&url, &destination)))
                .collect();

            for handle in handles {
                handle.join().map_err(|panic| {
                    Error::Threading(format!("Download thread panicked: {panic:?}"))
                })??;
            }

            Ok::<(), anyhow::Error>(())
        })?;

        Ok(())
    }

    /// Downloads and extracts the bundled NSIS if it is not cached yet.
    fn ensure_bundled_nsis(&self) -> Result<()> {
        let marker_file = self.nsis_install_dir.join(".extracted");

        if marker_file.exists() {
            debug!(nsis_dir = %self.nsis_install_dir.display(), "NSIS already installed");
            return Ok(());
        }

        info!("Installing NSIS...");
        let archive_bytes = download_file(NSIS_DOWNLOAD_URL)?;
        ensure_directory_exists(self.nsis_install_dir)?;

        trace!(nsis_dir = %self.nsis_install_dir.display(), "Extracting NSIS");
        extract_zip(&archive_bytes, self.nsis_install_dir)?;

        fs::write(&marker_file, "")?; // Mark as extracted

        info!(path = %self.nsis_install_dir.display(), "NSIS installed successfully");
        Ok(())
    }

    /// Copies the plugin DLL into the plugin directory `makensis` searches.
    fn install_plugin_dll(&self, install_dir: &Path) -> Result<()> {
        install_plugin_dll(self.nsis_install_dir, install_dir)
    }

    /// Picks the NSIS installation to compile with, returning its `makensis`.
    ///
    /// A `makensis` from `PATH` wins when its plugin directory can be written
    /// to; otherwise pacor falls back to its own pinned copy, which is always
    /// complete and never requires elevated permissions.
    fn resolve_makensis(&self) -> Result<PathBuf> {
        if let Some(makensis) = find_command_path("makensis")?
            && let Some(install_dir) = makensis.parent()
        {
            match self.install_plugin_dll(install_dir) {
                Ok(()) => {
                    info!(path = %makensis.display(), "Using system-installed makensis");
                    return Ok(makensis);
                }
                Err(error) => warn!(
                    path = %makensis.display(),
                    error = %error,
                    "The system-wide NSIS installation cannot take the nsProcess plugin; \
                     using pacor's pinned copy instead"
                ),
            }
        }

        self.ensure_bundled_nsis()?;
        let install_dir = self.nsis_install_dir.to_path_buf();
        self.install_plugin_dll(&install_dir)?;

        let makensis = install_dir.join("makensis.exe");
        if !makensis.exists() {
            return Err(PlatformError::ToolNotFound {
                tool: "makensis".to_string(),
                platform: "Windows".to_string(),
            }
            .into());
        }

        info!(path = %makensis.display(), "Using bundled makensis");
        Ok(makensis)
    }

    pub(super) fn build(
        &self,
        project_name: &str,
        version: &str,
        target_arch: &Arch,
        build_output_dir: &Path,
        installer_output_dir: &Path,
        command_output_mode: &CommandOutputMode,
    ) -> Result<BuildArtifact> {
        // Use common validation functionality
        pacor_core::packaging::validate_directory_permissions(
            installer_output_dir,
            "NSIS installer output",
        )?;

        self.ensure_template_files()?;
        let makensis = self.resolve_makensis()?;

        let setup_filename = pacor_core::packaging::artifact_name(
            project_name,
            version,
            target_arch,
            "setup",
            "exe",
        );
        let installer_path = installer_output_dir.join(&setup_filename);

        // The script and the plugin include are generated per build: the script
        // pins absolute paths into the cache, and the include has to sit next
        // to it for `makensis` to find it.
        let temp_dir = tempfile::TempDir::with_prefix("pacor-nsis-")?;
        stage_plugin_include(self.nsis_install_dir, temp_dir.path())?;

        let nsis_generator = NSISGenerator {
            app_config: self.app_config,
            default_description: self.flutter_project.description(),
            app_name: self.flutter_project.name(),
            version: self.flutter_project.version(),
            installer_name: &setup_filename,
            icon_path: self.icon_path,
            nsis_install_dir: self.nsis_install_dir,
            build_output_dir,
            installer_output_dir,
            executable_name: self.executable_name,
            default_legal_copyright: self.default_legal_copyright,
        };

        let nsis_script_path = nsis_generator.generate(temp_dir.path())?;

        let nsis_args = command_args!["/INPUTCHARSET", "UTF8", "/V3", &nsis_script_path];

        info!("Running makensis to build installer...");
        execute_command(
            &makensis,
            &nsis_args,
            Some(self.project_root),
            None::<&[(&str, &str)]>,
            command_output_mode,
        )?;

        if !installer_path.exists() {
            error!(
                path = %installer_path.display(),
                "Installer binary not found after running makensis",
            );

            return Err(BuildError::ArtifactGeneration {
                artifact: setup_filename,
                reason: "Installer binary not found after running makensis".to_string(),
            }
            .into());
        }

        debug!(path = %installer_path.display(), "Installer binary created");

        // Generate SHA1 checksum file using common functionality
        trace!("Generating SHA1 checksum file...");
        let artifact = pacor_core::packaging::create_artifact_with_checksum(installer_path)?;
        debug!(path = %artifact.sha1().display(), "SHA1 checksum file generated");

        info!(path = %artifact.file().display(), "NSIS installer built successfully");
        Ok(artifact)
    }
}

/// Download `url` and store it at `destination`.
fn download_to(url: &str, destination: &Path) -> Result<()> {
    debug!(url, path = %destination.display(), "Downloading template file");

    let data = download_file(url).with_context(|| format!("failed to download {url}"))?;
    fs::write(destination, &data)
        .with_context(|| format!("failed to write {}", destination.display()))?;

    trace!(path = %destination.display(), bytes = data.len(), "Template file cached");
    Ok(())
}

/// Copies the cached plugin DLL into the plugin directory `makensis` searches.
///
/// `makensis` only looks for plugins in `<NSISDIR>\Plugins\<platform>`, so a
/// copy in the cache is not enough on its own. That directory ships with NSIS 3;
/// requiring it also rejects an NSIS 2 installation, whose flat `Plugins`
/// directory cannot compile the `Unicode True` template at all.
///
/// # Errors
/// Fails when the NSIS installation has no such directory or cannot be written
/// to, which is how a system-wide install is detected and where the caller falls
/// back to pacor's pinned copy.
fn install_plugin_dll(nsis_install_dir: &Path, install_dir: &Path) -> Result<()> {
    let source = nsis_install_dir.join("Plugins").join(NSPROCESS_DLL);
    let target_dir = install_dir.join(NSIS_PLUGIN_DIR);
    let target = target_dir.join(NSPROCESS_DLL);

    if !source.exists() {
        return Err(PlatformError::FileNotFound { path: source }.into());
    }

    if !target_dir.is_dir() {
        return Err(PlatformError::InvalidConfiguration {
            platform: "Windows".to_string(),
            reason: format!(
                "{} does not look like an NSIS 3 installation: {} is missing",
                install_dir.display(),
                NSIS_PLUGIN_DIR
            ),
        }
        .into());
    }

    // Re-copying on every run is cheap (a few KiB) and repairs installs made by
    // an older pacor, but it is skipped when the copy is already current.
    if let (Ok(source), Ok(target_meta)) = (fs::metadata(&source), fs::metadata(&target))
        && source.len() == target_meta.len()
    {
        trace!(path = %target.display(), "NSIS plugin already installed");
        return Ok(());
    }

    fs::copy(&source, &target).with_context(|| {
        format!(
            "cannot install the nsProcess plugin into the NSIS installation at {}",
            install_dir.display()
        )
    })?;

    debug!(target = %target.display(), "Installed NSIS plugin");
    Ok(())
}

/// Copies the cached plugin include next to the generated script.
///
/// `!include "nsProcess.nsh"` resolves relative to the script's directory (and
/// `${NSISDIR}\Include`), so staging the include with the script avoids writing
/// into a system-wide NSIS installation.
fn stage_plugin_include(nsis_install_dir: &Path, script_dir: &Path) -> Result<()> {
    let source = nsis_install_dir.join("Plugins").join(NSPROCESS_INCLUDE);

    if !source.exists() {
        return Err(PlatformError::FileNotFound { path: source }.into());
    }

    let target = script_dir.join(NSPROCESS_INCLUDE);
    fs::copy(&source, &target)
        .with_context(|| format!("failed to stage {} for makensis", target.display()))?;

    trace!(path = %target.display(), "Staged NSIS plugin include");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::test_support::{
        app_config, demo_project, require_nsis_toolchain, write_minimal_icon,
    };
    use pacor_core::i18n::Locale;

    /// A builder whose paths are only ever inspected, never touched.
    fn builder(app_config: &AppConfig) -> NsisBuilder<'_> {
        NsisBuilder::new(
            app_config,
            Path::new("/project"),
            demo_project(),
            Path::new("/cache/nsis/v1"),
            "my_app.exe",
            Path::new("/project/windows/runner/resources/app_icon.ico"),
            "Copyright",
        )
    }

    /// Every template file must be fetched from the pinned tag, never a branch.
    #[test]
    fn template_files_come_from_the_pinned_tag() {
        for (remote, local) in NSIS_TEMPLATE_FILES {
            let url = pacor_core::tools::template_url(&format!("windows/nsis/{remote}"));

            assert!(url.contains(pacor_core::tools::TEMPLATE_REF), "{url}");
            assert!(url.contains("/refs/tags/"), "{url}");
            assert!(!url.contains("/refs/heads/"), "{url}");
            assert!(!local.is_empty(), "{remote} needs a cache path");
        }
    }

    /// The rendered script includes `languages/<NSIS language>.nsh` from the
    /// cache, so exactly those files have to be downloaded — one per locale,
    /// shared by locales that map to the same NSIS language.
    #[test]
    fn every_configured_locale_gets_its_language_file() {
        let config = app_config(&[Locale::EnUs, Locale::ZhCn, Locale::EnGb, Locale::JaJp]);
        let files = builder(&config).required_template_files();

        for expected in [
            "template.nsi",
            "Plugins/nsProcess.nsh",
            "Plugins/nsProcess.dll",
            "languages/English.nsh",
            "languages/SimpChinese.nsh",
            "languages/Japanese.nsh",
        ] {
            assert!(
                files
                    .iter()
                    .any(|(remote, local)| remote == expected && local == expected),
                "{expected} is missing from {files:?}"
            );
        }

        // `en-US` and `en-GB` share one file, and downloading the same path
        // twice in parallel would race.
        assert_eq!(
            files
                .iter()
                .filter(|(_, local)| local == "languages/English.nsh")
                .count(),
            1
        );
        assert_eq!(files.len(), 6, "{files:?}");
    }

    /// `makensis` only searches `<NSISDIR>\Plugins\<platform>` for plugins, and
    /// the template compiles a Unicode installer, so the DLL must end up in
    /// `Plugins/x86-unicode`. Getting this wrong made every installer build fail
    /// with "Plugin not found, cannot call nsProcess::_FindProcess".
    #[test]
    fn the_plugin_dll_target_is_the_unicode_plugin_directory() {
        let install_dir = Path::new("/nsis");
        let target = install_dir.join(NSIS_PLUGIN_DIR).join(NSPROCESS_DLL);

        assert_eq!(target, Path::new("/nsis/Plugins/x86-unicode/nsProcess.dll"));
        assert!(
            NSIS_PLUGIN_DIR.starts_with("Plugins"),
            "the plugin directory must be relative to $NSISDIR: {NSIS_PLUGIN_DIR}"
        );
    }

    /// The template includes the plugin by bare name, so the include is looked
    /// up next to the script.
    #[test]
    fn the_plugin_include_is_staged_by_bare_name() {
        assert_eq!(NSPROCESS_INCLUDE, "nsProcess.nsh");
    }

    /// Seed a cache directory with the files `ensure_template_files` downloads.
    fn cache_with_plugin_files(temp: &Path) -> PathBuf {
        let cache = temp.join("cache");
        fs::create_dir_all(cache.join("Plugins")).unwrap();
        fs::write(cache.join("Plugins").join(NSPROCESS_INCLUDE), b"!define").unwrap();
        fs::write(cache.join("Plugins").join(NSPROCESS_DLL), b"dll-bytes").unwrap();
        cache
    }

    #[test]
    fn the_plugin_dll_is_installed_into_the_nsis_plugin_directory() {
        let temp = tempfile::TempDir::new().unwrap();
        let cache = cache_with_plugin_files(temp.path());
        let install_dir = temp.path().join("nsis");
        // Emulates the extracted NSIS distribution, which ships an empty
        // `Plugins\x86-unicode` directory.
        fs::create_dir_all(install_dir.join(NSIS_PLUGIN_DIR)).unwrap();

        install_plugin_dll(&cache, &install_dir).unwrap();

        assert_eq!(
            fs::read(install_dir.join(NSIS_PLUGIN_DIR).join(NSPROCESS_DLL)).unwrap(),
            b"dll-bytes"
        );
    }

    /// An installation without NSIS 3's Unicode plugin directory cannot compile
    /// the template, so it is rejected instead of being patched up.
    #[test]
    fn an_installation_without_the_unicode_plugin_directory_is_rejected() {
        let temp = tempfile::TempDir::new().unwrap();
        let cache = cache_with_plugin_files(temp.path());
        let install_dir = temp.path().join("nsis");
        fs::create_dir_all(&install_dir).unwrap();

        let error = install_plugin_dll(&cache, &install_dir).unwrap_err();

        assert!(error.to_string().contains("NSIS 3"), "{error}");
        assert!(
            !install_dir.join(NSIS_PLUGIN_DIR).exists(),
            "nothing may be created inside an unknown installation"
        );
    }

    /// A read-only NSIS installation is what makes `resolve_makensis` fall back
    /// to the pinned copy, so this path has to fail rather than be skipped.
    #[test]
    fn a_missing_plugin_source_is_an_error() {
        let temp = tempfile::TempDir::new().unwrap();
        let error = install_plugin_dll(&temp.path().join("empty-cache"), temp.path()).unwrap_err();

        assert!(error.to_string().contains(NSPROCESS_DLL), "{error}");
    }

    #[test]
    fn the_plugin_include_is_staged_next_to_the_script() {
        let temp = tempfile::TempDir::new().unwrap();
        let cache = cache_with_plugin_files(temp.path());
        let script_dir = temp.path().join("script");
        fs::create_dir_all(&script_dir).unwrap();

        stage_plugin_include(&cache, &script_dir).unwrap();

        assert_eq!(
            fs::read(script_dir.join(NSPROCESS_INCLUDE)).unwrap(),
            b"!define"
        );
    }

    /// Builds a real installer: toolchain resolution, template provisioning,
    /// script rendering, `makensis` and the artifact checksum.
    ///
    /// Ignored by default because it needs a local NSIS toolchain; see
    /// [`require_nsis_toolchain`] and `docs/WINDOWS.md`.
    #[test]
    #[ignore = "requires an NSIS toolchain; set PACOR_NSIS_TOOLCHAIN_DIR"]
    fn an_installer_is_built_end_to_end() {
        use pacor_core::core::arch::Arch;

        // The bundled copy is copied into a scratch directory so the toolchain
        // passed in stays untouched, and so `.extracted` marks the NSIS
        // distribution as present without downloading it.
        let toolchain = require_nsis_toolchain();
        let temp = tempfile::TempDir::new().unwrap();
        let cache = temp.path().join("nsis-cache");
        fs::create_dir_all(&cache).unwrap();
        pacor_core::utils::fs::copy_dir(&toolchain, &cache).unwrap();
        fs::write(cache.join(".extracted"), "").unwrap();

        let project_root = temp.path().join("project");
        let build_dir = temp.path().join("project/build/windows/x64/runner/Release");
        fs::create_dir_all(build_dir.join("data")).unwrap();
        fs::write(build_dir.join("my_app.exe"), b"stub executable").unwrap();
        fs::write(build_dir.join("data/app.so"), b"stub library").unwrap();

        let icon_path = temp.path().join("app_icon.ico");
        write_minimal_icon(&icon_path);

        let output_dir = temp.path().join("dist");
        fs::create_dir_all(&output_dir).unwrap();

        let config = app_config(&[Locale::EnUs, Locale::ZhCn]);
        let builder = NsisBuilder::new(
            &config,
            &project_root,
            demo_project(),
            &cache,
            "my_app.exe",
            &icon_path,
            "Copyright (C) 2025 Example",
        );

        let artifact = builder
            .build(
                "my_app",
                "1.0.0+1",
                &Arch::X64,
                &build_dir,
                &output_dir,
                &CommandOutputMode::None,
            )
            .expect("the installer build must succeed");

        assert_eq!(
            artifact.file(),
            &output_dir.join("my_app-1.0.0+1-x64-setup.exe")
        );
        assert!(artifact.file().is_file());
        assert!(
            fs::metadata(artifact.file()).unwrap().len() > 0,
            "the installer is empty"
        );
        assert!(
            artifact.sha1().is_file(),
            "{} is missing",
            artifact.sha1().display()
        );

        // The plugin has to be in place for `makensis` to have found it.
        assert!(
            cache.join(NSIS_PLUGIN_DIR).join(NSPROCESS_DLL).is_file(),
            "the plugin DLL was not installed into the NSIS directory"
        );
    }
}
