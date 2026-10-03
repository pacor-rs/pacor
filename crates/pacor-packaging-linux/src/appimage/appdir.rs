//! AppDir staging: the directory tree that linuxdeploy turns into an AppImage.
//!
//! Layout produced here:
//!
//! ```text
//! <name>.AppDir/
//!     AppRun                              # entry point (executable)
//!     <name>.desktop                      # desktop entry
//!     <name>.<ext> -> usr/share/icons/... # icon symlink required by linuxdeploy
//!     usr/bin/<name>                      # application executable
//!     usr/lib/                            # bundled libraries
//!     usr/share/                          # data, icons
//! ```

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use pacor_core::core::freedesktop::FreeDesktop;
/// Subdirectory of the AppDir's `usr/` holding the bundled libraries.
///
/// The whole Flutter bundle is copied into `usr/`, so the layout mirrors the
/// bundle (`lib/`, `data/`) rather than the Debian one.
pub(super) const USR_LIB_SUBDIR: &str = "lib";

/// Subdirectory of the AppDir's `usr/` holding the Flutter assets.
pub(super) const USR_DATA_SUBDIR: &str = "data";

use pacor_core::utils::fs::{
    copy_dir, create_symlink, ensure_directory_exists, write_executable_file,
};

use crate::icon::{Icon, install_icons_to_hicolor_dir};

/// A staged AppDir.
///
/// Owns the temporary directory that backs it, so the staging tree cannot be
/// deleted while it is still in use.
pub(super) struct AppDir {
    _guard: tempfile::TempDir,
    path: PathBuf,
}

impl AppDir {
    /// Create an empty AppDir in a fresh temporary directory.
    pub(super) fn create(app_name: &str) -> Result<Self> {
        let guard = tempfile::TempDir::with_prefix("pacor_appimage_")?;
        let path = guard.path().join(format!("{app_name}.AppDir"));
        ensure_directory_exists(&path)?;

        debug!(app_dir = %path.display(), "AppDir created");
        Ok(Self {
            _guard: guard,
            path,
        })
    }

    /// Root of the AppDir.
    pub(super) fn path(&self) -> &Path {
        &self.path
    }

    /// `<appdir>/usr/bin/<name>`: the executable handed to linuxdeploy.
    pub(super) fn executable_path(&self, app_name: &str) -> PathBuf {
        self.path.join("usr/bin").join(app_name)
    }

    /// `<appdir>/usr/lib`: used as `LD_LIBRARY_PATH` when running linuxdeploy.
    pub(super) fn library_dir(&self) -> PathBuf {
        self.path.join("usr/lib")
    }

    /// Copy the Flutter build output into `usr/` and move the executable to `usr/bin`.
    pub(super) fn populate(&self, flutter_build_path: &Path, app_name: &str) -> Result<()> {
        let usr_dir = self.path.join("usr");
        copy_dir(flutter_build_path, &usr_dir)?;

        let bin_dir = usr_dir.join("bin");
        ensure_directory_exists(&bin_dir)?;

        let executable = usr_dir.join(app_name);
        if !executable.exists() {
            return Err(anyhow::anyhow!(
                "application executable `{app_name}` not found in {} (did the Flutter build run?)",
                flutter_build_path.display()
            ));
        }

        let target = bin_dir.join(app_name);
        if target.exists() {
            fs::remove_file(&target)?;
        }
        fs::rename(&executable, &target)?;

        debug!(executable = app_name, to = %bin_dir.display(), "Executable moved to usr/bin");
        Ok(())
    }

    /// Write the `AppRun` launcher and return its path.
    ///
    /// `AppRun` is the AppImage entry point; it resolves its own directory so
    /// the bundle works regardless of where it is mounted.
    pub(super) fn write_apprun(&self, app_name: &str) -> Result<PathBuf> {
        let content =
            format!("#!/bin/sh\ncd \"$(dirname \"$0\")\"\nexec ./usr/bin/{app_name} \"$@\"\n");

        let path = self.path.join("AppRun");
        write_executable_file(&path, content.as_bytes())?;

        debug!(path = %path.display(), "AppRun script created");
        Ok(path)
    }

    /// Install icons into the AppDir's hicolor theme tree.
    pub(super) fn install_icons(
        &self,
        app_name: &str,
        icons: std::collections::btree_map::Values<'_, u32, Icon>,
    ) -> Result<()> {
        let hicolor_dir = self.path.join("usr/share/icons/hicolor");
        ensure_directory_exists(&hicolor_dir)?;

        install_icons_to_hicolor_dir(app_name, &hicolor_dir, icons)?;

        debug!(directory = %hicolor_dir.display(), "Icons installed");
        Ok(())
    }

    /// Symlink the primary icon to the AppDir root, as linuxdeploy expects.
    pub(super) fn link_primary_icon(&self, app_name: &str, primary: &Icon) -> Result<()> {
        let file_name = format!("{app_name}.{}", primary.format());
        let target = self.path.join(&file_name);
        let source = self
            .path
            .join("usr/share/icons/hicolor")
            .join(format!("{}x{}", primary.size(), primary.size()))
            .join(&file_name);

        create_symlink(&source, &target).with_context(|| {
            format!(
                "failed to link the primary icon {} -> {}",
                source.display(),
                target.display()
            )
        })?;

        debug!(source = %source.display(), target = %target.display(), "Primary icon linked");
        Ok(())
    }

    /// Write the desktop entry into the AppDir root and return its path.
    pub(super) fn write_desktop_file(
        &self,
        app_name: &str,
        desktop: &FreeDesktop,
    ) -> Result<PathBuf> {
        let path = self.path.join(format!("{app_name}.desktop"));
        desktop.write(&path)?;
        Ok(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::icon::IconFormat;
    use std::collections::BTreeMap;
    use tempfile::TempDir;

    fn flutter_build(dir: &Path, name: &str) -> PathBuf {
        let root = dir.join("bundle");
        fs::create_dir_all(root.join("lib")).unwrap();
        fs::create_dir_all(root.join("data")).unwrap();
        fs::write(root.join(name), "#!/bin/sh\n").unwrap();
        fs::write(root.join("lib/libapp.so"), "lib").unwrap();
        root
    }

    #[test]
    fn app_dir_has_the_expected_layout() {
        let temp = TempDir::new().unwrap();
        let build = flutter_build(temp.path(), "demo");

        let app_dir = AppDir::create("demo").unwrap();
        app_dir.populate(&build, "demo").unwrap();
        app_dir.write_apprun("demo").unwrap();

        assert!(app_dir.path().join("AppRun").exists());
        // The executable must have been moved out of usr/ into usr/bin/.
        assert!(app_dir.path().join("usr/bin/demo").exists());
        assert!(!app_dir.path().join("usr/demo").exists());
        // Libraries keep their place for LD_LIBRARY_PATH.
        assert!(app_dir.path().join("usr/lib/libapp.so").exists());
    }

    #[test]
    fn apprun_is_an_executable_sh_script_pointing_at_usr_bin() {
        let app_dir = AppDir::create("demo").unwrap();
        let path = app_dir.write_apprun("demo").unwrap();

        let contents = fs::read_to_string(&path).unwrap();
        assert!(contents.starts_with("#!/bin/sh\n"));
        assert!(contents.contains("exec ./usr/bin/demo \"$@\""));
        assert!(contents.contains("cd \"$(dirname \"$0\")\""));

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o111, 0o111, "AppRun must be executable");
        }
    }

    #[test]
    fn populate_reports_a_missing_executable() {
        let temp = TempDir::new().unwrap();
        let build = temp.path().join("bundle");
        fs::create_dir_all(&build).unwrap();

        let app_dir = AppDir::create("demo").unwrap();
        let error = app_dir.populate(&build, "demo").unwrap_err();

        assert!(error.to_string().contains("not found in"), "{error}");
    }

    #[test]
    fn icons_are_installed_and_the_primary_one_is_linked() {
        let temp = TempDir::new().unwrap();
        let source = temp.path().join("32x32.png");
        fs::write(&source, "png").unwrap();

        let primary = Icon::new(32, source, IconFormat::Png);
        let icons = BTreeMap::from([(
            32,
            Icon::new(32, temp.path().join("32x32.png"), IconFormat::Png),
        )]);

        let app_dir = AppDir::create("demo").unwrap();
        app_dir.install_icons("demo", icons.values()).unwrap();
        app_dir.link_primary_icon("demo", &primary).unwrap();

        assert!(
            app_dir
                .path()
                .join("usr/share/icons/hicolor/32x32/demo.png")
                .exists()
        );
        assert!(app_dir.path().join("demo.png").exists());
    }

    #[test]
    fn desktop_file_is_written_into_the_app_dir_root() {
        let mut desktop = FreeDesktop::default();
        desktop.set_name("Demo");
        desktop.set_icon("demo");
        desktop.set_exec("AppRun");

        let app_dir = AppDir::create("demo").unwrap();
        let path = app_dir.write_desktop_file("demo", &desktop).unwrap();

        assert_eq!(path, app_dir.path().join("demo.desktop"));
        let contents = fs::read_to_string(&path).unwrap();
        assert!(contents.contains("[Desktop Entry]"));
        assert!(contents.contains("Name=Demo"));
        assert!(contents.contains("Exec=AppRun"));
    }
}
