// Copyright 2016-2019 Cargo-Bundle developers <https://github.com/burtonageo/cargo-bundle>
// Copyright 2019-2024 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! Assembly of the `data.tar.gz` payload: the filesystem layout that will be
//! installed on the target system.
//!
//! ```text
//! usr/bin/<app>                          # executable
//! usr/lib/<app>/                         # bundled shared libraries
//! usr/share/<app>/                       # application data / assets
//! usr/share/applications/<app>.desktop   # desktop entry
//! usr/share/icons/hicolor/<size>/apps/   # icons
//! usr/share/doc/<app>/copyright          # copyright
//! ```

use std::collections::BTreeMap;
use std::path::Path;
use std::{fs, path::PathBuf};

use anyhow::Result;

use pacor_core::config::AppConfig;
use pacor_core::core::freedesktop::FreeDesktop;
use pacor_core::utils::fs::{copy_dir, ensure_directory_exists, package_relative_path};

use crate::icon::{Icon, install_icons_to_hicolor_dir};

/// Subdirectory of the package's `usr/` that receives the Flutter bundle's `lib/`.
///
/// Shared with the build templates, which bake the same path into the runner.
pub(super) const USR_LIB_SUBDIR: &str = "lib";

/// Subdirectory of the package's `usr/` that receives the Flutter bundle's `data/`.
pub(super) const USR_DATA_SUBDIR: &str = "share";

/// Inputs required to lay out the package payload.
pub(super) struct DataContext<'a> {
    pub(super) app_config: &'a AppConfig,
    /// Name of the executable and of the bundle directories inside the package.
    ///
    /// This is the Flutter project name, which the build templates also use as
    /// `BINARY_NAME`; keeping them identical is what makes the runner find its
    /// own `libapp.so`, assets and ICU data.
    pub(super) executable_name: &'a str,
    /// Debian package name; `dpkg` forbids underscores, so this is the
    /// hyphenated form. Only used where a package name is really required.
    pub(super) package_name: &'a str,
    /// Directory produced by `flutter build linux` (contains the executable,
    /// `lib/` and `data/`).
    pub(super) flutter_build_path: &'a Path,
    pub(super) desktop: &'a FreeDesktop,
    pub(super) icons: &'a BTreeMap<u32, Icon>,
    /// Extra `source -> target` file mappings from the configuration.
    pub(super) extra_files: &'a std::collections::HashMap<PathBuf, PathBuf>,
}

/// Assemble the payload tree and return its root.
pub(super) fn assemble_data_root(
    package_staging_dir: &Path,
    context: &DataContext<'_>,
) -> Result<PathBuf> {
    let data_root = package_staging_dir.join("data");
    debug!(directory = %data_root.display(), "Assembling package filesystem");

    install_executable(&data_root, context)?;
    install_shared_libraries(&data_root, context)?;
    install_application_data(&data_root, context)?;
    install_desktop_entry(&data_root, context)?;
    install_icons(&data_root, context)?;
    install_extra_files(&data_root, context)?;
    install_copyright(&data_root, context)?;

    debug!("Package filesystem assembly completed");
    Ok(data_root)
}

/// Copy the application entry point into `usr/bin`.
fn install_executable(data_root: &Path, context: &DataContext<'_>) -> Result<()> {
    let target_dir = data_root.join("usr/bin");
    ensure_directory_exists(&target_dir)?;

    let source = context.flutter_build_path.join(context.executable_name);
    let target = target_dir.join(context.executable_name);
    if !source.exists() {
        return Err(anyhow::anyhow!(
            "application executable not found: {} (did the Flutter build run?)",
            source.display()
        ));
    }

    fs::copy(&source, &target)?;
    debug!(source = %source.display(), target = %target.display(), "Executable installed");
    Ok(())
}

/// Copy bundled shared libraries into `usr/lib/<package>`.
fn install_shared_libraries(data_root: &Path, context: &DataContext<'_>) -> Result<()> {
    let source = context.flutter_build_path.join("lib");
    if !source.exists() {
        debug!(path = %source.display(), "No bundled libraries to install");
        return Ok(());
    }

    let target = data_root
        .join("usr")
        .join(USR_LIB_SUBDIR)
        .join(context.executable_name);
    ensure_directory_exists(&target)?;
    copy_dir(&source, &target)?;

    debug!(source = %source.display(), target = %target.display(), "Shared libraries installed");
    Ok(())
}

/// Copy Flutter application assets into `usr/share/<package>`.
fn install_application_data(data_root: &Path, context: &DataContext<'_>) -> Result<()> {
    let source = context.flutter_build_path.join("data");
    if !source.exists() {
        debug!(path = %source.display(), "No application data to install");
        return Ok(());
    }

    let target = data_root
        .join("usr")
        .join(USR_DATA_SUBDIR)
        .join(context.executable_name);
    ensure_directory_exists(&target)?;
    copy_dir(&source, &target)?;

    debug!(source = %source.display(), target = %target.display(), "Application data installed");
    Ok(())
}

/// Write the desktop entry into `usr/share/applications`.
fn install_desktop_entry(data_root: &Path, context: &DataContext<'_>) -> Result<()> {
    let target_dir = data_root.join("usr/share/applications");
    ensure_directory_exists(&target_dir)?;

    let target = target_dir.join(format!("{}.desktop", context.package_name));
    context.desktop.write(&target)?;

    debug!(path = %target.display(), "Desktop entry installed");
    Ok(())
}

/// Install icons following the XDG hicolor theme layout.
fn install_icons(data_root: &Path, context: &DataContext<'_>) -> Result<()> {
    let hicolor_dir = data_root.join("usr/share/icons/hicolor");
    ensure_directory_exists(&hicolor_dir)?;

    // The desktop entry names the icon after the executable, so the files have to
    // carry that same name: `package_name` is hyphenated for `dpkg`, and an icon it
    // points at does not exist.
    install_icons_to_hicolor_dir(context.executable_name, &hicolor_dir, context.icons.values())?;

    debug!(directory = %hicolor_dir.display(), "Icons installed");
    Ok(())
}

/// Copy the `files` mappings declared in `[platforms.linux.deb]`.
///
/// Absolute targets are interpreted as paths inside the package root. The
/// configuration is validated when it is loaded, but the mapping can also arrive
/// from code, so the escape is refused here as well rather than trusted.
fn install_extra_files(data_root: &Path, context: &DataContext<'_>) -> Result<()> {
    for (source, target) in context.extra_files {
        if !source.exists() {
            return Err(anyhow::anyhow!(
                "configured extra file does not exist: {}",
                source.display()
            ));
        }

        let relative = package_relative_path(target).map_err(|problem| {
            anyhow::anyhow!(
                "configured extra file target `{}` {problem}",
                target.display()
            )
        })?;
        let target = data_root.join(relative);
        if let Some(parent) = target.parent() {
            ensure_directory_exists(parent)?;
        }

        if source.is_dir() {
            copy_dir(source, &target)?;
        } else {
            fs::copy(source, &target)?;
        }

        debug!(source = %source.display(), target = %target.display(), "Extra file installed");
    }

    Ok(())
}

/// Write `usr/share/doc/<package>/copyright`.
fn install_copyright(data_root: &Path, context: &DataContext<'_>) -> Result<()> {
    let doc_dir = data_root.join("usr/share/doc").join(context.package_name);
    ensure_directory_exists(&doc_dir)?;

    let path = doc_dir.join("copyright");
    let content = context
        .app_config
        .copyright()
        .unwrap_or("All rights reserved.");

    fs::write(&path, content)?;
    debug!(path = %path.display(), "Copyright file installed");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use pacor_core::config::AppMetadata;
    use pacor_core::i18n::Locale;
    use std::collections::{HashMap, HashSet};
    use tempfile::TempDir;

    fn app_config() -> AppConfig {
        AppConfig {
            default_locale: Locale::EnUs,
            i18n: HashMap::from([(
                Locale::EnUs,
                AppMetadata::new("Demo", "Demo", "desc", "Publisher", "MIT"),
            )]),
            authors: Some(HashSet::new()),
            url: "https://example.com".to_string(),
        }
    }

    fn flutter_build(dir: &Path, name: &str) {
        fs::create_dir_all(dir.join("lib")).unwrap();
        fs::create_dir_all(dir.join("data")).unwrap();
        fs::write(dir.join(name), "#!/bin/sh\n").unwrap();
        fs::write(dir.join("lib/libapp.so"), "lib").unwrap();
        fs::write(dir.join("data/asset.bin"), "asset").unwrap();
    }

    #[test]
    fn assembles_expected_layout() {
        let temp = TempDir::new().unwrap();
        let build_dir = temp.path().join("build");
        flutter_build(&build_dir, "demo");

        let app_config = app_config();
        let desktop = FreeDesktop::default();
        let icons = BTreeMap::new();
        let extra = HashMap::new();
        let context = DataContext {
            app_config: &app_config,
            executable_name: "demo",
            package_name: "demo",
            flutter_build_path: &build_dir,
            desktop: &desktop,
            icons: &icons,
            extra_files: &extra,
        };

        let staging = temp.path().join("staging");
        ensure_directory_exists(&staging).unwrap();
        let data_root = assemble_data_root(&staging, &context).unwrap();

        assert!(data_root.join("usr/bin/demo").exists());
        assert!(data_root.join("usr/lib/demo/libapp.so").exists());
        assert!(data_root.join("usr/share/demo/asset.bin").exists());
        assert!(
            data_root
                .join("usr/share/applications/demo.desktop")
                .exists()
        );
        assert!(data_root.join("usr/share/icons/hicolor").is_dir());
        assert!(data_root.join("usr/share/doc/demo/copyright").exists());
    }

    #[test]
    fn underscores_stay_in_the_bundle_directories_but_not_in_the_package_name() {
        // Flutter project names may contain underscores, and the build templates
        // keep them for `BINARY_NAME`. The bundle paths must follow suit, or the
        // runner cannot find its own library and assets; only `Package:` and the
        // documentation directory are hyphenated, which is all dpkg requires.
        let temp = TempDir::new().unwrap();
        let build_dir = temp.path().join("build");
        flutter_build(&build_dir, "my_app");

        let app_config = app_config();
        let desktop = FreeDesktop::default();
        let icons = BTreeMap::new();
        let extra = HashMap::new();
        let context = DataContext {
            app_config: &app_config,
            executable_name: "my_app",
            package_name: "my-app",
            flutter_build_path: &build_dir,
            desktop: &desktop,
            icons: &icons,
            extra_files: &extra,
        };

        let staging = temp.path().join("staging");
        ensure_directory_exists(&staging).unwrap();
        let data_root = assemble_data_root(&staging, &context).unwrap();

        // The executable the Flutter build produced must be found and installed
        // under the same name the runner uses to locate its resources.
        assert!(data_root.join("usr/bin/my_app").exists());
        assert!(data_root.join("usr/lib/my_app/libapp.so").exists());
        assert!(data_root.join("usr/share/my_app/asset.bin").exists());

        // dpkg forbids underscores in the package name.
        assert!(data_root.join("usr/share/doc/my-app/copyright").exists());
    }

    #[test]
    fn the_usr_subdirectories_match_the_build_template_placeholders() {
        // `deb/mod.rs` derives APP_LIB_DIR/APP_DATA_DIR from these constants, so
        // this pins the layout the runner is told to look in.
        let context = |data_root: &Path| data_root.join("usr").join(USR_LIB_SUBDIR);
        assert_eq!(USR_LIB_SUBDIR, "lib");
        assert_eq!(USR_DATA_SUBDIR, "share");
        assert!(context(Path::new("/pkg")).ends_with("usr/lib"));
    }

    #[test]
    fn missing_executable_is_reported() {
        let temp = TempDir::new().unwrap();
        let build_dir = temp.path().join("build");
        fs::create_dir_all(&build_dir).unwrap();

        let app_config = app_config();
        let desktop = FreeDesktop::default();
        let icons = BTreeMap::new();
        let extra = HashMap::new();
        let context = DataContext {
            app_config: &app_config,
            executable_name: "demo",
            package_name: "demo",
            flutter_build_path: &build_dir,
            desktop: &desktop,
            icons: &icons,
            extra_files: &extra,
        };

        let staging = temp.path().join("staging");
        ensure_directory_exists(&staging).unwrap();
        let error = assemble_data_root(&staging, &context).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("application executable not found")
        );
    }

    /// The payload is assembled in a staging directory, so a target with `..` would
    /// write next to that directory — on the packager's machine, not into the package.
    #[test]
    fn a_files_target_may_not_escape_the_package_root() {
        let temp = TempDir::new().unwrap();
        let build_dir = temp.path().join("build");
        flutter_build(&build_dir, "demo");

        let source = temp.path().join("LICENSE");
        fs::write(&source, "MIT").unwrap();

        let app_config = app_config();
        let desktop = FreeDesktop::default();
        let icons = BTreeMap::new();
        let extra = HashMap::from([(source, PathBuf::from("../../escaped.txt"))]);
        let context = DataContext {
            app_config: &app_config,
            executable_name: "demo",
            package_name: "demo",
            flutter_build_path: &build_dir,
            desktop: &desktop,
            icons: &icons,
            extra_files: &extra,
        };

        let staging = temp.path().join("staging");
        ensure_directory_exists(&staging).unwrap();
        let error = assemble_data_root(&staging, &context).unwrap_err();

        assert!(error.to_string().contains("package root"), "{error}");
        assert!(
            !temp.path().join("escaped.txt").exists(),
            "the file must not be written outside the package"
        );
    }
    #[test]
    fn extra_files_with_absolute_targets_land_in_package_root() {
        let temp = TempDir::new().unwrap();
        let build_dir = temp.path().join("build");
        flutter_build(&build_dir, "demo");

        let source = temp.path().join("LICENSE");
        fs::write(&source, "MIT").unwrap();

        let app_config = app_config();
        let desktop = FreeDesktop::default();
        let icons = BTreeMap::new();
        let extra = HashMap::from([(source, PathBuf::from("/usr/share/doc/demo/LICENSE"))]);
        let context = DataContext {
            app_config: &app_config,
            executable_name: "demo",
            package_name: "demo",
            flutter_build_path: &build_dir,
            desktop: &desktop,
            icons: &icons,
            extra_files: &extra,
        };

        let staging = temp.path().join("staging");
        ensure_directory_exists(&staging).unwrap();
        let data_root = assemble_data_root(&staging, &context).unwrap();

        assert!(data_root.join("usr/share/doc/demo/LICENSE").exists());
    }
}