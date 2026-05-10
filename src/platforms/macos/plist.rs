use std::{collections::BTreeMap, fmt::Display};

use crate::i18n::Locale;

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

// NOTE: BTreeMap 使用字典序排列，因此最终的key顺序可能与原文件不一致
pub(super) struct Plist(BTreeMap<String, PlistValue>);

impl Plist {
    pub(super) fn parse(input: &str) -> Self {
        let mut result = BTreeMap::new();
        let mut lines = input.lines();
        let mut current_key: Option<String> = None;

        while let Some(line) = lines.next() {
            let trimmed = line.trim();

            // 检查是否是键
            if trimmed.starts_with("<key>") && trimmed.ends_with("</key>") {
                let key = trimmed
                    .trim_start_matches("<key>")
                    .trim_end_matches("</key>")
                    .to_string();
                current_key = Some(key);
            }
            // 检查是否是字符串值
            else if trimmed.starts_with("<string>") && trimmed.ends_with("</string>") {
                if let Some(key) = current_key.take() {
                    let value = trimmed
                        .trim_start_matches("<string>")
                        .trim_end_matches("</string>")
                        .to_string();
                    result.insert(key, PlistValue::String(value));
                }
            }
            // 检查是否是数组开始
            else if trimmed == "<array>"
                && let Some(key) = current_key.take()
            {
                let mut array_values = Vec::new();
                for line in lines.by_ref() {
                    let trimmed = line.trim();
                    if trimmed == "</array>" {
                        break;
                    } else if trimmed.starts_with("<string>") && trimmed.ends_with("</string>") {
                        let value = trimmed
                            .trim_start_matches("<string>")
                            .trim_end_matches("</string>")
                            .to_string();
                        array_values.push(value);
                    }
                }
                result.insert(key, PlistValue::Array(array_values));
            }
        }

        Plist(result)
    }

    /// Inserts a key-value pair into the map.
    ///
    /// If the map did not have this key present, None is returned.
    ///
    /// If the map did have this key present, the value is updated, and the old value is returned.
    pub(super) fn insert<K: ToString, V: Into<PlistValue>>(
        &mut self,
        key: K,
        value: V,
    ) -> Option<PlistValue> {
        self.0.insert(key.to_string(), value.into())
    }
}

impl Display for Plist {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
    <dict>
"#
        )?;
        for (key, value) in &self.0 {
            writeln!(f, "        <key>{}</key>", key)?;
            match value {
                PlistValue::String(s) => writeln!(f, "        <string>{}</string>", s)?,
                PlistValue::Array(a) => {
                    writeln!(f, "        <array>")?;
                    for item in a {
                        writeln!(f, "            <string>{}</string>", item)?;
                    }
                    writeln!(f, "        </array>")?;
                }
            }
        }
        write!(f, "    </dict>\n</plist>")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_plist() {
        let plist_content = r#"
        <?xml version="1.0" encoding="UTF-8"?>
        <!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
        <plist version="1.0">
        <dict>
            <key>name</key>
            <string>John</string>
            <key>hobbies</key>
            <array>
                <string>reading</string>
                <string>swimming</string>
                <string>coding</string>
            </array>
        </dict>
        </plist>
        "#;
        let mut plist = Plist::parse(plist_content);
        assert_eq!(
            plist.0.get("name").unwrap(),
            &PlistValue::String("John".to_string())
        );
        assert_eq!(
            plist.0.get("hobbies").unwrap(),
            &PlistValue::Array(vec![
                "reading".to_string(),
                "swimming".to_string(),
                "coding".to_string()
            ])
        );

        plist.insert("name", "Mike");
        assert_eq!(
            plist.0.get("name").unwrap(),
            &PlistValue::String("Mike".to_string())
        );

        plist.insert("age", "25");
        assert_eq!(
            plist.0.get("age").unwrap(),
            &PlistValue::String("25".to_string())
        );
    }

    #[test]
    fn test_serialize_plist() {
        let plist = Plist(BTreeMap::from([
            ("name".to_string(), PlistValue::String("John".to_string())),
            (
                "hobbies".to_string(),
                PlistValue::Array(vec![
                    "reading".to_string(),
                    "swimming".to_string(),
                    "coding".to_string(),
                ]),
            ),
        ]));
        let serialized = plist.to_string();
        assert_eq!(
            serialized,
            r#"
        <?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
    <dict>
        <key>hobbies</key>
        <array>
            <string>reading</string>
            <string>swimming</string>
            <string>coding</string>
        </array>
        <key>name</key>
        <string>John</string>
    </dict>
</plist>
        "#.trim()
        );
    }
}
