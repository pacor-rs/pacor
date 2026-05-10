// Copyright 2016-2019 Cargo-Bundle developers <https://github.com/burtonageo/cargo-bundle>
// Copyright 2019-2024 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! Debian Package Builder
//!
//! This module provides functionality to create Debian (.deb) packages from Flutter applications.
//!
//! ## Debian Package Structure
//!
//! The structure of a Debian package looks something like this:
//!
//! ```text
//! foobar_1.2.3_i386.deb   # Actually an ar archive
//!     debian-binary           # Specifies deb format version (2.0 in our case)
//!     control.tar.gz          # Contains files controlling the installation:
//!         control                  # Basic package metadata
//!         md5sums                  # Checksums for files in data.tar.gz below
//!         postinst                 # Post-installation script (optional)
//!         prerm                    # Pre-uninstallation script (optional)
//!     data.tar.gz             # Contains files to be installed:
//!         usr/bin/foobar                            # Binary executable file
//!         usr/share/applications/foobar.desktop     # Desktop file (for apps)
//!         usr/share/icons/hicolor/...               # Icon files (for apps)
//!         usr/lib/foobar/...                        # Other resource files
//! ```
//!
//! ## Implementation Details
//!
//! For this package builder, we put bundle resource files under `/usr/lib/package_name/`,
//! and then generate the desktop file and control file from the bundle metadata,
//! as well as generating the md5sums file. The builder supports optional maintainer
//! scripts (preinst, postinst, prerm, postrm) when configured.

use std::collections::{BTreeMap, HashMap};
use std::io::Write;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::{fs, io};

use flate2::Compression;
use flate2::write::GzEncoder;
use tar::HeaderMode;
use walkdir::WalkDir;

use crate::AppConfig;
use crate::core::arch::Arch;
use crate::core::config::DebianConfig;
use crate::core::freedesktop::FreeDesktop;
use crate::error::Result;
use crate::platforms::linux::icon::{Icon, copy_icon_to_dir};
use crate::utils::downloader::download_file;
use crate::utils::fs::{clean_dir, copy_dir, ensure_directory_exists};
use crate::utils::template::render_template;

/// Configuration parameters required to build a Debian (.deb) package.
///
/// This struct contains all the necessary information and paths needed
/// to construct a complete Debian package, including application metadata,
/// file paths, and platform-specific configuration.
pub(super) struct DebianPackageBuildConfig<'a> {
    /// Main application configuration containing metadata and settings
    pub(super) app_config: &'a AppConfig,
    /// Name of the application executable file
    pub(super) app_executable_name: &'a str,
    /// Collection of additional application icons indexed by size
    pub(super) additional_icons: &'a BTreeMap<u32, Icon>,
    /// Path to the Flutter build output directory
    pub(super) flutter_build_path: &'a Path,
    /// Root directory of the project
    pub(super) project_root: &'a Path,
    /// Directory containing build tools and templates
    pub(super) tools_directory: &'a Path,
    /// Directory where the final package will be output
    pub(super) output_directory: &'a Path,
    /// Freedesktop
    pub(super) desktop: &'a FreeDesktop,
    /// Target CPU architecture for the package
    pub(super) target_architecture: &'a Arch,
    /// Package version string
    pub(super) package_version: &'a str,
    /// Short description of the application for package metadata
    pub(super) package_description: &'a str,
    /// Debian-specific package configuration
    pub(super) debian_package_config: DebianConfig,
}

/// Builder responsible for creating Debian (.deb) packages.
///
/// This builder handles the complete process of creating a Debian package,
/// including downloading templates, assembling package data, generating
/// control files, and creating the final .deb archive.
pub(super) struct DebianPackageBuilder<'a> {
    app_config: &'a AppConfig,
    app_executable_name: &'a str,
    additional_icons: &'a BTreeMap<u32, Icon>,
    flutter_build_path: &'a Path,
    project_root: &'a Path,
    tools_directory: &'a Path,
    output_directory: &'a Path,
    desktop: &'a FreeDesktop,
    target_architecture: &'a Arch,
    package_version: &'a str,
    package_description: &'a str,
    debian_package_config: DebianConfig,
}

/// Template file information for downloading and processing
struct PackageTemplate<'a> {
    /// Remote filename on the template repository
    remote_filename: &'a str,
    /// Local path where the template should be applied
    local_file_path: &'a Path,
}

impl<'a> DebianPackageBuilder<'a> {
    /// Creates a new Debian package builder from the provided configuration.
    ///
    /// # Arguments
    /// * `config` - Build configuration containing all necessary parameters
    ///
    /// # Returns
    /// A new instance of `DebianPackageBuilder` ready to build packages
    pub(super) fn from_config(config: DebianPackageBuildConfig<'a>) -> Self {
        debug!(
            "Initializing Debian package builder for '{}'",
            config.app_executable_name
        );

        Self {
            app_config: config.app_config,
            app_executable_name: config.app_executable_name,
            additional_icons: config.additional_icons,
            flutter_build_path: config.flutter_build_path,
            project_root: config.project_root,
            tools_directory: config.tools_directory,
            output_directory: config.output_directory,
            desktop: config.desktop,
            target_architecture: config.target_architecture,
            package_version: config.package_version,
            package_description: config.package_description,
            debian_package_config: config.debian_package_config,
        }
    }

    /// Downloads and applies necessary template files for the build process.
    ///
    /// This method downloads Flutter/Linux template files from the remote repository
    /// and replaces placeholders with actual application values. It creates backups
    /// of original files that can be restored later.
    ///
    /// # Returns
    /// A vector of file pairs [modified_file, backup_file] for later restoration
    pub(super) fn download_and_apply_templates(&self) -> Result<Vec<[PathBuf; 2]>> {
        info!("Downloading and applying build templates for Debian package");

        let templates = [
            PackageTemplate {
                remote_filename: "my_application_template.cc",
                local_file_path: &self
                    .project_root
                    .join("linux")
                    .join("runner")
                    .join("my_application.cc"),
            },
            PackageTemplate {
                remote_filename: "CMakeLists_template.txt",
                local_file_path: &self.project_root.join("linux").join("CMakeLists.txt"),
            },
        ];

        let mut modified_files = Vec::with_capacity(templates.len());

        for template in &templates {
            let file_pair = self
                .download_and_apply_template(template.remote_filename, template.local_file_path)?;
            modified_files.push(file_pair);
        }

        info!(
            "Successfully applied {} build templates",
            modified_files.len()
        );
        Ok(modified_files)
    }

    /// Downloads a single template file and applies it to the local project.
    ///
    /// This method handles downloading template files from the remote repository,
    /// caching them locally, replacing placeholders, and creating backups.
    ///
    /// # Arguments
    /// * `remote_filename` - Name of the template file in the remote repository
    /// * `local_file_path` - Path to the local file that will be modified
    ///
    /// # Returns
    /// A tuple of [modified_file_path, backup_file_path]
    fn download_and_apply_template(
        &self,
        remote_filename: &str,
        local_file_path: &Path,
    ) -> Result<[PathBuf; 2]> {
        debug!("Processing template: {}", remote_filename);

        // Generate local filenames and paths
        let local_filename = remote_filename.replace("_template", "");
        let backup_file_path = local_file_path.with_file_name(format!("{}.bak", local_filename));

        let debian_tools_dir = self.tools_directory.join("deb");
        let local_template_path = debian_tools_dir.join(remote_filename);
        ensure_directory_exists(&debian_tools_dir)?;

        // Download or read cached template data
        let template_data = if !local_template_path.exists() {
            debug!("Template not cached, downloading from remote repository");

            let template_url = format!(
                "https://raw.githubusercontent.com/pacor-rs/tools/refs/heads/main/linux/deb/{}",
                remote_filename
            );

            let template_data = download_file(&template_url)?;
            fs::write(&local_template_path, &template_data)?;

            info!(
                "Template downloaded and cached: {}",
                local_template_path.display()
            );

            template_data
        } else {
            debug!("Using cached template: {}", local_template_path.display());
            fs::read(&local_template_path)?
        };

        // Process template content and apply substitutions
        let template_content = String::from_utf8_lossy(&template_data);
        let replacements = HashMap::from([
            ("APP_EXECUTABLE_NAME", self.app_executable_name),
            // ("APP_DISPLAY_NAME", self.app_display_name), // TODO: Add support for display name later
        ]);
        let processed_content = render_template(&template_content, &replacements);

        // Create backup and apply template
        fs::rename(local_file_path, &backup_file_path)?;
        fs::write(local_file_path, &processed_content)?;

        info!(
            "Template applied to project file: {}",
            local_file_path.display()
        );

        Ok([local_file_path.to_path_buf(), backup_file_path])
    }

    /// Restores original project files from their backups.
    ///
    /// This method should be called after the build process is complete
    /// to restore the original state of modified project files.
    ///
    /// # Arguments
    /// * `modified_files` - Vector of [file, backup] pairs to restore
    pub(super) fn restore_original_files(&self, modified_files: &[[PathBuf; 2]]) -> Result<()> {
        debug!("Restoring {} original project files", modified_files.len());

        for [original_file, backup_file] in modified_files {
            fs::rename(backup_file, original_file)?;
            debug!("Restored file: {}", original_file.display());
        }

        info!("All original project files restored successfully");

        Ok(())
    }

    /// Builds the complete Debian package and returns the path to the resulting .deb file.
    ///
    /// This is the main entry point for the build process, orchestrating all
    /// the necessary steps to create a valid Debian package.
    ///
    /// # Returns
    /// Path to the generated .deb package file
    pub(super) fn build_package(&self) -> Result<PathBuf> {
        let package_filename = format!(
            "{}-{}-{}",
            self.app_executable_name, self.package_version, self.target_architecture
        );
        let deb_filename = format!("{}.deb", package_filename);

        let temp_dir = tempfile::TempDir::with_prefix("pacor_deb")?;
        let package_staging_dir = temp_dir.path().join(&package_filename);
        debug!(path = %package_staging_dir.display(), "Created package staging directory");

        // Clean and prepare staging directory
        clean_dir(&package_staging_dir)?;

        let output_deb_path = self.output_directory.join(&deb_filename);
        info!(
            "Starting Debian package build: {} -> {}",
            package_filename,
            output_deb_path.display()
        );

        // Assemble package components
        let package_data_dir = self.assemble_package_filesystem(&package_staging_dir)?;
        let package_control_dir = package_staging_dir.join("control");

        // Generate package metadata and control files
        self.generate_control_file(&package_data_dir, &package_control_dir)?;
        self.generate_maintainer_scripts(&package_control_dir)?;
        generate_md5_checksums_file(&package_control_dir, &package_data_dir)?;

        // Create debian-binary file (required for .deb format specification)
        // See: http://www.tldp.org/HOWTO/Debian-Binary-Package-Building-HOWTO/x60.html#AEN66
        let debian_binary_path = package_staging_dir.join("debian-binary");
        write_file_with_content(&debian_binary_path, "2.0\n")?;

        // Create compressed archives for control and data sections
        debug!("Creating compressed archives for package components");
        let control_archive_path = create_gzipped_tarball(&package_control_dir)?;
        let data_archive_path = create_gzipped_tarball(&package_data_dir)?;

        // Assemble final .deb using AR archive format
        create_ar_archive(
            vec![debian_binary_path, control_archive_path, data_archive_path],
            &output_deb_path,
        )?;

        info!(
            path = %output_deb_path.display(),
            "Debian package build completed successfully",
        );

        Ok(output_deb_path)
    }

    /// Assembles the complete filesystem layout for the package data section.
    ///
    /// This method creates the directory structure that will be installed on the
    /// target system, including the executable, libraries, data files, desktop
    /// entries, and icons.
    ///
    /// # Arguments
    /// * `package_staging_dir` - Root directory for package assembly
    ///
    /// # Returns
    /// Path to the assembled data directory
    fn assemble_package_filesystem(&self, package_staging_dir: &Path) -> Result<PathBuf> {
        let package_data_root = package_staging_dir.join("data");
        debug!(
            directory = %package_data_root.display(),
            "Assembling package filesystem in",
        );

        // Install application executable
        self.install_application_executable(&package_data_root)?;

        // Install shared libraries
        self.install_shared_libraries(&package_data_root)?;

        // Install application data and assets
        self.install_application_data(&package_data_root)?;

        // Install desktop entry
        self.install_desktop_entry(&package_data_root)?;

        // Install application icons
        self.install_application_icons(&package_data_root)?;

        debug!("Package filesystem assembly completed");
        Ok(package_data_root)
    }

    /// Installs the main application executable to the package filesystem.
    fn install_application_executable(&self, data_root: &Path) -> Result<()> {
        let executable_dir = data_root.join("usr/bin");
        ensure_directory_exists(&executable_dir)?;

        let source_executable = self.flutter_build_path.join(self.app_executable_name);
        let target_executable = executable_dir.join(self.app_executable_name);

        fs::copy(&source_executable, &target_executable)?;
        debug!(
            "Application executable installed: {} -> {}",
            source_executable.display(),
            target_executable.display()
        );

        Ok(())
    }

    /// Installs shared libraries required by the application.
    fn install_shared_libraries(&self, data_root: &Path) -> Result<()> {
        let library_source_dir = self.flutter_build_path.join("lib");
        let library_target_dir = data_root.join("usr/lib").join(self.app_executable_name);

        ensure_directory_exists(&library_target_dir)?;
        copy_dir(&library_source_dir, &library_target_dir)?;

        debug!(
            "Shared libraries installed: {} -> {}",
            library_source_dir.display(),
            library_target_dir.display()
        );

        Ok(())
    }

    /// Installs application data and assets.
    fn install_application_data(&self, data_root: &Path) -> Result<()> {
        let data_source_dir = self.flutter_build_path.join("data");
        let data_target_dir = data_root.join("usr/share").join(self.app_executable_name);

        ensure_directory_exists(&data_target_dir)?;
        copy_dir(&data_source_dir, &data_target_dir)?;

        debug!(
            "Application data installed: {} -> {}",
            data_source_dir.display(),
            data_target_dir.display()
        );

        Ok(())
    }

    /// Installs the desktop entry file for system integration.
    fn install_desktop_entry(&self, data_root: &Path) -> Result<()> {
        let applications_dir = data_root.join("usr/share/applications");
        ensure_directory_exists(&applications_dir)?;

        let desktop_target = applications_dir.join(format!("{}.desktop", self.app_executable_name));
        self.desktop.write(&desktop_target)?;

        debug!("Desktop entry installed: {}", desktop_target.display());

        Ok(())
    }

    /// Installs application icons into the hicolor icon theme directory structure.
    ///
    /// This follows the XDG Icon Theme Specification for proper system integration.
    fn install_application_icons(&self, data_root: &Path) -> Result<()> {
        let hicolor_icons_dir = data_root.join("usr/share/icons/hicolor");
        ensure_directory_exists(&hicolor_icons_dir)?;

        copy_icon_to_dir(
            self.app_executable_name,
            &hicolor_icons_dir,
            self.additional_icons.values(),
        )?;

        debug!(
            "Application icons installed to hicolor theme: {}",
            hicolor_icons_dir.display()
        );

        Ok(())
    }

    /// Generates the `control` file containing package metadata for dpkg.
    ///
    /// The control file format follows the Debian Policy Manual specification:
    /// https://www.debian.org/doc/debian-policy/ch-controlfields.html
    ///
    /// # Arguments
    /// * `data_dir` - Package data directory (for calculating installed size)
    /// * `control_dir` - Directory where control files are stored
    fn generate_control_file(&self, data_dir: &Path, control_dir: &Path) -> Result<()> {
        ensure_directory_exists(control_dir)?;
        let control_file_path = control_dir.join("control");
        let mut control_file = fs::File::create(&control_file_path)?;

        debug!(
            path = %control_file_path.display(),
            "Generating package control file",
        );

        // Package name must not contain underscores per Debian policy
        let debian_package_name = self.app_executable_name.replace("_", "-");
        writeln!(control_file, "Package: {}", debian_package_name)?;
        writeln!(control_file, "Version: {}", self.package_version)?;
        writeln!(control_file, "Architecture: {}", self.target_architecture)?;

        // Calculate installed size in kilobytes as required by Debian policy
        let installed_size_kb = calculate_installed_size_in_kb(data_dir)?;
        writeln!(control_file, "Installed-Size: {}", installed_size_kb)?;

        // Use authors if available, otherwise fall back to publisher
        let package_maintainer = self
            .app_config
            .authors()
            .map(|author_list| {
                author_list
                    .iter()
                    .map(|name| name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_else(|| self.app_config.publisher().to_string());
        writeln!(control_file, "Maintainer: {}", package_maintainer)?;

        writeln!(control_file, "Homepage: {}", self.app_config.url())?;
        writeln!(control_file, "Description: {}", self.package_description)?;

        // Add optional Debian-specific fields with sensible defaults
        if let Some(section) = self.debian_package_config.section() {
            writeln!(control_file, "Section: {}", section)?;
        }

        let package_priority = self.debian_package_config.priority().unwrap_or("optional");
        writeln!(control_file, "Priority: {}", package_priority)?;

        // Add package relationship fields if specified
        self.write_package_relationships(&mut control_file)?;

        control_file.flush()?;
        debug!("Package control file generated successfully");

        Ok(())
    }

    /// Writes package relationship fields (dependencies, conflicts, etc.) to the control file.
    fn write_package_relationships(&self, control_file: &mut fs::File) -> Result<()> {
        if let Some(dependencies) = self.debian_package_config.depends() {
            writeln!(control_file, "Depends: {}", dependencies.join(", "))?;
        }

        if let Some(recommendations) = self.debian_package_config.recommends() {
            writeln!(control_file, "Recommends: {}", recommendations.join(", "))?;
        }

        if let Some(provides) = self.debian_package_config.provides() {
            writeln!(control_file, "Provides: {}", provides.join(", "))?;
        }

        if let Some(conflicts) = self.debian_package_config.conflicts() {
            writeln!(control_file, "Conflicts: {}", conflicts.join(", "))?;
        }

        if let Some(replaces) = self.debian_package_config.replaces() {
            writeln!(control_file, "Replaces: {}", replaces.join(", "))?;
        }

        Ok(())
    }

    /// Generates maintainer scripts (preinst, postinst, prerm, postrm) if configured.
    ///
    /// These scripts are executed at various stages of package installation and removal.
    fn generate_maintainer_scripts(&self, control_dir: &Path) -> Result<()> {
        let script_mappings = [
            ("preinst", self.debian_package_config.pre_install_script()),
            ("postinst", self.debian_package_config.post_install_script()),
            ("prerm", self.debian_package_config.pre_remove_script()),
            ("postrm", self.debian_package_config.post_remove_script()),
        ];

        for (script_name, script_path_option) in script_mappings {
            if let Some(script_path) = script_path_option {
                let target_script_path = control_dir.join(script_name);
                copy_file_with_executable_permissions(script_path, &target_script_path)?;
                debug!("Maintainer script installed: {}", script_name);
            }
        }

        Ok(())
    }
}

/// Calculates the total installed size in kilobytes as required by Debian policy.
///
/// Each regular file contributes its size rounded up to the next full kilobyte.
/// Other filesystem objects (directories, symlinks, etc.) contribute 1 KiB each.
///
/// # Arguments
/// * `directory` - Directory to calculate size for
///
/// # Returns
/// Total size in kilobytes
fn calculate_installed_size_in_kb(directory: &Path) -> Result<u64> {
    let mut total_size_kb: u64 = 0;

    for entry in WalkDir::new(directory) {
        let entry = entry?;
        let metadata = entry.metadata()?;

        if metadata.is_file() {
            // Round file size up to next kilobyte
            total_size_kb += metadata.len().div_ceil(1024);
        } else {
            // Directories, symlinks, etc. count as 1 KiB each
            total_size_kb += 1;
        }
    }

    Ok(total_size_kb)
}

/// Copies a script file and sets executable permissions (0o755).
///
/// # Arguments
/// * `source_path` - Path to the source script file
/// * `target_path` - Path where the script should be copied
fn copy_file_with_executable_permissions(source_path: &Path, target_path: &Path) -> Result<()> {
    let mut source_file = fs::File::open(source_path)?;
    let mut target_file = fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .mode(0o755)
        .open(target_path)?;

    io::copy(&mut source_file, &mut target_file)?;
    Ok(())
}

/// Generates the `md5sums` file containing MD5 checksums of all package files.
///
/// This file is used by dpkg to verify package integrity and detect file modifications.
///
/// # Arguments
/// * `control_dir` - Directory where the md5sums file will be created
/// * `data_dir` - Directory containing files to checksum
fn generate_md5_checksums_file(control_dir: &Path, data_dir: &Path) -> Result<()> {
    let md5sums_file_path = control_dir.join("md5sums");
    let mut md5sums_file = fs::File::create(&md5sums_file_path)?;

    debug!("Generating MD5 checksums for package files");

    for entry in WalkDir::new(data_dir) {
        let entry = entry?;

        // Skip directories - only checksum regular files
        if entry.file_type().is_dir() {
            continue;
        }

        let file_path = entry.path();
        let mut file = fs::File::open(file_path)?;
        let mut md5_hasher = md5::Context::new();

        io::copy(&mut file, &mut md5_hasher)?;
        let file_digest = md5_hasher.finalize();

        // Write checksum in format: "checksum  relative_path"
        write!(md5sums_file, "{:x}", file_digest)?;

        let relative_path = file_path.strip_prefix(data_dir)?;
        let path_string = relative_path.to_str().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("Non-UTF-8 path encountered: {:?}", relative_path),
            )
        })?;

        writeln!(md5sums_file, "  {}", path_string)?;
    }

    debug!(
        "MD5 checksums file generated: {}",
        md5sums_file_path.display()
    );
    Ok(())
}

/// Writes content to a file, creating parent directories if necessary.
///
/// # Arguments
/// * `file_path` - Path where the file should be created
/// * `content` - Content to write to the file
fn write_file_with_content<P: AsRef<Path>>(file_path: P, content: &str) -> Result<()> {
    let file_path = file_path.as_ref();

    // Ensure parent directory exists
    if let Some(parent_dir) = file_path.parent() {
        ensure_directory_exists(parent_dir)?;
    }

    let mut file = fs::File::create(file_path)?;
    file.write_all(content.as_bytes())?;
    file.flush()?;

    Ok(())
}

/// Creates a gzip-compressed tar archive from a directory.
///
/// The output archive is placed in the parent directory of the source
/// and named `<dirname>.tar.gz`. The source directory is preserved.
///
/// # Arguments
/// * `source_directory` - Directory to archive
///
/// # Returns
/// Path to the created archive file
fn create_gzipped_tarball<P: AsRef<Path>>(source_directory: P) -> Result<PathBuf> {
    let source_directory = source_directory.as_ref();
    let archive_path = source_directory.with_extension("tar.gz");

    debug!(
        "Creating compressed tarball: {} -> {}",
        source_directory.display(),
        archive_path.display()
    );

    let output_file = fs::File::create(&archive_path)?;
    let gzip_encoder = GzEncoder::new(output_file, Compression::default());
    let gzip_encoder = write_deterministic_tar_archive(source_directory, gzip_encoder)?;
    let mut final_file = gzip_encoder.finish()?;
    final_file.flush()?;

    Ok(archive_path)
}

/// Writes a deterministic tar archive to the provided writer.
///
/// This function creates reproducible tar archives by using deterministic
/// header modes and properly handling different file types.
///
/// # Arguments
/// * `source_directory` - Directory to archive
/// * `writer` - Writer where the tar data will be written
///
/// # Returns
/// The writer after all data has been written
fn write_deterministic_tar_archive<P: AsRef<Path>, W: Write>(
    source_directory: P,
    writer: W,
) -> Result<W> {
    let source_directory = source_directory.as_ref();
    let mut tar_builder = tar::Builder::new(writer);

    for entry in WalkDir::new(source_directory) {
        let entry = entry?;
        let relative_path = entry.path().strip_prefix(source_directory)?;

        // Skip the root directory itself (empty relative path)
        if relative_path.as_os_str().is_empty() {
            continue;
        }

        let file_metadata = entry.metadata()?;
        let mut tar_header = tar::Header::new_gnu();
        tar_header.set_metadata_in_mode(&file_metadata, HeaderMode::Deterministic);
        tar_header.set_mtime(file_metadata.mtime() as u64);

        if file_metadata.is_dir() {
            tar_builder.append_data(&mut tar_header, relative_path, &mut io::empty())?;
        } else if file_metadata.is_file() {
            let mut file = fs::File::open(entry.path())?;
            tar_builder.append_data(&mut tar_header, relative_path, &mut file)?;
        } else if file_metadata.file_type().is_symlink() {
            let symlink_target = fs::read_link(entry.path())?;
            let target_string = symlink_target.to_str().ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Symlink target contains invalid UTF-8 characters",
                )
            })?;

            tar_header.set_entry_type(tar::EntryType::Symlink);
            tar_header.set_size(0);
            tar_builder.append_link(&mut tar_header, relative_path, target_string)?;
        }
        // Other file types (device files, etc.) are ignored as they're not suitable for .deb packages
    }

    Ok(tar_builder.into_inner()?)
}

/// Creates an AR archive (used for .deb packages) from a list of files.
///
/// The AR format is the container format used by Debian packages,
/// containing the debian-binary, control.tar.gz, and data.tar.gz files.
///
/// # Arguments
/// * `input_files` - Vector of file paths to include in the archive
/// * `output_path` - Path where the AR archive will be created
fn create_ar_archive(input_files: Vec<PathBuf>, output_path: &Path) -> Result<()> {
    debug!("Creating AR archive: {}", output_path.display());

    let output_file = fs::File::create(output_path)?;
    let mut ar_builder = ar::Builder::new(output_file);

    for file_path in &input_files {
        ar_builder.append_path(file_path)?;
    }

    ar_builder.into_inner()?.flush()?;
    Ok(())
}
