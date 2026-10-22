//! AppImage bundle creation.
//!
//! An AppImage is a self-contained executable bundling the application and its
//! dependencies. The build has four stages, each in its own module:
//!
//! - [`tool`]: fetch/patch the `linuxdeploy` tool and run it
//! - [`appdir`]: lay out the `AppDir` staging tree linuxdeploy consumes
//! - [`output`]: normalise linuxdeploy's naming and permissions
//! - this module: orchestration and the project template swap

mod appdir;
mod output;
mod tool;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::Result;

use pacor_core::core::arch::Arch;
use pacor_core::core::freedesktop::FreeDesktop;
use pacor_core::utils::command::CommandOutputMode;

use crate::icon::Icon;
use crate::project_templates::{ProjectTemplate, TemplateApplier, TemplateSwap, TemplateVariables};

/// Values baked into the AppImage build template.
///
/// The AppDir copies the Flutter bundle as-is into `usr/`, so the paths are the
/// bundle's own (`lib`, `data`) rather than the Debian ones.
fn template_variables(executable_name: &str) -> TemplateVariables {
    TemplateVariables {
        executable_name: executable_name.to_string(),
        lib_dir: appdir::USR_LIB_SUBDIR.to_string(),
        data_dir: appdir::USR_DATA_SUBDIR.to_string(),
        // The AppImage does not ship the DEB CMake template, but keep the field
        // meaningful rather than inventing a second variable type.
        application_id: format!("com.example.{executable_name}"),
    }
}

/// Everything required to build an AppImage.
pub struct AppImageBuildConfig<'a> {
    pub app_display_name: &'a str,
    pub app_executable_name: &'a str,
    pub primary_icon: &'a Icon,
    pub additional_icons: &'a BTreeMap<u32, Icon>,
    pub flutter_build_path: &'a Path,
    pub project_root: &'a Path,
    pub tools_directory: &'a Path,
    pub output_directory: &'a Path,
    pub desktop: &'a FreeDesktop,
    pub target_architecture: &'a Arch,
    pub artifact_name: &'a str,
}

/// Builds AppImages from a Flutter Linux build.
pub struct AppImageBuilder<'a> {
    config: AppImageBuildConfig<'a>,
}

impl<'a> AppImageBuilder<'a> {
    /// Create a builder from the provided configuration.
    pub fn from_config(config: AppImageBuildConfig<'a>) -> Self {
        debug!(
            app = config.app_executable_name,
            "Initializing AppImage builder"
        );
        Self { config }
    }

    /// Replace the project's Linux templates with rendered upstream versions.
    ///
    /// The returned guard keeps the project files replaced until it is dropped;
    /// it restores them even if the build fails.
    pub fn apply_templates(&self) -> Result<TemplateSwap> {
        info!("Applying AppImage build templates");

        let applier = TemplateApplier::new(
            self.config.tools_directory,
            "appimage",
            template_variables(self.config.app_executable_name),
        )?;

        TemplateSwap::apply(
            applier,
            &[ProjectTemplate {
                remote_filename: "my_application_template.cc".to_string(),
                local_file_path: self
                    .config
                    .project_root
                    .join("linux/runner/my_application.cc"),
            }],
        )
    }

    /// Build the AppImage and return the path to the final artifact.
    pub fn build_appimage(&self, command_output_mode: &CommandOutputMode) -> Result<PathBuf> {
        let expected_path = self.config.output_directory.join(self.config.artifact_name);
        info!(
            artifact = self.config.artifact_name,
            output = %expected_path.display(),
            "Starting AppImage build"
        );

        let linuxdeploy = tool::prepare_linuxdeploy_tool(
            self.config.tools_directory,
            self.config.target_architecture,
        )?;

        let app_dir = appdir::AppDir::create(self.config.app_executable_name)?;
        app_dir.populate(
            self.config.flutter_build_path,
            self.config.app_executable_name,
        )?;
        app_dir.write_apprun(self.config.app_executable_name)?;
        app_dir.install_icons(
            self.config.app_executable_name,
            self.config.additional_icons.values(),
        )?;
        app_dir.link_primary_icon(self.config.app_executable_name, self.config.primary_icon)?;

        let desktop_file =
            app_dir.write_desktop_file(self.config.app_executable_name, self.config.desktop)?;

        // Bounds what counts as this build's output: see `finalize_appimage`.
        let build_started_at = std::time::SystemTime::now();

        tool::run_linuxdeploy(
            &linuxdeploy,
            &app_dir,
            &app_dir.executable_path(self.config.app_executable_name),
            &desktop_file,
            self.config.output_directory,
            &expected_path,
            command_output_mode,
        )?;

        let final_path = output::finalize_appimage(
            self.config.output_directory,
            &expected_path,
            &[
                self.config.app_display_name,
                self.config.app_executable_name,
            ],
            build_started_at,
        )?;

        info!(path = %final_path.display(), "AppImage build completed");
        Ok(final_path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn template_variables_follow_the_appdir_layout() {
        let variables = template_variables("my_app");

        assert_eq!(variables.executable_name, "my_app");
        // The AppDir copies the Flutter bundle as-is, so the directories are the
        // bundle's own and carry no application name.
        assert_eq!(variables.lib_dir, appdir::USR_LIB_SUBDIR);
        assert_eq!(variables.data_dir, appdir::USR_DATA_SUBDIR);
    }
}
