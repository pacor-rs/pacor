use std::{
    collections::{HashMap, HashSet},
    fmt,
};

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::{error::ConfigurationError, i18n::Locale};

/// Localized application metadata
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppMetadata {
    /// Application display name
    ///
    /// If not specified, the `name` of `pubspec.yaml` will be used.
    name: Option<String>,

    display_name: Option<String>,

    /// Short description (optional for localization)
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,

    /// Company, organization, or publisher name
    publisher: String,

    /// Copyright notice
    copyright: Option<String>,
}

impl AppMetadata {
    pub fn new<S: ToString, I: Into<Option<S>>>(
        name: I,
        display_name: I,
        description: I,
        publisher: S,
        copyright: I,
    ) -> Self {
        Self {
            name: name.into().map(|s| s.to_string()),
            display_name: display_name.into().map(|s| s.to_string()),
            description: description.into().map(|s| s.to_string()),
            publisher: publisher.to_string(),
            copyright: copyright.into().map(|s| s.to_string()),
        }
    }

    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    pub fn display_name(&self) -> Option<&str> {
        self.display_name.as_deref()
    }

    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }

    /// Returns the publisher of the app.
    ///
    /// macOS not supported
    pub fn publisher(&self) -> &str {
        self.publisher.as_str()
    }

    pub fn copyright(&self) -> Option<&str> {
        self.copyright.as_deref()
    }
}

/// Application configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    /// Default locale
    ///
    /// For single-language apps, this is the only locale.
    /// For multi-language apps, this is the fallback locale.
    pub default_locale: Locale,

    /// Localized metadata
    ///
    /// Must contain at least the `default_locale` entry.
    /// For single-language apps, this contains only one entry.
    #[serde(alias = "localizations")]
    pub i18n: HashMap<Locale, AppMetadata>,

    /// Authors (optional, locale-independent)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub authors: Option<HashSet<String>>,

    /// Application website or repository
    pub url: String,
}

impl AppConfig {
    pub fn validate(&self) -> Result<(), ConfigurationError> {
        if self.i18n.is_empty() {
            return Err(ConfigurationError::MissingRequired {
                field: "app.i18n".to_string(),
            });
        }

        if !self.i18n.contains_key(&self.default_locale) {
            return Err(ConfigurationError::MissingRequired {
                field: format!("app.i18n.{}", self.default_locale),
            });
        }

        Ok(())
    }

    pub fn is_single_language(&self) -> bool {
        self.i18n.len() == 1
    }

    pub fn url(&self) -> &str {
        self.url.as_ref()
    }

    pub fn i18n(&self) -> &HashMap<Locale, AppMetadata> {
        &self.i18n
    }

    pub fn default_locale(&self) -> &Locale {
        &self.default_locale
    }

    /// Metadata for the default locale.
    ///
    /// Falls back to any available locale so callers never panic on a config
    /// that has not been validated yet; `validate()` reports the real problem.
    pub fn default_metadata(&self) -> Option<&AppMetadata> {
        self.i18n
            .get(&self.default_locale)
            .or_else(|| self.i18n.values().next())
    }

    pub fn name(&self) -> Option<&str> {
        self.default_metadata().and_then(AppMetadata::name)
    }

    pub fn display_name(&self) -> Option<&str> {
        self.default_metadata().and_then(AppMetadata::display_name)
    }

    pub fn description(&self) -> Option<&str> {
        self.default_metadata().and_then(AppMetadata::description)
    }

    pub fn publisher(&self) -> Option<&str> {
        self.default_metadata().map(AppMetadata::publisher)
    }

    pub fn copyright(&self) -> Option<&str> {
        self.default_metadata().and_then(AppMetadata::copyright)
    }

    pub fn authors(&self) -> Option<&HashSet<String>> {
        self.authors.as_ref()
    }
}

impl fmt::Display for AppConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Header
        writeln!(f, "Application Configuration")?;
        writeln!(f, "========================")?;
        writeln!(f)?;

        // Language info
        if self.is_single_language() {
            writeln!(f, "Language: {} (single-language)", self.default_locale)?;
        } else {
            writeln!(f, "Default Language: {}", self.default_locale)?;
            writeln!(f, "Supported Languages: {}", self.i18n.len())?;
        }
        writeln!(f)?;

        // Display localized metadata for each language
        let mut locales: Vec<_> = self.i18n.keys().collect();
        locales.sort();

        for locale in locales {
            let is_default = locale == &self.default_locale;
            let meta = &self.i18n[locale];

            // Locale header
            if is_default {
                writeln!(f, "[{}] (default):", locale)?;
            } else {
                writeln!(f, "[{}]:", locale)?;
            }

            // Name
            if let Some(name) = &meta.name {
                writeln!(f, "  Name: {}", name)?;
            }

            // Display Name
            if let Some(display_name) = &meta.display_name {
                writeln!(f, "  Display Name: {}", display_name)?;
            }

            // Description
            if let Some(desc) = &meta.description {
                writeln!(f, "  Description: {}", desc)?;
            }

            // Publisher
            writeln!(f, "  Publisher: {}", meta.publisher)?;

            // Copyright
            if let Some(copyright) = &meta.copyright {
                writeln!(f, "  Copyright: {}", copyright)?;
            }

            writeln!(f)?;
        }

        // Authors (locale-independent)
        if let Some(authors) = &self.authors
            && !authors.is_empty()
        {
            writeln!(f, "Authors:")?;
            for author in authors {
                writeln!(f, "  - {}", author)?;
            }
            writeln!(f)?;
        }

        // URL
        writeln!(f, "URL: {}", self.url)?;

        Ok(())
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        AppConfig {
            default_locale: Locale::EnUs,
            url: "https://example.com".to_string(),
            authors: Some(HashSet::from([
                "Author Name 1".to_string(),
                "Author Name 2".to_string(),
            ])),
            i18n: HashMap::from([
                (
                    Locale::EnUs,
                    AppMetadata::new(
                        "My Video App",
                        "My App",
                        "It is a video application",
                        "Publisher Name",
                        "Copyright @ 2023 Publisher Name. All rights reserved.",
                    ),
                ),
                (
                    Locale::ZhCn,
                    AppMetadata::new(
                        "我的应用",
                        "我的应用",
                        "这是一个视频应用",
                        "我的公司",
                        "版权所有 © 2025 我的公司。保留所有权利。",
                    ),
                ),
            ]),
        }
    }
}
