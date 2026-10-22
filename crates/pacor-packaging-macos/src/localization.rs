//! macOS localization: `InfoPlist.strings` rendering and installation into an
//! already-built `.app` bundle.

use std::{fs, path::Path};

use anyhow::{Context, Result};

use pacor_core::config::{AppConfig, AppMetadata};
use pacor_core::utils::fs::ensure_directory_exists;

use crate::plist::Plist;

/// Render an `InfoPlist.strings` body for a single locale.
///
/// Reference:
/// <https://developer.apple.com/library/archive/documentation/General/Reference/InfoPlistKeyReference/Articles/AboutInformationPropertyListFiles.html>
pub fn render_info_plist_strings(metadata: &AppMetadata) -> String {
    let mut output = String::new();

    if let Some(name) = metadata.name() {
        output.push_str(&format!("CFBundleName = \"{}\";\n", escape(name)));
    }
    if let Some(display_name) = metadata.display_name() {
        output.push_str(&format!(
            "CFBundleDisplayName = \"{}\";\n",
            escape(display_name)
        ));
    }
    if let Some(copyright) = metadata.copyright() {
        output.push_str(&format!(
            "NSHumanReadableCopyright = \"{}\";\n",
            escape(copyright)
        ));
    }
    if let Some(description) = metadata.description() {
        output.push_str(&format!(
            "CFBundleGetInfoString = \"{}\";\n",
            escape(description)
        ));
    }

    output
}

/// Install localization resources into a built `.app` bundle.
///
/// Writes one `<language-id>.lproj/InfoPlist.strings` per configured locale and
/// records the supported locales in `Info.plist`. Locales are processed in a
/// stable order so the produced bundle is reproducible.
///
/// Folder names and `CFBundleLocalizations` use Apple *language IDs*
/// (`zh-Hans`, `en-GB`, …), not the BCP-47 tags used in `pacor.toml`; see
/// [`pacor_core::i18n::Locale::as_apple`].
pub fn add_localization_resources(app_bundle_path: &Path, app_config: &AppConfig) -> Result<()> {
    let contents_dir = app_bundle_path.join("Contents");
    let resources_dir = contents_dir.join("Resources");

    let mut locales: Vec<_> = app_config.i18n().iter().collect();
    locales.sort_by_key(|(locale, _)| (*locale).clone());

    let mut localizations = Vec::with_capacity(locales.len());
    for (locale, metadata) in locales {
        let lproj_dir = resources_dir.join(format!("{}.lproj", locale.as_apple()));
        ensure_directory_exists(&lproj_dir)?;
        fs::write(
            lproj_dir.join("InfoPlist.strings"),
            render_info_plist_strings(metadata),
        )?;
        localizations.push(locale.as_apple());
    }

    let locale_count = localizations.len();
    let plist_path = contents_dir.join("Info.plist");
    let contents = fs::read_to_string(&plist_path).with_context(|| {
        format!(
            "failed to read {} (pacor edits an XML `Info.plist`; if the build wrote a binary \
             one, convert it first with `plutil -convert xml1 {}`)",
            plist_path.display(),
            plist_path.display()
        )
    })?;
    let mut plist = Plist::parse(&contents);
    plist.insert("CFBundleLocalizations", localizations)?;
    plist.insert(
        "CFBundleDevelopmentRegion",
        app_config.default_locale().as_apple(),
    )?;
    fs::write(&plist_path, plist.to_string())
        .with_context(|| format!("failed to write {}", plist_path.display()))?;

    debug!(locales = locale_count, "Localization resources installed");
    Ok(())
}

/// Escape the characters that would terminate an Objective-C string literal.
fn escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use super::*;
    use pacor_core::i18n::Locale;
    use std::collections::{HashMap, HashSet};
    use tempfile::TempDir;

    const INFO_PLIST: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key>
    <string>Demo</string>
</dict>
</plist>
"#;

    fn app_config() -> AppConfig {
        AppConfig {
            default_locale: Locale::EnUs,
            i18n: HashMap::from([
                (
                    Locale::EnUs,
                    AppMetadata::new(Some("Demo"), Some("Demo"), None, "Publisher", None),
                ),
                (
                    Locale::ZhCn,
                    AppMetadata::new(Some("演示"), Some("演示"), None, "发布者", None),
                ),
                (
                    Locale::ZhTw,
                    AppMetadata::new(Some("演示正"), Some("演示正"), None, "發布者", None),
                ),
            ]),
            authors: Some(HashSet::new()),
            url: "https://example.com".to_string(),
        }
    }

    fn bundle(temp: &TempDir) -> std::path::PathBuf {
        let bundle = temp.path().join("Demo.app");
        let contents = bundle.join("Contents");
        fs::create_dir_all(&contents).unwrap();
        fs::write(contents.join("Info.plist"), INFO_PLIST).unwrap();
        bundle
    }

    #[test]
    fn lproj_directories_use_apple_language_ids() {
        let temp = TempDir::new().unwrap();
        let bundle = bundle(&temp);

        add_localization_resources(&bundle, &app_config()).unwrap();

        let resources = bundle.join("Contents/Resources");
        // `zh-CN` / `zh-TW` are not Apple language IDs; the script forms are.
        assert!(resources.join("zh-Hans.lproj/InfoPlist.strings").exists());
        assert!(resources.join("zh-Hant.lproj/InfoPlist.strings").exists());
        assert!(resources.join("en.lproj/InfoPlist.strings").exists());
        assert!(!resources.join("zh-CN.lproj").exists());
        assert!(!resources.join("en-US.lproj").exists());
    }

    #[test]
    fn plist_records_language_ids_and_the_development_region() {
        let temp = TempDir::new().unwrap();
        let bundle = bundle(&temp);

        add_localization_resources(&bundle, &app_config()).unwrap();

        let plist = fs::read_to_string(bundle.join("Contents/Info.plist")).unwrap();
        // Everything must use language IDs, otherwise macOS ignores them.
        assert!(plist.contains("zh-Hans"), "{plist}");
        assert!(plist.contains("zh-Hant"), "{plist}");
        assert!(!plist.contains("zh-CN"), "{plist}");
        assert!(plist.contains("CFBundleDevelopmentRegion"), "{plist}");
        assert!(plist.contains("CFBundleLocalizations"), "{plist}");
        // The pre-existing key must survive the rewrite.
        assert!(plist.contains("CFBundleName"), "{plist}");
    }

    #[test]
    fn strings_files_carry_the_localized_values() {
        let temp = TempDir::new().unwrap();
        let bundle = bundle(&temp);

        add_localization_resources(&bundle, &app_config()).unwrap();

        let strings =
            fs::read_to_string(bundle.join("Contents/Resources/zh-Hans.lproj/InfoPlist.strings"))
                .unwrap();
        assert!(strings.contains("CFBundleName = \"演示\";"), "{strings}");
    }
}
