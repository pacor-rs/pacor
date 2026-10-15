//! Rendering of the pinned NSIS template into an installer script.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::{fs, io};

use anyhow::Result;

use pacor_core::error::{BuildError, Error};
use pacor_core::utils::template::render_template;
use pacor_core::{config::AppConfig, i18n::Locale};

/// Returns the NSIS language code for a locale.
///
/// Also used by [`crate::nsis`] to fetch exactly the language files the script
/// will include.
///
/// # References
/// - [NSIS Language Examples](https://nsis.sourceforge.io/Examples/languages.nsi)
/// - [Modern UI Multi-Language](https://nsis.sourceforge.io/Examples/Modern%20UI/MultiLanguage.nsi)
pub(super) fn nsis_lang_code(locale: &Locale) -> &'static str {
    match locale {
        Locale::EnUs | Locale::EnGb => "English",
        Locale::ZhCn => "SimpChinese",
        Locale::ZhTw => "TradChinese",
        Locale::JaJp => "Japanese",
    }
}

/// Converts an NSIS language code to its constant identifier.
///
/// The MUI language files define `${LANG_ENGLISH}`, `${LANG_SIMPCHINESE}`, … so
/// the constant is the upper-cased language name with a `LANG_` prefix.
fn nsis_lang_constant(lang_code: &str) -> String {
    format!("LANG_{}", lang_code.to_uppercase())
}

/// Highest value NSIS accepts for one `VIProductVersion` component.
const NSIS_VERSION_COMPONENT_MAX: u32 = u16::MAX as u32;

/// Converts a pubspec version into the `X.X.X.X` form `VIProductVersion` needs.
///
/// Pubspec versions are semver, and Flutter's default is `1.0.0+1`: the build
/// number is a separate field, so it becomes the fourth component
/// (`1.0.0.1`). Pre-release identifiers are dropped (`1.2.0-beta.1+7` becomes
/// `1.2.0.7`) and components are clamped, because Windows version fields are
/// 16-bit.
fn nsis_product_version(version: &str) -> String {
    /// Parses one dotted component, ignoring anything non-numeric.
    fn component(value: &str) -> Option<u32> {
        let digits: String = value.chars().take_while(char::is_ascii_digit).collect();
        digits.parse().ok()
    }

    let (base, build) = version.split_once('+').unwrap_or((version, ""));

    let mut components: Vec<u32> = base.split('.').map_while(component).take(3).collect();
    components.resize(3, 0);
    components.push(component(build).unwrap_or(0));

    components
        .iter()
        .map(|component| component.min(&NSIS_VERSION_COMPONENT_MAX).to_string())
        .collect::<Vec<_>>()
        .join(".")
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
/// 1. Reading the NSIS template cached from the pinned tag
/// 2. Generating localized content for all supported languages
/// 3. Rendering the final installer script
///
/// The template files themselves are provisioned by
/// [`crate::nsis::NsisBuilder`], which keeps this module free of network access.
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

    /// Directory where NSIS tools and templates are cached
    pub(super) nsis_install_dir: &'a Path,

    /// Path to the application icon file
    pub(super) icon_path: &'a Path,

    /// Directory containing the built application files
    pub(super) build_output_dir: &'a Path,

    /// Directory where the final installer will be output
    pub(super) installer_output_dir: &'a Path,
}

impl<'a> NSISGenerator<'a> {
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
        let default_lang_code = nsis_lang_code(default_locale);
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
        language_includes.push(self.language_include(default_lang_code));

        let mut version_info_lines = vec![format!(
            "VIProductVersion \"{}\"",
            nsis_product_version(self.version)
        )];
        let mut localized_strings = Vec::with_capacity(locale_count);

        // Generate all content in a single iteration
        let mut locales: Vec<_> = self.app_config.i18n().iter().collect();
        locales.sort_by_key(|(locale, _)| (*locale).clone());
        for (locale, metadata) in locales {
            let lang_code = nsis_lang_code(locale);
            let nsis_lang_const = nsis_lang_constant(lang_code);

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
                language_includes.push(self.language_include(lang_code));
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

    /// `!include` directive for one language's installer strings.
    ///
    /// The template uses `SECTION_*`, `DESC_*`, `SHORTCUT_*` and `MSG_*`
    /// LangStrings that NSIS does not define: they come from the
    /// `languages/<NSIS language>.nsh` files of the pinned tag, which live in
    /// pacor's cache. The path therefore has to be absolute — the template's own
    /// example points at `${NSISDIR}\languages`, a directory NSIS does not ship
    /// (`Contrib\Language files` holds the MUI language files instead).
    fn language_include(&self, lang_code: &str) -> String {
        let path = self
            .nsis_install_dir
            .join("languages")
            .join(format!("{lang_code}.nsh"));

        format!("!include \"{}\"", path.display())
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
    /// 1. Reads the template cached from the pinned tag
    /// 2. Generates localized content
    /// 3. Renders and saves the final script
    ///
    /// # Returns
    /// The path to the generated NSIS script file.
    ///
    /// # Errors
    /// Returns an error if the template cannot be read or rendered.
    pub(super) fn generate(&self, temp_dir: &Path) -> Result<PathBuf> {
        info!("Starting NSIS installer script generation...");

        let template_path = self.nsis_install_dir.join("template.nsi");
        let template_content = fs::read_to_string(&template_path).map_err(|e| {
            Error::Io(io::Error::new(
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
            BuildError::TemplateProcessing {
                template: "template.nsi".to_string(),
                reason: e.to_string(),
            }
        })?;

        let output_path = temp_dir.join("installer.nsi");

        // Write the generated script to disk
        fs::write(&output_path, nsis_script_content).map_err(|e| {
            Error::Io(io::Error::new(
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

#[cfg(test)]
mod tests {
    use super::*;

    use crate::test_support::{
        app_config, bilingual_config, require_nsis_toolchain, write_minimal_icon,
    };
    use pacor_core::i18n::Locale;

    /// A generator whose paths are only ever inspected, never touched.
    fn generator<'a>(app_config: &'a AppConfig, nsis_install_dir: &'a Path) -> NSISGenerator<'a> {
        NSISGenerator {
            app_config,
            app_name: "my_app",
            default_description: "desc",
            version: "1.0.0+1",
            executable_name: "my_app.exe",
            installer_name: "my_app-1.0.0+1-x64-setup.exe",
            default_legal_copyright: "Copyright",
            nsis_install_dir,
            icon_path: Path::new("/project/windows/runner/resources/app_icon.ico"),
            build_output_dir: Path::new("/project/build/windows/x64/runner/Release"),
            installer_output_dir: Path::new("/project/dist"),
        }
    }

    fn english_config() -> AppConfig {
        app_config(&[Locale::EnUs])
    }

    #[test]
    fn nsis_lang_constant_matches_the_mui_defines() {
        assert_eq!(nsis_lang_constant("English"), "LANG_ENGLISH");
        assert_eq!(nsis_lang_constant("SimpChinese"), "LANG_SIMPCHINESE");
        assert_eq!(nsis_lang_constant("TradChinese"), "LANG_TRADCHINESE");
        assert_eq!(nsis_lang_constant("Japanese"), "LANG_JAPANESE");
    }

    /// `VIProductVersion` rejects anything but four numeric components, and the
    /// Flutter default version is `1.0.0+1`.
    #[test]
    fn the_product_version_is_always_four_numeric_components() {
        for (version, expected) in [
            ("1.0.0+1", "1.0.0.1"),
            ("1.0.0", "1.0.0.0"),
            ("2.3.4+56", "2.3.4.56"),
            ("1.0.0-beta.1+7", "1.0.0.7"),
            ("1.2", "1.2.0.0"),
            ("1", "1.0.0.0"),
            ("", "0.0.0.0"),
            ("1.0.0+", "1.0.0.0"),
        ] {
            let rendered = nsis_product_version(version);

            assert_eq!(rendered, expected, "version {version:?}");
            assert_eq!(
                rendered.split('.').count(),
                4,
                "version {version:?} produced {rendered}"
            );
            assert!(
                rendered
                    .split('.')
                    .all(|part| part.parse::<u32>().is_ok_and(|n| n <= u32::from(u16::MAX))),
                "version {version:?} produced out-of-range components: {rendered}"
            );
        }
    }

    #[test]
    fn oversized_version_components_are_clamped() {
        assert_eq!(nsis_product_version("70000.0.0+80000"), "65535.0.0.65535");
    }

    /// The template's own example (`${NSISDIR}\languages\…`) points at a
    /// directory NSIS does not ship; the strings the template needs come from
    /// the pinned tag, which lives in pacor's cache.
    #[test]
    fn language_includes_point_into_the_cache() {
        let config = english_config();
        // Spelt out the same way `language_include` builds the path, so the
        // assertion holds on hosts that use a different separator.
        let expected = Path::new("/cache/windows/nsis/v1")
            .join("languages")
            .join("SimpChinese.nsh");

        assert_eq!(
            generator(&config, Path::new("/cache/windows/nsis/v1")).language_include("SimpChinese"),
            format!("!include \"{}\"", expected.display())
        );
    }

    #[test]
    fn nsis_files_are_fetched_from_the_pinned_tag() {
        let url = pacor_core::tools::template_url("windows/nsis/template.nsi");

        assert!(url.contains(pacor_core::tools::TEMPLATE_REF), "{url}");
        assert!(url.contains("/refs/tags/"), "{url}");
        assert!(!url.contains("/refs/heads/"), "{url}");
        assert!(url.ends_with("/windows/nsis/template.nsi"), "{url}");
    }

    #[test]
    fn plugin_and_language_files_share_the_same_pin() {
        for file in [
            "Plugins/nsProcess.nsh",
            "Plugins/nsProcess.dll",
            "languages/SimpChinese.nsh",
        ] {
            let url = pacor_core::tools::template_url(&format!("windows/nsis/{file}"));
            assert!(url.contains(pacor_core::tools::TEMPLATE_REF), "{url}");
            assert!(url.ends_with(&format!("/windows/nsis/{file}")), "{url}");
        }
    }

    /// Renders a stand-in for the pinned template that uses every placeholder
    /// the real one uses, so a missing replacement fails here rather than at
    /// `makensis` time.
    #[test]
    fn rendering_fills_every_placeholder_the_template_uses() {
        let app_config = bilingual_config();
        let generator = generator(&app_config, Path::new("/cache/nsis/v1"));

        let script = generator
            .render_nsis_script(
                "{{LANGUAGE_MACROS}}\n{{LANGUAGE_STRINGS}}\n{{LANGUAGE_INCLUDE}}\n\
                 !define APP_NAME \"{{APP_NAME}}\"\n\
                 !define APP_VERSION \"{{APP_VERSION}}\"\n\
                 !define APP_URL \"{{APP_URL}}\"\n\
                 !define APP_EXECUTABLE \"{{APP_EXECUTABLE}}\"\n\
                 !define APP_ICON \"{{APP_ICON_PATH}}\"\n\
                 !define BUILD_OUTPUT_DIR \"{{BUILD_OUTPUT_DIR}}\"\n\
                 !define INSTALLER_OUTPUT_PATH \"{{INSTALLER_OUTPUT_PATH}}\"\n\
                 {{VERSION_INFO}}",
            )
            .expect("every placeholder must be provided");

        assert!(script.contains("!define APP_NAME \"my_app\""), "{script}");
        assert!(
            script.contains("!define APP_VERSION \"1.0.0+1\""),
            "the user-visible version keeps the build number: {script}"
        );
        assert!(
            script.contains(&format!(
                "!define BUILD_OUTPUT_DIR \"{}\"",
                generator.build_output_dir.display()
            )),
            "{script}"
        );
        assert!(
            script.contains(&format!(
                "!define INSTALLER_OUTPUT_PATH \"{}\"",
                generator
                    .installer_output_dir
                    .join(generator.installer_name)
                    .display()
            )),
            "{script}"
        );
        assert!(
            script.contains("!insertmacro MUI_LANGUAGE \"English\"")
                && script.contains("!insertmacro MUI_LANGUAGE \"SimpChinese\""),
            "{script}"
        );
        // The default locale is included first so its strings exist earliest.
        let english = script.find("English.nsh").expect("english include");
        let chinese = script.find("SimpChinese.nsh").expect("chinese include");
        assert!(english < chinese, "{script}");
        assert!(
            script.contains("VIProductVersion \"1.0.0.1\""),
            "the product version must be numeric: {script}"
        );
        assert!(
            script.contains("LangString APP_DISPLAY_NAME ${LANG_SIMPCHINESE} \"我的应用\""),
            "{script}"
        );
        assert!(
            script.contains(
                "VIAddVersionKey /LANG=${LANG_ENGLISH} \"CompanyName\" \"Publisher Name\""
            ),
            "{script}"
        );
        assert!(!script.contains("{{"), "unreplaced placeholder: {script}");
    }

    /// Compiles the rendered script with a real `makensis`.
    ///
    /// Ignored by default because it needs a local NSIS toolchain; see
    /// [`require_nsis_toolchain`] and `docs/WINDOWS.md`.
    #[test]
    #[ignore = "requires an NSIS toolchain; set PACOR_NSIS_TOOLCHAIN_DIR"]
    fn the_rendered_script_compiles_with_makensis() {
        use std::process::Command;

        use crate::nsis::{install_plugin_dll, stage_plugin_include};

        let toolchain = require_nsis_toolchain();
        let makensis = toolchain.join("makensis.exe");

        let temp = tempfile::TempDir::new().unwrap();

        // A stand-in for the Flutter build output: the installer only needs
        // files it can embed.
        let build_dir = temp.path().join("build/windows/x64/runner/Release");
        fs::create_dir_all(build_dir.join("data")).unwrap();
        fs::write(build_dir.join("my_app.exe"), b"stub executable").unwrap();
        fs::write(build_dir.join("data/app.so"), b"stub library").unwrap();

        let installer_output_dir = temp.path().join("dist");
        fs::create_dir_all(&installer_output_dir).unwrap();

        let icon_path = temp.path().join("app_icon.ico");
        write_minimal_icon(&icon_path);

        // Both plugin files have to be prepared exactly as a build would: the
        // include next to the script, the DLL in the plugin directory.
        stage_plugin_include(&toolchain, temp.path()).unwrap();
        install_plugin_dll(&toolchain, &toolchain).unwrap();

        let config = bilingual_config();
        let generator = NSISGenerator {
            app_config: &config,
            app_name: "my_app",
            default_description: "A Flutter application",
            version: "1.0.0+1",
            executable_name: "my_app.exe",
            installer_name: "my_app-1.0.0+1-x64-setup.exe",
            default_legal_copyright: "Copyright (C) 2025 Example",
            nsis_install_dir: &toolchain,
            icon_path: &icon_path,
            build_output_dir: &build_dir,
            installer_output_dir: &installer_output_dir,
        };

        let script = generator.generate(temp.path()).unwrap();

        let output = Command::new(&makensis)
            .args(["/INPUTCHARSET", "UTF8", "/V3"])
            .arg(&script)
            .output()
            .expect("failed to run makensis");

        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            output.status.success(),
            "makensis failed with {}:\n{stdout}\n{}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );

        let installer = installer_output_dir.join("my_app-1.0.0+1-x64-setup.exe");
        let size = fs::metadata(&installer)
            .expect("the installer was not produced")
            .len();
        assert!(size > 0, "the installer is empty");
        assert!(
            stdout.contains("x86-unicode"),
            "expected a Unicode installer:\n{stdout}"
        );
    }
}
