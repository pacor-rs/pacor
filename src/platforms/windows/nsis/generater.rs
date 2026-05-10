use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::{fs, io, thread};

use anyhow::{Context, Result};

use crate::error::{BuildError, PackageError};
use crate::utils::downloader::download_file;
use crate::utils::fs::ensure_directory_exists;
use crate::utils::template::render_template;
use crate::{config::AppConfig, i18n::Locale};

/// Base URL for downloading NSIS tools and templates
const NSIS_TOOLS_BASE_URL: &str =
    "https://raw.githubusercontent.com/pacor-rs/tools/main/windows/nsis";

/// Required NSIS files that must be downloaded for installer generation
const NSIS_REQUIRED_FILES: &[&str] = &[
    "template.nsi",
    "Plugins/nsProcess.nsh",
    "Plugins/nsProcess.dll",
];

impl Locale {
    /// Returns the NSIS language code for this locale.
    ///
    /// Maps application locales to NSIS language identifiers used in installer scripts.
    ///
    /// # References
    /// - [NSIS Language Examples](https://nsis.sourceforge.io/Examples/languages.nsi)
    /// - [Modern UI Multi-Language](https://nsis.sourceforge.io/Examples/Modern%20UI/MultiLanguage.nsi)
    fn as_nsis_lang_code(&self) -> &'static str {
        match self {
            Locale::EnUs | Locale::EnGb => "English",
            Locale::ZhCn => "SimpChinese",
            Locale::ZhTw => "TradChinese",
            Locale::JaJp => "Japanese",
        }
    }
}

/// Encapsulates all language-specific content for NSIS script generation.
///
/// This structure holds the various language-dependent sections that will be
/// interpolated into the NSIS installer script template.
struct LanguageContent {
    /// Language macro insertions (e.g., `!insertmacro MUI_LANGUAGE "English"`)
    language_macros: String,

    /// Language file include directives
    language_includes: String,

    /// Version information blocks for each language
    version_info_blocks: String,

    /// Language-specific string definitions
    localized_strings: String,
}

/// Generator for NSIS (Nullsoft Scriptable Install System) installer scripts.
///
/// This generator creates Windows installer packages by:
/// 1. Downloading required NSIS tools and language files
/// 2. Generating localized content for all supported languages
/// 3. Rendering the final installer script from a template
#[derive(Debug)]
pub(super) struct NSISGenerator<'a> {
    /// Application configuration containing metadata and localization info
    pub(super) app_config: &'a AppConfig,

    /// Application name (used as fallback for localized names)
    pub(super) app_name: &'a str,

    /// Default application description
    pub(super) default_description: &'a str,

    /// Application version string (e.g., "1.0.0")
    pub(super) version: &'a str,

    /// Name of the main executable file
    pub(super) executable_name: &'a str,

    /// Base name for the installer file
    pub(super) installer_name: &'a str,

    /// Default copyright notice
    pub(super) default_legal_copyright: &'a str,

    /// Directory where NSIS tools and templates are installed
    pub(super) nsis_install_dir: &'a Path,

    /// Path to the application icon file
    pub(super) icon_path: &'a Path,

    /// Directory containing the built application files
    pub(super) build_output_dir: &'a Path,

    /// Directory where the final installer will be output
    pub(super) installer_output_dir: &'a Path,
}

impl<'a> NSISGenerator<'a> {
    /// Downloads a file from the given URL and saves it to the specified path.
    ///
    /// # Arguments
    /// * `url` - The URL to download from
    /// * `path` - The local path where the file should be saved
    ///
    /// # Errors
    /// Returns an error if the download fails or the file cannot be written.
    fn download_and_save(url: String, path: PathBuf) -> Result<()> {
        debug!("Downloading file from {} to {}", url, path.display());

        download_file(&url)
            .and_then(|data| {
                debug!(
                    "Successfully downloaded {} bytes, writing to {}",
                    data.len(),
                    path.display()
                );
                fs::write(&path, data)
                    .map_err(PackageError::Io)
                    .context("Failed to write file")
            })
            .map_err(|e| {
                error!(
                    "Failed to download/save file from {} to {}: {}",
                    url,
                    path.display(),
                    e
                );
                e
            })?;

        info!("Successfully saved file to {}", path.display());
        Ok(())
    }

    /// Downloads multiple files in parallel using scoped threads.
    ///
    /// This method optimizes download performance by parallelizing network operations.
    /// Files that already exist on disk are skipped.
    ///
    /// # Type Parameters
    /// * `I` - An iterator over items to download
    /// * `F` - A function that maps each item to a (URL, path) tuple
    ///
    /// # Arguments
    /// * `items` - Collection of items to process
    /// * `url_path_mapper` - Function that converts each item to a URL and destination path
    ///
    /// # Errors
    /// Returns an error if any download fails or if thread operations fail.
    fn download_files_parallel<I, F>(&self, items: I, url_path_mapper: F) -> Result<()>
    where
        I: IntoIterator,
        I::Item: Send + 'static,
        F: Fn(&I::Item) -> (String, PathBuf) + Send + Sync + 'static,
    {
        let items_vec: Vec<_> = items.into_iter().collect();
        let total_items = items_vec.len();

        debug!("Starting parallel download of {} files", total_items);

        thread::scope(|scope| {
            let download_handles: Vec<_> = items_vec
                .into_iter()
                .enumerate()
                .filter_map(|(index, item)| {
                    let (url, path) = url_path_mapper(&item);

                    // Skip files that already exist
                    if path.exists() {
                        debug!(
                            "File [{}/{}] already exists, skipping: {}",
                            index + 1,
                            total_items,
                            path.display()
                        );
                        return None;
                    }

                    // Ensure parent directory exists before spawning download thread
                    if let Some(parent) = path.parent()
                        && let Err(e) = ensure_directory_exists(parent)
                    {
                        error!(
                            "Failed to create directory {} for file {}: {}",
                            parent.display(),
                            path.display(),
                            e
                        );
                        return None;
                    }

                    debug!(
                        "Spawning download thread [{}/{}] for {}",
                        index + 1,
                        total_items,
                        url
                    );
                    Some(scope.spawn(move || Self::download_and_save(url, path)))
                })
                .collect();

            let active_downloads = download_handles.len();
            debug!(
                "Waiting for {} active downloads to complete",
                active_downloads
            );

            // Wait for all downloads to complete and collect results
            for (index, handle) in download_handles.into_iter().enumerate() {
                handle.join().map_err(|e| {
                    error!("Download thread {} panicked: {:?}", index + 1, e);
                    PackageError::Threading(format!("Download thread panicked: {:?}", e))
                })??;
                debug!(
                    "Download thread [{}/{}] completed",
                    index + 1,
                    active_downloads
                );
            }

            debug!(
                "Successfully completed {} parallel downloads",
                active_downloads
            );
            Ok::<(), anyhow::Error>(())
        })?;

        Ok(())
    }

    /// Downloads and prepares all required NSIS files.
    ///
    /// This includes:
    /// - Core NSIS template and plugin files
    /// - Language files for all configured i18n
    ///
    /// # Errors
    /// Returns an error if any required file cannot be downloaded.
    fn prepare_required_nsis_files(&self) -> Result<()> {
        info!("Preparing required NSIS files from {}", NSIS_TOOLS_BASE_URL);

        // Download core NSIS files
        debug!(
            "Downloading {} core NSIS files...",
            NSIS_REQUIRED_FILES.len()
        );
        let base_url = NSIS_TOOLS_BASE_URL.to_string();
        let nsis_dir = self.nsis_install_dir.to_path_buf();

        self.download_files_parallel(
            NSIS_REQUIRED_FILES.iter().map(|s| s.to_string()),
            move |file: &String| {
                let url = format!("{}/{}", base_url, file);
                let path = nsis_dir.join(file);
                (url, path)
            },
        )?;

        // Download language files for all configured locales
        let languages_dir = self.nsis_install_dir.join("languages");
        debug!("Creating languages directory: {}", languages_dir.display());
        ensure_directory_exists(&languages_dir)?;

        let locales: Vec<_> = self.app_config.i18n().keys().cloned().collect();
        debug!(
            "Downloading language files for {} locales: {:?}",
            locales.len(),
            locales
                .iter()
                .map(|l| l.as_nsis_lang_code())
                .collect::<Vec<_>>()
        );

        let base_url = NSIS_TOOLS_BASE_URL.to_string();

        self.download_files_parallel(locales, move |locale: &Locale| {
            let lang_code = locale.as_nsis_lang_code();
            let filename = format!("{}.nsh", lang_code);
            let url = format!("{}/languages/{}", base_url, filename);
            let path = languages_dir.join(filename);
            (url, path)
        })?;

        info!("Successfully prepared all required NSIS files");

        Ok(())
    }

    /// Converts an NSIS language code to its corresponding constant identifier.
    ///
    /// # Example
    /// ```
    /// assert_eq!(get_nsis_lang_constant("English"), "LANG_ENGLISH");
    /// ```
    fn get_nsis_lang_constant(lang_code: &str) -> String {
        format!("LANG_{}", lang_code.to_uppercase())
    }

    /// Generates all language-specific content for the NSIS script in a single pass.
    ///
    /// This method efficiently processes all localized metadata and produces:
    /// - Language macro insertions
    /// - Language file includes
    /// - Version information for each language
    /// - Localized string definitions
    ///
    /// # Returns
    /// A `LanguageContent` struct containing all generated language-specific sections.
    fn generate_all_language_content(&self) -> LanguageContent {
        let locale_count = self.app_config.i18n().len();
        debug!("Generating language content for {} locales", locale_count);

        let mut language_macros = Vec::with_capacity(locale_count);

        // Add language file includes, with the default language first
        let default_locale = self.app_config.default_locale();
        let default_lang_code = default_locale.as_nsis_lang_code();
        debug!(
            "Default locale: {} ({})",
            default_lang_code,
            self.app_config
                .i18n()
                .get(default_locale)
                .and_then(|m| m.name())
                .unwrap_or("Unknown")
        );

        let mut language_includes = Vec::with_capacity(locale_count);
        language_includes.push(format!(
            "!include \"${{NSISDIR}}\\languages\\{}.nsh\"",
            default_lang_code
        ));

        let mut version_info_lines = vec![format!("VIProductVersion \"{}.0\"", self.version)];
        let mut localized_strings = Vec::with_capacity(locale_count);

        // Generate all content in a single iteration
        for (locale, metadata) in self.app_config.i18n() {
            let lang_code = locale.as_nsis_lang_code();
            let nsis_lang_const = Self::get_nsis_lang_constant(lang_code);

            debug!(
                "Processing locale: {} - Name: {}, Publisher: {}",
                lang_code,
                metadata.name().unwrap_or(self.app_name),
                metadata.publisher()
            );

            // 1. Language macro insertions
            language_macros.push(format!("!insertmacro MUI_LANGUAGE \"{}\"", lang_code));

            // 2. Language file includes (skip default locale as it's already included)
            if locale != self.app_config.default_locale() {
                language_includes.push(format!(
                    "!include \"${{NSISDIR}}\\languages\\{}.nsh\"",
                    lang_code
                ));
            }

            // 3. Version information block
            version_info_lines.push(format!("\n; {} Version Information", lang_code));

            let version_info_keys = [
                ("ProductName", metadata.name().unwrap_or(self.app_name)),
                ("ProductVersion", self.version),
                ("CompanyName", metadata.publisher()),
                ("FileVersion", self.version),
            ];

            for (key, value) in version_info_keys {
                version_info_lines.push(format!(
                    "VIAddVersionKey /LANG=${{{}}} \"{}\" \"{}\"",
                    nsis_lang_const, key, value
                ));
            }

            version_info_lines.push(format!(
                "VIAddVersionKey /LANG=${{{}}} \"FileDescription\" \"{}\"",
                nsis_lang_const,
                metadata.description().unwrap_or(self.default_description)
            ));

            version_info_lines.push(format!(
                "VIAddVersionKey /LANG=${{{}}} \"LegalCopyright\" \"{}\"",
                nsis_lang_const,
                metadata.copyright().unwrap_or(self.default_legal_copyright)
            ));

            // 4. Localized string definitions
            localized_strings.push(format!("; {} LangStrings", lang_code));

            localized_strings.push(format!(
                "LangString APP_DISPLAY_NAME ${{{}}} \"{}\"",
                nsis_lang_const,
                metadata
                    .display_name()
                    .unwrap_or(metadata.name().unwrap_or(self.app_name))
            ));

            localized_strings.push(format!(
                "LangString APP_PUBLISHER ${{{}}} \"{}\"\n",
                nsis_lang_const,
                metadata.publisher()
            ));
        }

        debug!(
            "Successfully generated language content for all {} locales",
            locale_count
        );

        LanguageContent {
            language_macros: language_macros.join("\n"),
            language_includes: language_includes.join("\n"),
            version_info_blocks: version_info_lines.join("\n"),
            localized_strings: localized_strings.join("\n"),
        }
    }

    /// Renders the NSIS script by replacing placeholders in the template.
    ///
    /// # Arguments
    /// * `template` - The NSIS template content with placeholders
    ///
    /// # Returns
    /// The fully rendered NSIS script content with all placeholders replaced.
    ///
    /// # Errors
    /// Returns an error if rendering fails.
    fn render_nsis_script(&self, template: &str) -> Result<String> {
        debug!("Starting NSIS script rendering");

        let installer_output_path = self.installer_output_dir.join(self.installer_name);

        debug!("Generating language-specific content");
        let lang_content = self.generate_all_language_content();

        let icon_path_str = self.icon_path.display().to_string();
        let build_output_dir_str = self.build_output_dir.display().to_string();
        let installer_output_path_str = installer_output_path.display().to_string();

        debug!("Template variables:");
        debug!("  APP_NAME: {}", self.app_name);
        debug!("  APP_VERSION: {}", self.version);
        debug!("  APP_URL: {}", self.app_config.url());
        debug!("  APP_EXECUTABLE: {}", self.executable_name);
        debug!("  APP_ICON_PATH: {}", icon_path_str);
        debug!("  BUILD_OUTPUT_DIR: {}", build_output_dir_str);
        debug!("  INSTALLER_OUTPUT_PATH: {}", installer_output_path_str);

        // Build replacement map for template rendering
        let replacements = HashMap::from([
            ("APP_NAME", self.app_name),
            ("APP_VERSION", self.version),
            ("APP_URL", self.app_config.url()),
            ("APP_EXECUTABLE", self.executable_name),
            ("APP_ICON_PATH", icon_path_str.as_str()),
            ("BUILD_OUTPUT_DIR", build_output_dir_str.as_str()),
            ("INSTALLER_OUTPUT_PATH", installer_output_path_str.as_str()),
            ("LANGUAGE_MACROS", lang_content.language_macros.as_str()),
            ("LANGUAGE_INCLUDE", lang_content.language_includes.as_str()),
            ("VERSION_INFO", lang_content.version_info_blocks.as_str()),
            ("LANGUAGE_STRINGS", lang_content.localized_strings.as_str()),
        ]);

        debug!(
            "Rendering template with {} placeholders",
            replacements.len()
        );
        let rendered_script = render_template(template, &replacements)?;

        debug!(
            "Successfully rendered NSIS script ({} bytes)",
            rendered_script.len()
        );

        Ok(rendered_script)
    }

    /// Generates the complete NSIS installer script.
    ///
    /// This is the main entry point for NSIS script generation. It:
    /// 1. Downloads required NSIS files
    /// 2. Reads the template
    /// 3. Generates localized content
    /// 4. Renders and saves the final script
    ///
    /// # Returns
    /// The path to the generated NSIS script file.
    ///
    /// # Errors
    /// Returns an error if any step in the generation process fails.
    pub(super) fn generate(&self, temp_dir: &Path) -> Result<PathBuf> {
        info!("Starting NSIS installer script generation...");

        self.prepare_required_nsis_files()?;

        let template_path = self.nsis_install_dir.join("template.nsi");
        let template_content = fs::read_to_string(&template_path).map_err(|e| {
            PackageError::Io(io::Error::new(
                e.kind(),
                format!(
                    "Failed to read NSIS template at {}: {}",
                    template_path.display(),
                    e
                ),
            ))
        })?;

        // Render the template with all replacements
        let nsis_script_content = self.render_nsis_script(&template_content).map_err(|e| {
            BuildError::TemplateProcessingFailed {
                template: "template.nsi".to_string(),
                reason: e.to_string(),
            }
        })?;

        let output_path = temp_dir.join("installer.nsi");

        // Write the generated script to disk
        fs::write(&output_path, nsis_script_content).map_err(|e| {
            PackageError::Io(io::Error::new(
                e.kind(),
                format!(
                    "Failed to write NSIS script to {}: {}",
                    output_path.display(),
                    e
                ),
            ))
        })?;

        info!(
            path = %output_path.display(),
            "NSIS script generated successfully",
        );
        Ok(output_path)
    }
}
