//! macOS localization: `InfoPlist.strings` rendering and installation into an
//! already-built `.app` bundle, plus the bundle metadata written into the same
//! `Info.plist`.

use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};

use pacor_core::config::{AppConfig, AppMetadata, MacOSBuildConfig};
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
    let plist_path = info_plist_path(app_bundle_path);
    let mut plist = read_info_plist(&plist_path)?;
    plist.insert("CFBundleLocalizations", localizations)?;
    plist.insert(
        "CFBundleDevelopmentRegion",
        app_config.default_locale().as_apple(),
    )?;
    write_info_plist(&plist_path, &plist)?;

    debug!(locales = locale_count, "Localization resources installed");
    Ok(())
}

/// Path of the built bundle's `Info.plist`.
fn info_plist_path(app_bundle_path: &Path) -> PathBuf {
    app_bundle_path.join("Contents").join("Info.plist")
}

/// Read the built `Info.plist`.
fn read_info_plist(path: &Path) -> Result<Plist> {
    let contents = fs::read_to_string(path).with_context(|| {
        format!(
            "failed to read {} (pacor edits an XML `Info.plist`; if the build wrote a binary \
             one, convert it first with `plutil -convert xml1 {}`)",
            path.display(),
            path.display()
        )
    })?;
    Ok(Plist::parse(&contents))
}

/// Write the built `Info.plist` back.
fn write_info_plist(path: &Path, plist: &Plist) -> Result<()> {
    fs::write(path, plist.to_string())
        .with_context(|| format!("failed to write {}", path.display()))
}

/// Write the bundle identity from `pacor.toml` into the built `Info.plist`.
///
/// Flutter's macOS template points `CFBundleIdentifier` at Xcode's
/// `PRODUCT_BUNDLE_IDENTIFIER` and derives `LSMinimumSystemVersion` from the deployment
/// target, so without this the matching `pacor.toml` settings were parsed and then ignored:
/// the app shipped with the identifier and the minimum version Flutter chose, and with no
/// App Store category at all. A key whose setting is absent is left exactly as the build
/// wrote it.
pub fn write_bundle_metadata(
    app_bundle_path: &Path,
    macos_config: &MacOSBuildConfig,
) -> Result<()> {
    let settings = [
        (
            "CFBundleIdentifier",
            macos_config.bundle_identifier.as_deref(),
        ),
        (
            "LSMinimumSystemVersion",
            macos_config.minimum_system_version.as_deref(),
        ),
        (
            "LSApplicationCategoryType",
            macos_config.category.as_deref(),
        ),
    ];
    if settings.iter().all(|(_, value)| value.is_none()) {
        return Ok(());
    }

    let plist_path = info_plist_path(app_bundle_path);
    let mut plist = read_info_plist(&plist_path)?;
    for (key, value) in settings {
        if let Some(value) = value {
            plist.insert(key, value)?;
        }
    }
    write_info_plist(&plist_path, &plist)?;

    debug!(bundle = %app_bundle_path.display(), "Bundle metadata written to Info.plist");
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

    #[test]
    fn the_configured_bundle_metadata_is_written_into_the_plist() {
        let temp = TempDir::new().unwrap();
        let bundle = bundle(&temp);
        let macos_config = MacOSBuildConfig {
            bundle_identifier: Some("com.example.demo".to_string()),
            minimum_system_version: Some("11.0".to_string()),
            category: Some("public.app-category.developer-tools".to_string()),
            ..Default::default()
        };

        write_bundle_metadata(&bundle, &macos_config).unwrap();

        let plist = fs::read_to_string(bundle.join("Contents/Info.plist")).unwrap();
        assert!(plist.contains("<key>CFBundleIdentifier</key>"), "{plist}");
        assert!(
            plist.contains("<string>com.example.demo</string>"),
            "{plist}"
        );
        assert!(
            plist.contains("<key>LSMinimumSystemVersion</key>"),
            "{plist}"
        );
        assert!(plist.contains("<string>11.0</string>"), "{plist}");
        assert!(
            plist.contains("<key>LSApplicationCategoryType</key>"),
            "{plist}"
        );
        assert!(
            plist.contains("<string>public.app-category.developer-tools</string>"),
            "{plist}"
        );
        // The keys the build already wrote are not this function's business.
        assert!(plist.contains("<key>CFBundleName</key>"), "{plist}");
    }

    /// `Default` sets the minimum system version and the category, so "nothing configured"
    /// has to be spelled out rather than taken from `Default`.
    #[test]
    fn a_config_without_bundle_metadata_leaves_the_plist_alone() {
        let temp = TempDir::new().unwrap();
        let bundle = bundle(&temp);
        let macos_config = MacOSBuildConfig {
            bundle_identifier: None,
            minimum_system_version: None,
            category: None,
            ..Default::default()
        };

        write_bundle_metadata(&bundle, &macos_config).unwrap();

        let plist = fs::read_to_string(bundle.join("Contents/Info.plist")).unwrap();
        assert_eq!(plist, INFO_PLIST, "nothing configured, nothing to write");
    }
}
