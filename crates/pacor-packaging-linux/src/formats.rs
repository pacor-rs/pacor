//! Adapters from the shared Linux build inputs to the individual package
//! formats, plus the dispatch over `platforms.linux.formats`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Result, bail};

use pacor_core::config::{AppConfig, LinuxConfig, LinuxPackageFormat};
use pacor_core::core::arch::Arch;
use pacor_core::core::project::FlutterProject;
use pacor_core::platforms::BuildArtifact;
use pacor_core::utils::command::CommandOutputMode;

use crate::appimage::{AppImageBuildConfig, AppImageBuilder};
use crate::deb::{DebianPackageBuildConfig, DebianPackageBuilder};
use crate::icon::Icon;
use crate::{desktop, flutter, icons};

/// Resolved inputs shared by every Linux package format.
///
/// Borrows from [`crate::LinuxBuilder`], which owns the configuration; this
/// keeps the per-format adapters free of builder state.
pub(crate) struct LinuxBuildContext<'a> {
    pub app_config: &'a AppConfig,
    pub linux_config: &'a LinuxConfig,
    pub flutter_project: &'a FlutterProject,
    pub project_root: &'a Path,
    pub flutter_build_path: &'a Path,
    pub output_dir: &'a Path,
    pub tools_dir: &'a Path,
    pub arch: &'a Arch,
}

/// Build every configured package format.
pub(crate) fn build_installers(
    context: &LinuxBuildContext<'_>,
    command_output_mode: &CommandOutputMode,
) -> Result<Vec<BuildArtifact>> {
    let formats = sorted_formats(context.linux_config);
    info!(formats = ?formats, "Building Linux installers");
    if formats.is_empty() {
        bail!("`platforms.linux.formats` is empty; configure at least one of: appimage, deb");
    }

    let icons = icons::search_icons(
        context.linux_config.icon_directory(),
        context.tools_dir,
        context.project_root,
    )?;
    let primary_icon = icons::resolve_primary_icon(&icons)?;
    debug!(icon = %primary_icon.path().display(), "Primary icon selected");

    let executable_name = context.flutter_project.name();
    let desktop_entry = desktop::create_desktop_entry(
        context.app_config,
        executable_name,
        context.linux_config.desktop().unwrap_or_default(),
    );

    let mut artifacts = Vec::with_capacity(formats.len());
    for format in formats {
        let output_file = match format {
            LinuxPackageFormat::AppImage => build_appimage(
                context,
                primary_icon,
                &icons,
                desktop_entry.clone(),
                command_output_mode,
            )?,
            LinuxPackageFormat::Deb => {
                build_deb(context, &icons, desktop_entry.clone(), command_output_mode)?
            }
            LinuxPackageFormat::Rpm => bail!(
                "RPM packaging is not implemented yet; `platforms.linux.formats` supports: appimage, deb"
            ),
        };

        artifacts.push(pacor_core::packaging::create_artifact_with_checksum(
            output_file,
        )?);
    }

    Ok(artifacts)
}

/// Text used for the Debian `Description` field.
///
/// Prefers the application's default-locale description — the same text that
/// becomes the desktop entry `Comment` — so `dpkg`/`apt` and the application
/// menu agree. Falls back to the `pubspec.yaml` description when the metadata
/// omits one.
fn package_description<'a>(
    app_config: &'a AppConfig,
    flutter_project: &'a FlutterProject,
) -> &'a str {
    app_config
        .description()
        .unwrap_or_else(|| flutter_project.description())
}

/// Configured formats in a stable order.
///
/// `formats` is a `HashSet`, so without this the artifact order (and therefore
/// the build log) would vary between runs.
fn sorted_formats(linux_config: &LinuxConfig) -> Vec<LinuxPackageFormat> {
    let mut formats: Vec<LinuxPackageFormat> = linux_config.formats().iter().cloned().collect();
    formats.sort_by_key(|format| format.to_string());
    formats
}

/// Build an AppImage.
fn build_appimage(
    context: &LinuxBuildContext<'_>,
    primary_icon: &Icon,
    icons: &BTreeMap<u32, Icon>,
    mut desktop_entry: pacor_core::core::freedesktop::FreeDesktop,
    command_output_mode: &CommandOutputMode,
) -> Result<PathBuf> {
    info!("Starting AppImage build");

    // The AppImage is launched through AppRun, not the executable directly.
    desktop_entry.set_exec("AppRun");

    let executable_name = context.flutter_project.name();
    let appimage_artifact_name = pacor_core::packaging::artifact_name(
        executable_name,
        context.flutter_project.version(),
        context.arch,
        "",
        "AppImage",
    );

    let builder = AppImageBuilder::from_config(AppImageBuildConfig {
        project_root: context.project_root,
        app_display_name: context
            .app_config
            .name()
            .unwrap_or_else(|| context.flutter_project.name()),
        app_executable_name: executable_name,
        primary_icon,
        additional_icons: icons,
        flutter_build_path: context.flutter_build_path,
        tools_directory: context.tools_dir,
        output_directory: context.output_dir,
        desktop: &desktop_entry,
        target_architecture: context.arch,
        artifact_name: &appimage_artifact_name,
    });

    build_flutter_with_templates(context, builder.apply_templates()?, command_output_mode)?;

    if !context.flutter_build_path.exists() {
        bail!(
            "Flutter build directory does not exist: {}",
            context.flutter_build_path.display()
        );
    }

    builder.build_appimage(command_output_mode)
}

/// Build a Debian package.
fn build_deb(
    context: &LinuxBuildContext<'_>,
    icons: &BTreeMap<u32, Icon>,
    desktop_entry: pacor_core::core::freedesktop::FreeDesktop,
    command_output_mode: &CommandOutputMode,
) -> Result<PathBuf> {
    info!("Starting Debian package build");

    let executable_name = context.flutter_project.name();
    let deb_artifact_name = pacor_core::packaging::artifact_name(
        executable_name,
        context.flutter_project.version(),
        context.arch,
        "",
        "deb",
    );

    let builder = DebianPackageBuilder::from_config(DebianPackageBuildConfig {
        app_config: context.app_config,
        app_executable_name: executable_name,
        additional_icons: icons,
        flutter_build_path: context.flutter_build_path,
        project_root: context.project_root,
        tools_directory: context.tools_dir,
        output_directory: context.output_dir,
        desktop: &desktop_entry,
        target_architecture: context.arch,
        package_version: context.flutter_project.version(),
        package_description: package_description(context.app_config, context.flutter_project),
        debian_package_config: context.linux_config.deb.clone().unwrap_or_default(),
        artifact_name: &deb_artifact_name,
        maintainer: context.linux_config.maintainer(),
    });

    build_flutter_with_templates(context, builder.apply_templates()?, command_output_mode)?;

    builder.build_package()
}

/// Run the Flutter build while `swap` holds the patched project templates.
///
/// The swap is restored explicitly so failures are reported; if the Flutter
/// build returns early with `?`, the guard's `Drop` still restores the files.
fn build_flutter_with_templates(
    context: &LinuxBuildContext<'_>,
    swap: crate::project_templates::TemplateSwap,
    command_output_mode: &CommandOutputMode,
) -> Result<()> {
    flutter::build(context.project_root, command_output_mode)?;
    swap.restore()
}

#[cfg(test)]
mod tests {
    use super::*;
    use pacor_core::config::{AppConfig, AppMetadata};
    use pacor_core::core::project::FlutterProject;
    use pacor_core::i18n::Locale;
    use std::collections::{HashMap, HashSet};
    use tempfile::TempDir;

    fn app_config_with(description: Option<&str>) -> AppConfig {
        AppConfig {
            default_locale: Locale::EnUs,
            i18n: HashMap::from([(
                Locale::EnUs,
                AppMetadata::new(
                    Some("Demo"),
                    Some("Demo"),
                    description,
                    "Publisher",
                    Some("Copyright"),
                ),
            )]),
            authors: Some(HashSet::new()),
            url: "https://example.com".to_string(),
        }
    }

    /// A Flutter project whose `pubspec.yaml` carries its own description.
    fn flutter_project(temp: &TempDir) -> FlutterProject {
        std::fs::write(
            temp.path().join("pubspec.yaml"),
            "name: demo\nversion: 1.0.0\ndescription: pubspec description\n",
        )
        .unwrap();
        FlutterProject::load_from_pubspec(temp.path()).unwrap()
    }

    #[test]
    fn description_prefers_the_localized_metadata() {
        let temp = TempDir::new().unwrap();
        let project = flutter_project(&temp);
        let config = app_config_with(Some("App description"));

        assert_eq!(package_description(&config, &project), "App description");
    }

    #[test]
    fn description_falls_back_to_pubspec() {
        let temp = TempDir::new().unwrap();
        let project = flutter_project(&temp);
        let config = app_config_with(None);

        assert_eq!(
            package_description(&config, &project),
            "pubspec description"
        );
    }
}
