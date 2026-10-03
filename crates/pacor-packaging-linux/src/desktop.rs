//! Generation of the freedesktop `.desktop` entry.

use pacor_core::config::AppConfig;
use pacor_core::core::freedesktop::FreeDesktop;

/// Build the desktop entry for `executable_name`.
///
/// `base` is the configured [`FreeDesktop`] value, so user-provided categories
/// and MIME types are preserved. Localized names/comments are derived from the
/// application's i18n metadata; the default locale is skipped because it is
/// already the untagged `Name`/`Comment`.
pub(crate) fn create_desktop_entry(
    app_config: &AppConfig,
    executable_name: &str,
    base: FreeDesktop,
) -> FreeDesktop {
    let mut desktop = base;

    let display_name = app_config
        .display_name()
        .or(app_config.name())
        .unwrap_or(executable_name);

    desktop.set_name(display_name);
    desktop.set_icon(executable_name);
    desktop.set_exec(executable_name);
    desktop.set_startup_wm_class(executable_name);

    if let Some(description) = app_config.description() {
        desktop.set_comment(description);
    }

    let default_locale = app_config.default_locale();
    for (locale, metadata) in app_config.i18n() {
        if locale == default_locale {
            continue;
        }

        let locale = locale.as_posix();
        if let Some(name) = metadata.display_name().or(metadata.name()) {
            desktop.set_localized_name(locale, name);
        }
        if let Some(description) = metadata.description() {
            desktop.set_localized_comment(locale, description);
        }
    }

    desktop
}

#[cfg(test)]
mod tests {
    use super::*;
    use pacor_core::config::AppMetadata;
    use pacor_core::i18n::Locale;
    use std::collections::{HashMap, HashSet};

    fn app_config() -> AppConfig {
        AppConfig {
            default_locale: Locale::EnUs,
            i18n: HashMap::from([
                (
                    Locale::EnUs,
                    AppMetadata::new(
                        "demo-bin",
                        "Demo",
                        "An English description",
                        "Publisher",
                        "Copyright",
                    ),
                ),
                (
                    Locale::ZhCn,
                    AppMetadata::new("演示", "演示", "中文描述", "发布者", "版权"),
                ),
            ]),
            authors: Some(HashSet::new()),
            url: "https://example.com".to_string(),
        }
    }

    #[test]
    fn entry_uses_the_display_name_and_executable() {
        let desktop = create_desktop_entry(&app_config(), "demo-bin", FreeDesktop::default());
        let rendered = desktop.to_string();

        assert!(rendered.contains("Name=Demo"));
        assert!(rendered.contains("Icon=demo-bin"));
        assert!(rendered.contains("Exec=demo-bin"));
        assert!(rendered.contains("Comment=An English description"));
    }

    #[test]
    fn non_default_locales_are_added_as_localized_keys() {
        let desktop = create_desktop_entry(&app_config(), "demo-bin", FreeDesktop::default());
        let rendered = desktop.to_string();

        assert!(rendered.contains("Name[zh_CN]=演示"));
        assert!(rendered.contains("Comment[zh_CN]=中文描述"));
        // The default locale must not be duplicated as a localized key.
        assert!(!rendered.contains("Name[en_US]"));
    }

    #[test]
    fn configured_categories_and_mime_types_are_preserved() {
        let base: FreeDesktop = toml::from_str(
            "categories = [{ Main = \"Utility\" }]\nmime_type = \"x-scheme-handler/demo\"",
        )
        .unwrap();

        let desktop = create_desktop_entry(&app_config(), "demo-bin", base);
        let rendered = desktop.to_string();

        assert!(rendered.contains("Categories=Utility"));
        assert!(rendered.contains("MimeType=x-scheme-handler/demo"));
    }

    #[test]
    fn the_appimage_exec_override_keeps_the_window_class() {
        let mut desktop = create_desktop_entry(&app_config(), "demo-bin", FreeDesktop::default());
        // What the AppImage adapter does: launch through AppRun.
        desktop.set_exec("AppRun");

        let rendered = desktop.to_string();
        assert!(rendered.contains("Exec=AppRun"));
        assert!(
            rendered.contains("StartupWMClass=demo-bin"),
            "StartupWMClass must stay the Flutter window class, not AppRun: {rendered}"
        );
    }

    #[test]
    fn the_default_entry_has_a_category() {
        let desktop = create_desktop_entry(&app_config(), "demo-bin", FreeDesktop::default());
        assert!(desktop.to_string().contains("Categories=Utility"));
    }

    #[test]
    fn falls_back_to_the_executable_name_when_no_metadata_is_set() {
        let mut config = app_config();
        config.i18n = HashMap::from([(
            Locale::EnUs,
            AppMetadata::new(None, None, None, "Publisher", None),
        )]);

        let desktop = create_desktop_entry(&config, "demo-bin", FreeDesktop::default());

        assert!(desktop.to_string().contains("Name=demo-bin"));
    }
}
