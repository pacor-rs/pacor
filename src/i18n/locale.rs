use std::fmt::{Display, Formatter, Result};

use serde::{Deserialize, Serialize};

/// Locale identifier
#[derive(Debug, Default, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
pub(crate) enum Locale {
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

impl Display for Locale {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        let s = match self {
            Locale::EnUs => "en-US",
            Locale::EnGb => "en-GB",
            Locale::ZhCn => "zh-CN",
            Locale::ZhTw => "zh-TW",
            Locale::JaJp => "ja-JP",
        };

        write!(f, "{}", s)
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
        let set: HashSet<Locale> = [Locale::EnUs, Locale::ZhCn, Locale::EnUs].into_iter().collect();
        assert_eq!(set.len(), 2);
    }

    #[test]
    fn test_locale_ordering() {
        assert!(Locale::EnUs < Locale::ZhCn);
    }
}
