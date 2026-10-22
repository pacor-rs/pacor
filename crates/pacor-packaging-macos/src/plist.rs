//! Editing the built `Info.plist` without owning it.
//!
//! The plist inside a built `.app` is Xcode's output, not pacor's document: it holds
//! booleans, integers, nested dictionaries and arrays of things other than strings. Reading
//! it into the two value kinds pacor sets and writing that map back would silently delete
//! every entry this module does not model, so the file is kept as the text it was read from
//! and only the value element of a key pacor sets is replaced. A binary plist is not text at
//! all; it is reported rather than rewritten.

use std::fmt::Display;

use anyhow::{Result, anyhow};

use pacor_core::i18n::Locale;

/// A value pacor writes into `Info.plist`.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum PlistValue {
    String(String),
    Array(Vec<String>),
}

impl From<&str> for PlistValue {
    fn from(s: &str) -> Self {
        PlistValue::String(s.to_string())
    }
}

impl From<&Locale> for PlistValue {
    fn from(s: &Locale) -> Self {
        PlistValue::String(s.to_string())
    }
}

impl From<Vec<&str>> for PlistValue {
    fn from(s: Vec<&str>) -> Self {
        PlistValue::Array(s.iter().map(|s| s.to_string()).collect())
    }
}

impl PlistValue {
    /// The value's XML, indented for a plist written with four spaces per level.
    fn render(&self, indent: &str) -> String {
        match self {
            PlistValue::String(value) => format!("<string>{value}</string>"),
            PlistValue::Array(values) => {
                let items = values
                    .iter()
                    .map(|value| format!("{indent}    <string>{value}</string>"))
                    .collect::<Vec<_>>()
                    .join("\n");
                format!("<array>\n{items}\n{indent}</array>")
            }
        }
    }
}

/// An XML plist, kept as the text it was read from.
pub(super) struct Plist {
    contents: String,
}

impl Plist {
    pub(super) fn parse(input: &str) -> Self {
        Self {
            contents: input.to_string(),
        }
    }

    /// Sets `key` to `value`.
    ///
    /// An existing key keeps its position and only its value element is replaced; a new key
    /// is inserted before the closing `</dict>`. Nothing else in the file is touched, so the
    /// entries this module does not model survive.
    pub(super) fn insert<K: ToString, V: Into<PlistValue>>(
        &mut self,
        key: K,
        value: V,
    ) -> Result<()> {
        let key = key.to_string();
        let value = value.into();
        let key_element = format!("<key>{key}</key>");

        if let Some(key_start) = self.contents.find(&key_element) {
            let (start, end) = value_span(&self.contents, key_start + key_element.len())
                .map_err(|error| anyhow!("{error} (key `{key}`)"))?;
            let indent = indentation(&self.contents, start);
            let rendered = value.render(&indent);
            self.contents.replace_range(start..end, &rendered);
            return Ok(());
        }

        let closing = self.contents.rfind("</dict>").ok_or_else(|| {
            anyhow!("the Info.plist has no closing `</dict>` to insert `{key}` into")
        })?;
        let indent = indentation(&self.contents, closing);
        let rendered = value.render(&indent);
        let entry = format!("{indent}<key>{key}</key>\n{indent}{rendered}\n");
        self.contents.insert_str(closing, &entry);
        Ok(())
    }
}

impl Display for Plist {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.contents)
    }
}

/// The span of the value element that starts at or after `from`.
fn value_span(contents: &str, from: usize) -> Result<(usize, usize)> {
    let rest = &contents[from..];
    let start = from
        + rest
            .find('<')
            .ok_or_else(|| anyhow!("no value element follows the key"))?;
    let rest = &contents[start..];
    let tag_end = rest
        .find('>')
        .ok_or_else(|| anyhow!("the value element after the key is not closed"))?;
    let tag = &rest[..tag_end];
    if tag.ends_with('/') {
        // `<true/>`, `<string/>`: there is no closing tag to match.
        return Ok((start, start + tag_end + 1));
    }

    let name = tag
        .trim_start_matches('<')
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_string();
    let open = format!("<{name}");
    let close = format!("</{name}>");

    let mut depth = 1usize;
    let mut cursor = start + tag_end + 1;
    loop {
        let next_close = contents[cursor..]
            .find(&close)
            .map(|offset| cursor + offset)
            .ok_or_else(|| anyhow!("the `<{name}>` element is never closed"))?;
        match contents[cursor..]
            .find(&open)
            .map(|offset| cursor + offset)
            .filter(|open_at| *open_at < next_close)
        {
            Some(open_at) => {
                let after = contents[open_at + open.len()..].chars().next();
                if after.is_some_and(|c| c == '>' || c == '/' || c.is_whitespace()) {
                    depth += 1;
                }
                cursor = open_at + open.len();
            }
            None => {
                depth -= 1;
                if depth == 0 {
                    return Ok((start, next_close + close.len()));
                }
                cursor = next_close + close.len();
            }
        }
    }
}

/// The indentation of the line `at` falls on.
fn indentation(contents: &str, at: usize) -> String {
    let line_start = contents[..at].rfind('\n').map_or(0, |index| index + 1);
    contents[line_start..at]
        .chars()
        .take_while(|character| character.is_whitespace())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Shaped after what Xcode writes into a built `.app`: the value kinds pacor does not
    /// model are the point of the fixture.
    const BUILT_INFO_PLIST: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleDevelopmentRegion</key>
    <string>en</string>
    <key>NSHighResolutionCapable</key>
    <true/>
    <key>NSAppTransportSecurity</key>
    <dict>
        <key>NSAllowsArbitraryLoads</key>
        <false/>
    </dict>
    <key>CFBundleSupportedPlatforms</key>
    <array>
        <string>MacOSX</string>
    </array>
</dict>
</plist>
"#;

    #[test]
    fn an_existing_key_is_replaced_in_place() {
        let mut plist = Plist::parse(BUILT_INFO_PLIST);

        plist
            .insert("CFBundleDevelopmentRegion", PlistValue::from("zh-Hans"))
            .unwrap();

        let text = plist.to_string();
        assert!(text.contains("<string>zh-Hans</string>"), "{text}");
        assert!(!text.contains("<string>en</string>"), "{text}");
        assert!(
            text.find("CFBundleDevelopmentRegion").unwrap()
                < text.find("NSHighResolutionCapable").unwrap(),
            "the key keeps its position: {text}"
        );
    }

    #[test]
    fn a_new_key_is_inserted_before_the_closing_dict() {
        let mut plist = Plist::parse(BUILT_INFO_PLIST);

        plist
            .insert(
                "CFBundleLocalizations",
                PlistValue::from(vec!["en", "zh-Hans"]),
            )
            .unwrap();

        let text = plist.to_string();
        assert!(text.contains("<key>CFBundleLocalizations</key>"), "{text}");
        assert!(text.contains("<string>zh-Hans</string>"), "{text}");
        assert!(
            text.find("CFBundleLocalizations").unwrap() < text.rfind("</dict>").unwrap(),
            "{text}"
        );
        assert!(
            text.ends_with(
                "</plist>
"
            ),
            "{text}"
        );
    }

    /// The regression this module exists for: rewriting the plist must not delete the entries
    /// pacor does not model.
    #[test]
    fn values_pacor_does_not_model_survive_the_rewrite() {
        let mut plist = Plist::parse(BUILT_INFO_PLIST);

        plist
            .insert("CFBundleLocalizations", PlistValue::from(vec!["en"]))
            .unwrap();
        plist
            .insert("CFBundleDevelopmentRegion", PlistValue::from("zh-Hans"))
            .unwrap();

        let text = plist.to_string();
        for untouched in [
            "<true/>",
            "<false/>",
            "<key>NSAppTransportSecurity</key>",
            "<key>NSAllowsArbitraryLoads</key>",
            "<key>CFBundleSupportedPlatforms</key>",
            "<string>MacOSX</string>",
        ] {
            assert!(
                text.contains(untouched),
                "`{untouched}` was dropped: {text}"
            );
        }
    }

    #[test]
    fn a_self_closing_value_is_replaced_whole() {
        let mut plist = Plist::parse(BUILT_INFO_PLIST);

        plist
            .insert("NSHighResolutionCapable", PlistValue::from("yes"))
            .unwrap();

        let text = plist.to_string();
        assert!(text.contains("<string>yes</string>"), "{text}");
        assert!(!text.contains("<true/>"), "{text}");
    }

    #[test]
    fn a_plist_without_a_dict_is_reported() {
        let mut plist = Plist::parse("<plist version=\"1.0\"></plist>");

        let error = plist
            .insert("CFBundleLocalizations", PlistValue::from(vec!["en"]))
            .unwrap_err()
            .to_string();

        assert!(error.contains("</dict>"), "{error}");
    }
}
