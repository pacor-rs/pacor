use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use crate::{
    AppConfig, BundleBuilder, BundleConfig, BundleError, Result,
    cli::args::CommandOutputMode,
    core::{
        arch::Arch,
        common::{
            ArtifactManager, BuildEnvironmentValidator, CommonBuilderImpl, CommonBuilderOperations,
        },
        config::LinuxConfig,
        freedesktop::FreeDesktop,
        project::{FlutterProject, find_flutter_project_root},
    },
    platforms::{
        BuildArtifact, Platform,
        linux::{
            appimage::{AppImageBuildConfig, AppImageBuilder},
            deb::{DebianPackageBuildConfig, DebianPackageBuilder},
            icon::{Icon, IconFormat},
        },
    },
    utils::{
        archive::extract_zip, command::execute_flutter_build_command,
        crypto::generate_sha1_checksum_file, downloader::download_file,
        fs::ensure_directory_exists,
    },
};

mod appimage;
mod deb;
mod icon;

pub(crate) struct LinuxBuilder {
    /// Common builder functionality
    common: CommonBuilderImpl,
    app_config: AppConfig,
    linux_config: LinuxConfig,

    flutter_project: FlutterProject,

    project_root: PathBuf,
    flutter_build_path: PathBuf,
    output_dir: PathBuf,

    tools_dir: PathBuf,

    arch: Arch,
}

impl BundleBuilder for LinuxBuilder {
    type Config = BundleConfig;

    type Output = Vec<BuildArtifact>;

    fn new(config: Self::Config) -> crate::Result<Self>
    where
        Self: Sized,
    {
        let linux_config = config.platforms.linux.clone();

        if !linux_config.enabled() {
            return Err(BundleError::Platform(
                "Linux platform is not enabled".to_string(),
            ));
        }

        let project_root = find_flutter_project_root()?;
        let flutter_project = FlutterProject::load_from_pubspec(&project_root)?;
        let flutter_build_path =
            flutter_project.get_flutter_build_path(&project_root, Platform::Linux);

        let arch = Arch::current()?;
        let output_dir = config.output_dir()?;

        let tools_dir = dirs::config_dir()
            .ok_or(BundleError::Platform(
                "Failed to get user config directory".to_string(),
            ))?
            .join("pacor")
            .join("linux");

        info!(
            project = %flutter_project.name(),
            version = %flutter_project.version(),
            arch = %arch,
            output_dir = %output_dir.display(),
            tools_dir = %tools_dir.display(),
            "Initializing Linux builder"
        );

        Ok(Self {
            common: CommonBuilderImpl::default(),
            app_config: config.app,
            linux_config,

            project_root,
            flutter_project,

            flutter_build_path,
            output_dir,
            tools_dir,

            arch,
        })
    }

    fn build(
        &self,
        skip_flutter_build: bool,
        command_output_mode: CommandOutputMode,
    ) -> crate::Result<Self::Output> {
        info!("Starting to build Linux installer...");

        // Ensure output directory exists and validate permissions
        self.validate_directory_permissions(&self.output_dir, "Linux build output")?;
        ensure_directory_exists(&self.output_dir)?;

        if skip_flutter_build {
            warn!("The `--skip-flutter-build` flag is not supported for Linux platform");
        }

        let outputs = self.build_installer(&command_output_mode)?;

        info!("Linux installer built successfully");
        Ok(outputs)
    }

    fn clean(&self) -> crate::Result<()> {
        Ok(())
    }

    fn validate(&self) -> crate::Result<()> {
        // Use common validation functionality
        let required_tools = ["flutter"];
        let required_files = [
            self.project_root.join("pubspec.yaml").as_path(),
            self.project_root.join("linux").as_path(),
        ];

        self.validate_build_environment(
            "Linux",
            &required_tools,
            &required_files,
            &self.output_dir,
        )?;

        Ok(())
    }

    fn project_root(&self) -> &std::path::Path {
        &self.project_root
    }

    fn flutter_build_path(&self) -> &std::path::Path {
        &self.flutter_build_path
    }
}

impl LinuxBuilder {
    fn build_flutter_app(&self, command_output_mode: &CommandOutputMode) -> Result<()> {
        info!("Building Linux app in release mode...");

        let args = vec![
            "linux",
            "--release",
            "--obfuscate",
            "--split-debug-info=tmp/",
        ];

        execute_flutter_build_command(&args, &self.project_root, command_output_mode)?;

        info!(
            build_path = %self.flutter_build_path().display(),
            "Linux app built successfully"
        );
        Ok(())
    }

    fn build_freedesktop(
        &self,
        app_display_name: &str,
        app_executable_name: &str,
    ) -> Result<FreeDesktop> {
        let mut desktop = self.linux_config.desktop().unwrap_or_default();
        desktop.set_name(app_display_name);
        desktop.set_icon(app_executable_name);
        desktop.set_exec(self.flutter_project.name());

        Ok(desktop)
    }

    fn build_installer(
        &self,
        command_output_mode: &CommandOutputMode,
    ) -> Result<Vec<BuildArtifact>> {
        let formats = self.linux_config.package_formats();
        info!(formats = ?formats, "Building Linux installer...");

        let app_display_name = self
            .app_config
            .name()
            .unwrap_or(self.flutter_project.name());
        let app_executable_name = self.flutter_project.name();

        let icons = self.search_icons()?;
        debug!(icons = ?icons, "Icons found");

        // 图标选择：优先 32x32，否则选最大可用
        let icon = icons
            .get(&32)
            .or_else(|| {
                icons
                    .iter()
                    .max_by_key(|(size, _icon)| *size)
                    .map(|(_, icon)| icon)
            })
            .ok_or_else(||
                BundleError::Build(
                    "No icon found in configured directory. Please ensure `platforms.linux.icon_directory` is valid and contains PNG or SVG icons in standard sizes (16x16, 24x24, 32x32, 48x48, 64x64, 128x128, 256x256, 512x512, 1024x1024)".to_string(),
                )
            )?;
        debug!(icon = %icon.path().display(), "Icon selected");

        let desktop = self.build_freedesktop(app_display_name, app_executable_name)?;

        let mut outputs = Vec::with_capacity(formats.len());

        for format in formats {
            let output_file = match format {
                crate::core::config::LinuxPackageFormat::AppImage => self.build_appimage(
                    app_display_name,
                    icon,
                    &icons,
                    desktop.clone(),
                    command_output_mode,
                )?,
                crate::core::config::LinuxPackageFormat::Deb => self.build_deb(
                    app_executable_name,
                    &icons,
                    desktop.clone(),
                    command_output_mode,
                )?,
                crate::core::config::LinuxPackageFormat::Rpm => todo!(),
            };

            let sha1_file = generate_sha1_checksum_file(&output_file)?;

            // Use common artifact creation functionality
            let artifact = self.create_artifact_with_checksum(output_file)?;
            outputs.push(artifact);
        }

        Ok(outputs)
    }

    fn build_appimage(
        &self,
        app_display_name: &str,
        icon: &Icon,
        icons: &BTreeMap<u32, Icon>,
        mut desktop: FreeDesktop,
        command_output_mode: &CommandOutputMode,
    ) -> Result<PathBuf> {
        info!("Starting build appimage...");

        let app_executable_name = self.flutter_project.name();

        // AppImage 启动时会把自身挂载为一个目录，用户双击 `.AppImage` 会执行根目录的 `AppRun`。`.desktop` 文件也应指向 `AppRun`，而不是内部可执行文件。
        desktop.set_exec("AppRun");

        let config = AppImageBuildConfig {
            project_root: &self.project_root,
            app_display_name,
            app_executable_name,
            primary_icon: icon,
            additional_icons: icons,
            flutter_build_path: self.flutter_build_path(),
            tools_directory: &self.tools_dir,
            output_directory: &self.output_dir,
            desktop: &desktop,
            target_architecture: &self.arch,
            version: self.flutter_project.version(),
        };

        let appimage_builder = AppImageBuilder::from_config(config);

        let replaced_files = appimage_builder.download_and_apply_templates()?;
        self.build_flutter_app(command_output_mode)?;
        appimage_builder.restore_original_files(&replaced_files)?;

        if !self.flutter_build_path().exists() {
            return Err(BundleError::Platform(
                "Flutter build directory does not exist".to_string(),
            ));
        }

        let output_appimage_file = appimage_builder.build_appimage(command_output_mode)?;
        Ok(output_appimage_file)
    }

    fn build_deb(
        &self,
        app_executable_name: &str,
        icons: &BTreeMap<u32, Icon>,
        desktop: FreeDesktop,
        command_output_mode: &CommandOutputMode,
    ) -> Result<PathBuf> {
        let config = DebianPackageBuildConfig {
            app_config: &self.app_config,
            app_executable_name,
            additional_icons: icons,
            flutter_build_path: &self.flutter_build_path,
            project_root: &self.project_root,
            tools_directory: &self.tools_dir,
            output_directory: &self.output_dir,
            desktop: &desktop,
            target_architecture: &self.arch,
            package_version: self.flutter_project.version(),
            package_description: self.flutter_project.description(),
            debian_package_config: self
                .linux_config
                .package_config
                .as_ref()
                .map(|pc| pc.deb.clone().unwrap_or_default())
                .unwrap_or_default(),
        };
        let deb_builder = DebianPackageBuilder::from_config(config);

        let replaced_files = deb_builder.download_and_apply_templates()?;
        self.build_flutter_app(command_output_mode)?;
        deb_builder.restore_original_files(&replaced_files)?;

        let output_deb_file = deb_builder.build_package()?;
        Ok(output_deb_file)
    }

    fn download_default_icons(default_icons_dir: &Path) -> Result<()> {
        let url =
            "https://raw.githubusercontent.com/pacor-rs/tools/refs/heads/main/linux/icons.zip";

        debug!(url, "Downloading default icons");
        let data = download_file(url)?;

        extract_zip(&data, default_icons_dir)?;
        info!("Default icons is downloaded");

        Ok(())
    }

    fn search_icons(&self) -> Result<BTreeMap<u32, Icon>> {
        const ICON_SIZES: [u32; 9] = [16, 24, 32, 48, 64, 128, 256, 512, 1024];

        let icon_dir = if let Some(dir) = self.linux_config.icon_directory() {
            dir.to_path_buf()
        } else {
            let default_icons_dir = self.tools_dir.join("icons");
            if !default_icons_dir.exists() {
                Self::download_default_icons(&default_icons_dir)?;
            }

            default_icons_dir
        };

        let icon_dir = if icon_dir.is_absolute() {
            icon_dir
        } else {
            self.project_root.join(icon_dir)
        };

        let mut icons = BTreeMap::new();

        for &size in ICON_SIZES.iter() {
            // 优先查找svg格式的图片
            let icon_path = icon_dir.join(format!("{size}x{size}.{}", IconFormat::Svg));
            if icon_path.exists() {
                icons.insert(size, Icon::new(size, icon_path, IconFormat::Svg));
                continue; // Found one format, no need to check others
            }

            let icon_path = icon_dir.join(format!("{size}x{size}.{}", IconFormat::Png));
            if icon_path.exists() {
                icons.insert(size, Icon::new(size, icon_path, IconFormat::Png));
            }
        }

        Ok(icons)
    }
}

// Use the macro to automatically implement all common traits
crate::impl_common_builders!(LinuxBuilder, common);
