use std::fmt::{Display, Formatter, Result};

use serde::{Deserialize, Serialize};

/// Locale identifier
#[derive(Debug, Default, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
pub enum Locale {
    #[default]
    #[serde(rename = "en-US")]
    EnUs,
    #[serde(rename = "en-GB")]
    EnGb,
    #[serde(rename = "zh-CN")]
    ZhCn,
    #[serde(rename = "zh-TW")]
    ZhTw,
    #[serde(rename = "ja-JP")]
    JaJp,
}

impl Locale {
    /// Canonical BCP-47 style tag, e.g. `en-US`.
    ///
    /// `pacor.toml` uses this spelling, and so does [`Display`].
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::EnUs => "en-US",
            Self::EnGb => "en-GB",
            Self::ZhCn => "zh-CN",
            Self::ZhTw => "zh-TW",
            Self::JaJp => "ja-JP",
        }
    }

    /// Apple *language ID*, e.g. `zh-Hans`.
    ///
    /// Apple names `.lproj` resource folders, `CFBundleLocalizations` entries
    /// and `CFBundleDevelopmentRegion` with language IDs: a language designator
    /// plus an optional region (`en-GB`) or **script** (`zh-Hans`) designator.
    /// Chinese must use the script form — `zh-CN`/`zh-TW` are not recognised
    /// language IDs, so those bundles are silently ignored.
    ///
    /// Reference: <https://developer.apple.com/library/archive/documentation/MacOSX/Conceptual/BPInternational/LanguageandLocaleIDs/LanguageandLocaleIDs.html>
    pub fn as_apple(&self) -> &'static str {
        match self {
            Self::EnUs => "en",
            Self::EnGb => "en-GB",
            Self::ZhCn => "zh-Hans",
            Self::ZhTw => "zh-Hant",
            Self::JaJp => "ja",
        }
    }

    /// POSIX locale name, e.g. `zh_CN`.
    ///
    /// The desktop entry specification requires `Name[lang_COUNTRY]` /
    /// `Comment[lang_COUNTRY]` postfixes to use an underscore, and desktop
    /// environments match them against `LC_MESSAGES` (e.g. `zh_CN.UTF-8`).
    /// Emitting the BCP-47 tag here would silently disable localisation.
    ///
    /// Reference: <https://specifications.freedesktop.org/desktop-entry/1.3/localized-keys.html>
    pub fn as_posix(&self) -> &'static str {
        match self {
            Self::EnUs => "en_US",
            Self::EnGb => "en_GB",
            Self::ZhCn => "zh_CN",
            Self::ZhTw => "zh_TW",
            Self::JaJp => "ja_JP",
        }
    }
}

impl Display for Locale {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        write!(f, "{}", self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[test]
    fn test_locale_default() {
        assert_eq!(Locale::default(), Locale::EnUs);
    }

    #[test]
    fn test_locale_display() {
        assert_eq!(format!("{}", Locale::EnUs), "en-US");
        assert_eq!(format!("{}", Locale::ZhCn), "zh-CN");
        assert_eq!(format!("{}", Locale::ZhTw), "zh-TW");
        assert_eq!(format!("{}", Locale::JaJp), "ja-JP");
    }

    #[test]
    fn test_locale_posix_name_uses_an_underscore() {
        assert_eq!(Locale::EnUs.as_posix(), "en_US");
        assert_eq!(Locale::EnGb.as_posix(), "en_GB");
        assert_eq!(Locale::ZhCn.as_posix(), "zh_CN");
        assert_eq!(Locale::ZhTw.as_posix(), "zh_TW");
        assert_eq!(Locale::JaJp.as_posix(), "ja_JP");

        // The two spellings must never be confused: the desktop entry spec
        // matches `lang_COUNTRY`, while `pacor.toml` uses BCP-47.
        for locale in [
            Locale::EnUs,
            Locale::EnGb,
            Locale::ZhCn,
            Locale::ZhTw,
            Locale::JaJp,
        ] {
            assert!(!locale.as_posix().contains('-'), "{}", locale.as_posix());
            assert!(!locale.as_str().contains('_'), "{}", locale.as_str());
        }
    }

    #[test]
    fn test_locale_apple_language_ids() {
        // Apple's language IDs: plain designator for common English/Japanese,
        // region designator for dialects, *script* designator for Chinese.
        assert_eq!(Locale::EnUs.as_apple(), "en");
        assert_eq!(Locale::EnGb.as_apple(), "en-GB");
        assert_eq!(Locale::ZhCn.as_apple(), "zh-Hans");
        assert_eq!(Locale::ZhTw.as_apple(), "zh-Hant");
        assert_eq!(Locale::JaJp.as_apple(), "ja");

        // Language IDs never use the POSIX underscore form.
        for locale in [
            Locale::EnUs,
            Locale::EnGb,
            Locale::ZhCn,
            Locale::ZhTw,
            Locale::JaJp,
        ] {
            assert!(!locale.as_apple().contains('_'), "{}", locale.as_apple());
        }
    }

    #[test]
    fn the_three_spellings_are_distinct_where_it_matters() {
        // Each consumer needs a different spelling; mixing them up silently
        // disables localisation on the respective platform.
        assert_eq!(Locale::ZhCn.as_str(), "zh-CN"); // pacor.toml / display
        assert_eq!(Locale::ZhCn.as_posix(), "zh_CN"); // desktop entry
        assert_eq!(Locale::ZhCn.as_apple(), "zh-Hans"); // .lproj
    }

    #[test]
    fn test_locale_serde_rename() {
        #[derive(Deserialize)]
        struct Wrapper {
            locale: Locale,
        }
        let w: Wrapper = toml::from_str("locale = \"zh-CN\"").unwrap();
        assert_eq!(w.locale, Locale::ZhCn);
        let w: Wrapper = toml::from_str("locale = \"en-US\"").unwrap();
        assert_eq!(w.locale, Locale::EnUs);
        let w: Wrapper = toml::from_str("locale = \"ja-JP\"").unwrap();
        assert_eq!(w.locale, Locale::JaJp);
    }

    #[test]
    fn test_locale_eq_and_hash() {
        use std::collections::HashSet;
        let set: HashSet<Locale> = [Locale::EnUs, Locale::ZhCn, Locale::EnUs]
            .into_iter()
            .collect();
        assert_eq!(set.len(), 2);
    }

    #[test]
    fn test_locale_ordering() {
        assert!(Locale::EnUs < Locale::ZhCn);
    }
}
