//! Rendering of the pinned WiX templates.
//!
//! `main.wxs` is rendered once per build; the localization template is rendered
//! once per configured locale, because `light` cannot merge several localization
//! files that define the same identifiers (`LGHT0100`) and therefore produces one
//! MSI per locale.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};

use pacor_core::packaging::windows_version;
use pacor_core::utils::template::render_template;
use pacor_core::{config::AppConfig, config::WindowsConfig, i18n::Locale};

use crate::msi::harvest::xml_escape;

/// Texts the installer shows that pacor owns rather than the application.
///
/// WixUI localizes its own dialogs; these four are referenced from `main.wxs`
/// through the localization file.
struct UiStrings {
    feature_main: &'static str,
    uninstall_shortcut: &'static str,
    downgrade_error: &'static str,
}

/// Wording per supported locale, falling back to English.
fn ui_strings(locale: &Locale) -> UiStrings {
    match locale {
        Locale::ZhCn => UiStrings {
            feature_main: "应用程序文件",
            uninstall_shortcut: "卸载",
            downgrade_error: "已安装同一或更新版本，请先卸载后再安装。",
        },
        Locale::ZhTw => UiStrings {
            feature_main: "應用程式檔案",
            uninstall_shortcut: "解除安裝",
            downgrade_error: "已安裝同一或更新版本，請先解除安裝後再安裝。",
        },
        Locale::JaJp => UiStrings {
            feature_main: "アプリケーション ファイル",
            uninstall_shortcut: "アンインストール",
            downgrade_error: "同じか新しいバージョンが既にインストールされています。",
        },
        Locale::EnUs | Locale::EnGb => UiStrings {
            feature_main: "Application files",
            uninstall_shortcut: "Uninstall",
            downgrade_error: "A same or newer version is already installed.",
        },
    }
}

/// Everything `candle` and `light` need for one build.
pub(super) struct MsiScripts {
    /// Generated `payload.wxs`, identical for every locale.
    pub(super) payload: PathBuf,

    /// The sources that differ per locale, in the order the MSIs are produced.
    pub(super) locales: Vec<MsiLocaleScripts>,
}

/// Sources whose content depends on the locale.
///
/// `main.wxs` is regenerated per locale rather than shared, because
/// `Product/@Language` carries the locale's LCID and cannot be localized through
/// a `.wxl` file.
pub(super) struct MsiLocaleScripts {
    pub(super) locale: Locale,

    /// Rendered per-locale `main.wxs`.
    pub(super) main: PathBuf,

    /// Rendered localization file.
    pub(super) culture: PathBuf,
}

pub(super) struct MsiGenerator<'a> {
    pub(super) app_config: &'a AppConfig,
    pub(super) windows_config: &'a WindowsConfig,
    pub(super) app_name: &'a str,
    pub(super) version: &'a str,
    pub(super) executable_name: &'a str,
    pub(super) icon_path: &'a Path,
    pub(super) upgrade_code: &'a str,
    pub(super) wix_dir: &'a Path,
    pub(super) payload_component_group: &'a str,
}

impl MsiGenerator<'_> {
    /// Writes every source file the MSI build needs into `temp_dir`.
    pub(super) fn write_sources(
        &self,
        temp_dir: &Path,
        build_output_dir: &Path,
    ) -> Result<MsiScripts> {
        let payload = crate::msi::harvest::harvest(build_output_dir, self.upgrade_code)?;
        let payload_path = temp_dir.join("payload.wxs");
        std::fs::write(&payload_path, payload)
            .with_context(|| format!("failed to write {}", payload_path.display()))?;

        let mut locales = Vec::with_capacity(self.app_config.i18n().len());
        for locale in self.locales() {
            let culture_name = locale.as_wix_culture();

            let main = self.render_main_wxs(&locale)?;
            let main_path = temp_dir.join(format!("{culture_name}-main.wxs"));
            std::fs::write(&main_path, main)
                .with_context(|| format!("failed to write {}", main_path.display()))?;

            let culture = self.render_culture(&locale)?;
            let culture_path = temp_dir.join(format!("{culture_name}.wxl"));
            std::fs::write(&culture_path, culture)
                .with_context(|| format!("failed to write {}", culture_path.display()))?;

            locales.push(MsiLocaleScripts {
                locale,
                main: main_path,
                culture: culture_path,
            });
        }

        Ok(MsiScripts {
            payload: payload_path,
            locales,
        })
    }

    /// Locales to build MSIs for: the default one first, then the rest sorted,
    /// so the artifact list is stable.
    fn locales(&self) -> Vec<Locale> {
        let mut locales: Vec<Locale> = self.app_config.i18n().keys().cloned().collect();
        locales.sort();

        let default = self.app_config.default_locale().clone();
        locales.retain(|locale| locale != &default);
        locales.insert(0, default);
        locales
    }

    /// Fills `main.wxs` for one locale.
    ///
    /// Values are escaped: an application name may legitimately contain `&` or a
    /// path may contain a quote character.
    fn render_main_wxs(&self, locale: &Locale) -> Result<String> {
        let template = std::fs::read_to_string(self.wix_template("main.wxs")?)
            .context("failed to read the cached main.wxs template")?;

        let product_version = windows_version(self.version);
        let product_language = locale.as_windows_lcid().to_string();
        let app_name = xml_escape(self.app_name);
        let icon_path = xml_escape(&self.icon_path.display().to_string());
        let executable = xml_escape(self.executable_name);

        let install_dir_guid = self.component_guid("__install_dir__");
        let start_menu_guid = self.component_guid("__start_menu_shortcut__");
        let desktop_guid = self.component_guid("__desktop_shortcut__");

        let url = xml_escape(self.app_config.url());

        let replacements = HashMap::from([
            ("PRODUCT_VERSION", product_version.as_str()),
            ("PRODUCT_LANGUAGE", product_language.as_str()),
            ("UPGRADE_CODE", self.upgrade_code),
            ("APP_NAME", app_name.as_str()),
            ("APP_URL", url.as_str()),
            ("APP_EXECUTABLE", executable.as_str()),
            ("APP_ICON_PATH", icon_path.as_str()),
            ("PAYLOAD_COMPONENT_GROUP", self.payload_component_group),
            ("INSTALL_DIR_COMPONENT_GUID", install_dir_guid.as_str()),
            ("START_MENU_COMPONENT_GUID", start_menu_guid.as_str()),
            ("DESKTOP_COMPONENT_GUID", desktop_guid.as_str()),
        ]);

        render_template(&template, &replacements)
            .map_err(|error| anyhow::anyhow!("failed to render main.wxs for {locale}: {error}"))
    }

    /// Fills the localization template for one locale.
    fn render_culture(&self, locale: &Locale) -> Result<String> {
        let template = std::fs::read_to_string(self.wix_template("culture.wxl.tmpl")?)
            .context("failed to read the cached culture.wxl.tmpl template")?;

        let metadata = self.app_config.i18n().get(locale);
        let fallback = self.app_config.default_metadata();
        let ui = ui_strings(locale);

        let display_name = xml_escape(
            metadata
                .and_then(|metadata| metadata.display_name())
                .or_else(|| metadata.and_then(|metadata| metadata.name()))
                .unwrap_or(self.app_name),
        );
        let publisher = xml_escape(
            metadata
                .map(|metadata| metadata.publisher())
                .unwrap_or_else(|| fallback.map(|metadata| metadata.publisher()).unwrap_or("")),
        );
        let description = xml_escape(
            metadata
                .and_then(|metadata| metadata.description())
                .or_else(|| fallback.and_then(|metadata| metadata.description()))
                .unwrap_or(""),
        );
        let copyright = xml_escape(
            metadata
                .and_then(|metadata| metadata.copyright())
                .or_else(|| fallback.and_then(|metadata| metadata.copyright()))
                .unwrap_or(""),
        );
        let codepage = self
            .windows_config
            .resolved_msi_codepage(locale)
            .to_string();

        let replacements = HashMap::from([
            ("CULTURE", locale.as_wix_culture()),
            ("APP_DISPLAY_NAME", display_name.as_str()),
            ("APP_PUBLISHER", publisher.as_str()),
            ("APP_DESCRIPTION", description.as_str()),
            ("APP_COPYRIGHT", copyright.as_str()),
            ("MSI_CODEPAGE", codepage.as_str()),
            ("FEATURE_MAIN", ui.feature_main),
            ("UNINSTALL_SHORTCUT", ui.uninstall_shortcut),
            ("DOWNGRADE_ERROR", ui.downgrade_error),
        ]);

        render_template(&template, &replacements)
            .map_err(|error| anyhow::anyhow!("failed to render culture.wxl for {locale}: {error}"))
    }

    /// Component GUID for a component the template itself declares.
    ///
    /// Derived from the product identity for the same reason the payload GUIDs
    /// are: a stable id across releases, unique across products.
    fn component_guid(&self, role: &str) -> String {
        pacor_core::packaging::deterministic_uuid_v5(&format!("{}/{role}", self.upgrade_code))
    }

    fn wix_template(&self, name: &str) -> Result<PathBuf> {
        let path = crate::msi::toolchain::template_path(self.wix_dir, name);
        if !path.is_file() {
            anyhow::bail!(
                "the WiX template {} is missing; run the build again so it can be fetched",
                path.display()
            );
        }

        Ok(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::collections::HashMap;

    use pacor_core::config::{AppConfig, AppMetadata};

    fn config(locales: &[Locale]) -> AppConfig {
        AppConfig {
            default_locale: locales[0].clone(),
            i18n: locales
                .iter()
                .map(|locale| {
                    let (name, publisher) = match locale {
                        Locale::ZhCn => ("我的应用", "我的公司"),
                        _ => ("My App", "Publisher Name"),
                    };
                    (
                        locale.clone(),
                        AppMetadata::new(
                            Some(name),
                            Some(name),
                            Some("A Flutter application"),
                            publisher,
                            Some("Copyright (C) 2025"),
                        ),
                    )
                })
                .collect::<HashMap<_, _>>(),
            authors: None,
            url: "https://example.com".to_string(),
        }
    }

    fn templates(temp: &Path) {
        std::fs::write(
            temp.join("main.wxs"),
            "<Product Version=\"{{PRODUCT_VERSION}}\" UpgradeCode=\"{{UPGRADE_CODE}}\">\
             {{PAYLOAD_COMPONENT_GROUP}} {{APP_NAME}} {{APP_URL}} {{APP_EXECUTABLE}} \
             {{APP_ICON_PATH}} {{INSTALL_DIR_COMPONENT_GUID}} {{START_MENU_COMPONENT_GUID}} \
             {{DESKTOP_COMPONENT_GUID}}</Product>",
        )
        .unwrap();
        std::fs::write(
            temp.join("culture.wxl.tmpl"),
            "<WixLocalization Culture=\"{{CULTURE}}\">{{APP_DISPLAY_NAME}}|{{APP_PUBLISHER}}|\
             {{APP_DESCRIPTION}}|{{APP_COPYRIGHT}}|{{FEATURE_MAIN}}|{{UNINSTALL_SHORTCUT}}|\
             {{DOWNGRADE_ERROR}}</WixLocalization>",
        )
        .unwrap();
    }

    #[test]
    fn the_default_locale_comes_first_after_the_others_are_sorted() {
        let config = config(&[Locale::ZhCn, Locale::EnUs, Locale::JaJp]);
        let temp = tempfile::TempDir::new().unwrap();
        templates(temp.path());
        let generator = MsiGenerator {
            app_config: &config,
            windows_config: &WindowsConfig::default(),
            app_name: "my_app",
            version: "1.0.0+1",
            executable_name: "my_app.exe",
            icon_path: Path::new("/project/app_icon.ico"),
            upgrade_code: "{11111111-2222-3333-4444-555555555555}",
            wix_dir: temp.path(),
            payload_component_group: "AppPayload",
        };

        let locales: Vec<Locale> = generator.locales();

        assert_eq!(locales, vec![Locale::ZhCn, Locale::EnUs, Locale::JaJp]);
    }

    #[test]
    fn rendering_escapes_values_and_normalizes_the_version() {
        let config = config(&[Locale::EnUs]);
        let temp = tempfile::TempDir::new().unwrap();
        templates(temp.path());
        let generator = MsiGenerator {
            app_config: &config,
            windows_config: &WindowsConfig::default(),
            app_name: "my&app",
            version: "1.0.0+1",
            executable_name: "my&app.exe",
            icon_path: Path::new("/project/icon.ico"),
            upgrade_code: "{11111111-2222-3333-4444-555555555555}",
            wix_dir: temp.path(),
            payload_component_group: "AppPayload",
        };

        let rendered = generator.render_main_wxs(&Locale::EnUs).unwrap();

        assert!(rendered.contains("Version=\"1.0.0.1\""), "{rendered}");
        assert!(rendered.contains("my&amp;app"), "{rendered}");
        assert!(!rendered.contains("my&app\""), "{rendered}");
        assert!(!rendered.contains("{{"), "{rendered}");
    }

    #[test]
    fn every_locale_renders_its_own_culture_name_and_wording() {
        let config = config(&[Locale::EnUs, Locale::ZhCn]);
        let temp = tempfile::TempDir::new().unwrap();
        templates(temp.path());
        let generator = MsiGenerator {
            app_config: &config,
            windows_config: &WindowsConfig::default(),
            app_name: "my_app",
            version: "1.0.0",
            executable_name: "my_app.exe",
            icon_path: Path::new("/project/icon.ico"),
            upgrade_code: "{11111111-2222-3333-4444-555555555555}",
            wix_dir: temp.path(),
            payload_component_group: "AppPayload",
        };

        let english = generator.render_culture(&Locale::EnUs).unwrap();
        let chinese = generator.render_culture(&Locale::ZhCn).unwrap();

        assert!(english.contains("Culture=\"en-us\""), "{english}");
        assert!(english.contains("My App|Publisher Name"), "{english}");
        assert!(chinese.contains("Culture=\"zh-cn\""), "{chinese}");
        assert!(chinese.contains("我的应用|我的公司"), "{chinese}");
        assert!(!english.contains("{{") && !chinese.contains("{{"));
    }

    /// The template's own components need GUIDs too, and they must be neither
    /// duplicated nor dependent on the build directory.
    #[test]
    fn template_component_guids_are_distinct_and_stable() {
        let config = config(&[Locale::EnUs]);
        let temp = tempfile::TempDir::new().unwrap();
        templates(temp.path());
        let windows_config = WindowsConfig::default();
        let generator = |upgrade_code: &'static str| MsiGenerator {
            app_config: &config,
            windows_config: &windows_config,
            app_name: "my_app",
            version: "1.0.0",
            executable_name: "my_app.exe",
            icon_path: Path::new("/project/icon.ico"),
            upgrade_code,
            wix_dir: temp.path(),
            payload_component_group: "AppPayload",
        };

        let first = generator("{11111111-2222-3333-4444-555555555555}");
        let again = generator("{11111111-2222-3333-4444-555555555555}");
        let other = generator("{99999999-8888-7777-6666-555555555555}");

        assert_eq!(first.component_guid("a"), again.component_guid("a"));
        assert_ne!(first.component_guid("a"), other.component_guid("a"));
        assert_ne!(
            first.component_guid("a"),
            first.component_guid("b"),
            "roles must not collide"
        );
    }

    #[test]
    fn a_missing_template_is_reported_with_its_path() {
        let config = config(&[Locale::EnUs]);
        let temp = tempfile::TempDir::new().unwrap();
        let generator = MsiGenerator {
            app_config: &config,
            windows_config: &WindowsConfig::default(),
            app_name: "my_app",
            version: "1.0.0",
            executable_name: "my_app.exe",
            icon_path: Path::new("/project/icon.ico"),
            upgrade_code: "{11111111-2222-3333-4444-555555555555}",
            wix_dir: temp.path(),
            payload_component_group: "AppPayload",
        };

        let error = generator
            .render_main_wxs(&Locale::EnUs)
            .unwrap_err()
            .to_string();

        assert!(error.contains("main.wxs"), "{error}");
    }
}
