//! Fixtures shared by the Windows backend's tests.
//!
//! Everything here is `#[cfg(test)]`: it keeps the metadata, the Flutter project
//! and the icon used by several test modules in one place instead of one copy
//! per module.

use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    sync::OnceLock,
};

use pacor_core::{
    config::{AppConfig, AppMetadata},
    core::project::FlutterProject,
    i18n::Locale,
};

/// Environment variable pointing at a prepared NSIS toolchain directory.
pub(crate) const NSIS_TOOLCHAIN_ENV: &str = "PACOR_NSIS_TOOLCHAIN_DIR";

/// Environment variable pointing at a prepared WiX toolset directory.
pub(crate) const WIX_TOOLSET_ENV: &str = "PACOR_MSI_TOOLCHAIN_DIR";

/// Metadata for `locale`, with locally distinct values so that generated
/// content can be told apart.
fn metadata(locale: &Locale) -> AppMetadata {
    match locale {
        Locale::ZhCn => AppMetadata::new(
            Some("我的应用"),
            Some("我的应用"),
            Some("一个 Flutter 应用"),
            "我的公司",
            Some("版权所有 © 2025 我的公司。"),
        ),
        Locale::ZhTw => AppMetadata::new(
            Some("我的應用"),
            Some("我的應用"),
            Some("一個 Flutter 應用"),
            "我的公司",
            Some("版權所有 © 2025 我的公司。"),
        ),
        Locale::JaJp => AppMetadata::new(
            Some("マイアプリ"),
            Some("マイアプリ"),
            Some("Flutter アプリ"),
            "発行者",
            Some("Copyright (C) 2025 発行者"),
        ),
        Locale::EnUs | Locale::EnGb => AppMetadata::new(
            Some("My App"),
            Some("My App"),
            Some("A Flutter application"),
            "Publisher Name",
            Some("Copyright (C) 2025 Publisher Name."),
        ),
    }
}

/// Configuration with one entry per locale; the first one is the default.
pub(crate) fn app_config(locales: &[Locale]) -> AppConfig {
    AppConfig {
        default_locale: locales[0].clone(),
        i18n: locales
            .iter()
            .map(|locale| (locale.clone(), metadata(locale)))
            .collect::<HashMap<_, _>>(),
        authors: Some(HashSet::new()),
        url: "https://example.com".to_string(),
    }
}

/// The configuration most tests use: English plus Simplified Chinese.
pub(crate) fn bilingual_config() -> AppConfig {
    app_config(&[Locale::EnUs, Locale::ZhCn])
}

/// A loaded Flutter project, from a generated `pubspec.yaml`.
pub(crate) fn demo_project() -> &'static FlutterProject {
    static PROJECT: OnceLock<FlutterProject> = OnceLock::new();

    PROJECT.get_or_init(|| {
        let temp = tempfile::TempDir::new().unwrap();
        fs::write(
            temp.path().join("pubspec.yaml"),
            "name: my_app\nversion: 1.0.0+1\ndescription: A Flutter application\n",
        )
        .unwrap();

        FlutterProject::load_from_pubspec(temp.path()).unwrap()
    })
}

/// Writes the smallest icon `makensis` accepts: a 16×16 32bpp bitmap.
pub(crate) fn write_minimal_icon(path: &Path) {
    // BITMAPINFOHEADER: 16×16, 32bpp; the height counts the AND mask too.
    let mut image = vec![
        40, 0, 0, 0, // header size
        16, 0, 0, 0, // width
        32, 0, 0, 0, // height (16 pixels + 16 mask rows)
        1, 0, // planes
        32, 0, // bits per pixel
        0, 0, 0, 0, // compression
        0, 4, 0, 0, // image size (16*16*4 + 16*4)
        0, 0, 0, 0, // x pixels per meter
        0, 0, 0, 0, // y pixels per meter
        0, 0, 0, 0, // colours used
        0, 0, 0, 0, // important colours
    ];
    image.extend(std::iter::repeat_n(0u8, 16 * 16 * 4 + 16 * 4));

    let mut icon = vec![0, 0, 1, 0, 1, 0, 16, 16, 0, 0, 1, 0, 32, 0];
    icon.extend_from_slice(&(image.len() as u32).to_le_bytes());
    icon.extend_from_slice(&22u32.to_le_bytes());
    icon.extend_from_slice(&[0u8; 16]);
    icon.extend_from_slice(&image);

    fs::write(path, icon).unwrap();
}

/// The NSIS toolchain directory an ignored test was pointed at.
///
/// See `docs/WINDOWS.md` for the layout it has to have.
pub(crate) fn require_nsis_toolchain() -> PathBuf {
    let directory = std::env::var_os(NSIS_TOOLCHAIN_ENV)
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            panic!("set {NSIS_TOOLCHAIN_ENV} to a prepared NSIS directory (see docs/WINDOWS.md)")
        });

    assert!(
        directory.join("makensis.exe").is_file(),
        "{} does not contain makensis.exe",
        directory.display()
    );
    assert!(
        directory.join("template.nsi").is_file(),
        "{} does not contain the pinned template.nsi",
        directory.display()
    );

    directory
}

/// The WiX directory an ignored test was pointed at.
///
/// Unlike the NSIS one, this directory also holds the WiX *templates* of the
/// pinned tag, because the toolset is downloaded from the WiX release and the
/// templates from `pacor-rs/tools` (see `docs/MSI.md`).
pub(crate) fn require_wix_toolset() -> PathBuf {
    let directory = std::env::var_os(WIX_TOOLSET_ENV)
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            panic!("set {WIX_TOOLSET_ENV} to a prepared WiX directory (see docs/MSI.md)")
        });

    for file in ["candle.exe", "light.exe", "WixUIExtension.dll", "main.wxs"] {
        assert!(
            directory.join(file).is_file(),
            "{} does not contain {file}",
            directory.display()
        );
    }

    directory
}
