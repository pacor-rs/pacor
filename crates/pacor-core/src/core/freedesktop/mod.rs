use std::fmt::Display;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::Result;

use self::category::{AdditionalCategory, Category, MainCategory};

mod category;
mod related;

/// A problem detected in a desktop entry's `Categories` value.
///
/// Returned by [`FreeDesktop::category_problems`]. The caller decides whether a
/// problem is fatal, because the specification treats the two cases differently:
/// a missing main category makes the entry unusable, while a missing *related*
/// category is only a deviation from the registry's suggestion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CategoryProblem {
    /// Additional categories are present but no main category is.
    ///
    /// Main categories decide which menu an entry appears in, so an entry that
    /// lists only additional categories cannot be placed in any menu. Debian's
    /// Lintian reports this as `desktop-entry-lacks-main-category`.
    MissingMainCategory {
        /// The additional categories that were configured, sorted.
        additional: Vec<String>,
    },

    /// An additional category is present without any of its related categories.
    ///
    /// The registry describes these relations as "suggested", so this is
    /// advisory: the entry installs, but it may not appear where users expect.
    UnrelatedAdditional {
        /// The additional category that has no related category.
        category: String,
        /// Acceptable combinations, rendered for display.
        suggested: Vec<String>,
    },
}

/// Main categories from the registry, listed in diagnostics.
const MAIN_CATEGORY_HINT: &str = "AudioVideo, Development, Game, Graphics, Network, Office, \
                                  Science, Settings, System, Utility";

impl Display for CategoryProblem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingMainCategory { additional } => {
                let listed = if additional.is_empty() {
                    "`Categories` is empty".to_string()
                } else {
                    format!(
                        "`Categories` lists only additional categories ({})",
                        additional.join(", ")
                    )
                };

                write!(
                    f,
                    "{listed}; at least one main category is required for the entry to \
                     appear in a menu ({MAIN_CATEGORY_HINT})"
                )
            }
            Self::UnrelatedAdditional {
                category,
                suggested,
            } => write!(
                f,
                "additional category `{category}` is normally used together with {}",
                suggested
                    .iter()
                    .map(|group| format!("`{group}`"))
                    .collect::<Vec<_>>()
                    .join(", or ")
            ),
        }
    }
}

/// A freedesktop.org desktop entry.
///
/// Only the fields that `pacor` derives from the application configuration are
/// public API; everything the user can set lives in `[platforms.linux.desktop]`
/// in `pacor.toml`.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FreeDesktop {
    #[serde(skip)]
    name: String,
    #[serde(skip)]
    icon: String,
    #[serde(skip)]
    exec: String,
    #[serde(skip)]
    startup_wm_class: String,
    #[serde(default = "default_categories")]
    categories: Vec<Category>,
    comment: Option<String>,
    mime_type: Option<String>,
    #[serde(skip)]
    localized_names: std::collections::BTreeMap<String, String>,
    #[serde(skip)]
    localized_comments: std::collections::BTreeMap<String, String>,
}

/// `Categories` is required for `Type=Application` entries, so fall back to the
/// generic "Accessories" menu instead of emitting an invalid empty value.
fn default_categories() -> Vec<Category> {
    vec![Category::Main(MainCategory::Utility)]
}

impl Default for FreeDesktop {
    fn default() -> Self {
        Self {
            name: String::new(),
            icon: String::new(),
            exec: String::new(),
            startup_wm_class: String::new(),
            categories: default_categories(),
            comment: None,
            mime_type: None,
            localized_names: Default::default(),
            localized_comments: Default::default(),
        }
    }
}

impl Display for FreeDesktop {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "[Desktop Entry]")?;
        writeln!(f, "Type=Application")?;
        writeln!(f, "Name={}", self.name)?;
        for (locale, name) in &self.localized_names {
            writeln!(f, "Name[{locale}]={name}")?;
        }
        writeln!(f, "Icon={}", self.icon)?;
        writeln!(f, "Exec={}", self.exec)?;

        if !self.startup_wm_class.is_empty() {
            writeln!(f, "StartupWMClass={}", self.startup_wm_class)?;
        }

        writeln!(f, "Terminal=false")?;

        // Emit the key only when it has a value: `Categories=` (empty) is
        // rejected by `desktop-file-validate`.
        if !self.categories.is_empty() {
            let categories = self
                .categories
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(";");
            writeln!(f, "Categories={categories}")?;
        }

        if let Some(comment) = &self.comment {
            writeln!(f, "Comment={comment}")?;
        }
        for (locale, comment) in &self.localized_comments {
            writeln!(f, "Comment[{locale}]={comment}")?;
        }
        if let Some(mime_type) = &self.mime_type {
            writeln!(f, "MimeType={mime_type}")?;
        }

        Ok(())
    }
}

impl FreeDesktop {
    pub fn set_name(&mut self, name: &str) {
        self.name = name.to_string();
    }

    pub fn set_icon<S: Into<String>>(&mut self, icon: S) {
        self.icon = icon.into();
    }

    /// Set the `Exec` key.
    ///
    /// This deliberately does *not* touch `StartupWMClass`: for an AppImage the
    /// entry point becomes `AppRun`, while the window class stays the Flutter
    /// application name. Use [`Self::set_startup_wm_class`] for the latter.
    pub fn set_exec(&mut self, exec: &str) {
        self.exec = exec.to_string();
    }

    /// Set the `StartupWMClass` key (must match the application's `WM_CLASS`).
    pub fn set_startup_wm_class(&mut self, class: &str) {
        self.startup_wm_class = class.to_string();
    }

    pub fn set_comment(&mut self, comment: &str) {
        self.comment = Some(comment.to_string());
    }

    pub fn set_mime_type<S: Into<String>>(&mut self, mime_type: S) {
        self.mime_type = Some(mime_type.into());
    }

    pub fn set_localized_name(&mut self, locale: &str, name: &str) {
        self.localized_names
            .insert(locale.to_string(), name.to_string());
    }

    pub fn set_localized_comment(&mut self, locale: &str, comment: &str) {
        self.localized_comments
            .insert(locale.to_string(), comment.to_string());
    }

    /// Problems with the configured `Categories` value.
    ///
    /// Applies two rules from the Desktop Menu Specification:
    ///
    /// 1. Additional categories require at least one main category — without one
    ///    the entry cannot be placed in any menu. Reported once, as
    ///    [`CategoryProblem::MissingMainCategory`].
    /// 2. Every additional category should be accompanied by one of its related
    ///    categories. Reported per category as
    ///    [`CategoryProblem::UnrelatedAdditional`].
    ///
    /// Rule 2 is skipped when rule 1 already applies, because related-category
    /// advisories are meaningless without a main category.
    pub fn category_problems(&self) -> Vec<CategoryProblem> {
        let present: std::collections::HashSet<String> =
            self.categories.iter().map(ToString::to_string).collect();

        let additional: Vec<&AdditionalCategory> = self
            .categories
            .iter()
            .filter_map(|category| match category {
                Category::Additional(additional) => Some(additional),
                _ => None,
            })
            .collect();

        let has_main_category = self
            .categories
            .iter()
            .any(|category| matches!(category, Category::Main(_)));

        // An empty list is the same defect as a list of additional categories only: no
        // main category, so no menu the entry could appear in. `categories = []` used to
        // fall through here and validate, producing a desktop file with no `Categories`
        // at all — installed, listed in the package, and invisible in every menu.
        if has_main_category && additional.is_empty() {
            return Vec::new();
        }

        if !has_main_category {
            let mut names: Vec<String> = additional
                .iter()
                .map(ToString::to_string)
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .collect();
            names.sort();

            return vec![CategoryProblem::MissingMainCategory { additional: names }];
        }

        let mut problems = Vec::new();
        let mut reported = std::collections::BTreeSet::new();

        for category in additional {
            let name = category.to_string();
            if !reported.insert(name.clone()) {
                continue;
            }

            let groups = related::related_categories(category);
            if groups.is_empty() {
                // The registry does not constrain this category.
                continue;
            }

            // Satisfied when every name of at least one AND-group is present.
            let satisfied = groups
                .iter()
                .any(|group| group.iter().all(|required| present.contains(*required)));

            if !satisfied {
                problems.push(CategoryProblem::UnrelatedAdditional {
                    category: name,
                    suggested: groups.iter().map(|group| group.join(" and ")).collect(),
                });
            }
        }

        problems
    }

    pub fn write(&self, path: &Path) -> Result<()> {
        std::fs::write(path, self.to_string())?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_entry_is_valid() {
        let output = FreeDesktop::default().to_string();

        assert!(output.contains("[Desktop Entry]"));
        assert!(output.contains("Type=Application"));
        assert!(output.contains("Terminal=false"));
        // `Categories` is required and must never be emitted empty.
        assert!(output.contains("Categories=Utility"));
        assert!(!output.contains("Categories=\n"));
    }

    #[test]
    fn categories_render_semicolon_separated() {
        let mut desktop = FreeDesktop::default();
        desktop.set_name("TestApp");
        desktop.categories = vec![
            Category::Main(MainCategory::Utility),
            Category::Additional(category::AdditionalCategory::FileTools),
        ];

        assert!(desktop.to_string().contains("Categories=Utility;FileTools"));
    }

    #[test]
    fn an_empty_category_list_omits_the_key_entirely() {
        let mut desktop = FreeDesktop::default();
        desktop.categories.clear();

        let output = desktop.to_string();
        assert!(!output.contains("Categories"), "{output}");
    }

    #[test]
    fn a_desktop_table_without_categories_falls_back_to_utility() {
        // Only `comment` is set, so serde must supply the default categories.
        let desktop: FreeDesktop = toml::from_str("comment = \"hello\"").unwrap();

        assert!(desktop.to_string().contains("Categories=Utility"));
    }

    #[test]
    fn configured_categories_replace_the_default() {
        let desktop: FreeDesktop =
            toml::from_str("categories = [{ Main = \"Graphics\" }]").unwrap();

        let output = desktop.to_string();
        assert!(output.contains("Categories=Graphics"));
        assert!(!output.contains("Utility"));
    }

    #[test]
    fn set_exec_does_not_change_the_startup_wm_class() {
        let mut desktop = FreeDesktop::default();
        desktop.set_startup_wm_class("my-app");

        // AppImage rewrites Exec to AppRun; the window class must stay put.
        desktop.set_exec("AppRun");

        let output = desktop.to_string();
        assert!(output.contains("Exec=AppRun"));
        assert!(output.contains("StartupWMClass=my-app"));
    }

    #[test]
    fn an_unset_startup_wm_class_is_omitted() {
        let output = FreeDesktop::default().to_string();
        assert!(!output.contains("StartupWMClass"), "{output}");
    }

    #[test]
    fn set_methods_populate_the_entry() {
        let mut desktop = FreeDesktop::default();
        desktop.set_name("MyApp");
        desktop.set_icon("my-icon");
        desktop.set_exec("my-exec");
        desktop.set_startup_wm_class("my-exec");

        let output = desktop.to_string();
        assert!(output.contains("Name=MyApp"));
        assert!(output.contains("Icon=my-icon"));
        assert!(output.contains("Exec=my-exec"));
        assert!(output.contains("StartupWMClass=my-exec"));
    }

    #[test]
    fn write_creates_the_file() {
        let mut desktop = FreeDesktop::default();
        desktop.set_name("WriteTest");
        desktop.set_icon("write-icon");
        desktop.set_exec("write-exec");

        let temp_dir = tempfile::TempDir::new().unwrap();
        let desktop_path = temp_dir.path().join("test.desktop");

        desktop.write(&desktop_path).unwrap();

        let content = std::fs::read_to_string(&desktop_path).unwrap();
        assert!(content.contains("Name=WriteTest"));
    }

    #[test]
    fn localized_names_and_comments_are_rendered() {
        let mut desktop = FreeDesktop::default();
        desktop.set_name("MyApp");
        desktop.set_comment("A great app");
        desktop.set_localized_name("zh_CN", "我的应用");
        desktop.set_localized_name("ja-JP", "マイアプリ");
        desktop.set_localized_comment("zh_CN", "一个很棒的应用");
        desktop.set_icon("my-icon");
        desktop.set_exec("my-exec");

        let output = desktop.to_string();
        assert!(output.contains("Name=MyApp"));
        assert!(output.contains("Name[zh_CN]=我的应用"));
        assert!(output.contains("Name[ja-JP]=マイアプリ"));
        assert!(output.contains("Comment=A great app"));
        assert!(output.contains("Comment[zh_CN]=一个很棒的应用"));
    }

    #[test]
    fn comment_and_mime_type_are_omitted_when_unset() {
        let output = FreeDesktop::default().to_string();
        assert!(!output.contains("Comment="));
        assert!(!output.contains("MimeType="));
    }

    /// Parse a `Categories` list the way `pacor.toml` expresses it.
    fn with_categories(toml_value: &str) -> FreeDesktop {
        let document = format!("categories = {toml_value}");
        toml::from_str(&document).unwrap()
    }

    #[test]
    fn the_default_category_list_has_no_problems() {
        assert!(FreeDesktop::default().category_problems().is_empty());
    }

    #[test]
    fn additional_categories_without_a_main_category_are_reported() {
        let desktop = with_categories(r#"[{ Additional = "FileTools" }]"#);
        let problems = desktop.category_problems();

        assert_eq!(
            problems,
            vec![CategoryProblem::MissingMainCategory {
                additional: vec!["FileTools".to_string()],
            }]
        );
        // The message must name the offending categories and the requirement.
        let message = problems[0].to_string();
        assert!(message.contains("FileTools"), "{message}");
        assert!(message.contains("main category"), "{message}");
    }

    #[test]
    fn the_missing_main_category_report_is_reported_once_and_sorted() {
        let desktop = with_categories(
            r#"[{ Additional = "TextEditor" }, { Additional = "Clock" }, { Additional = "Calculator" }]"#,
        );

        let problems = desktop.category_problems();

        assert_eq!(problems.len(), 1, "one report, not one per category");
        let CategoryProblem::MissingMainCategory { additional } = &problems[0] else {
            panic!("expected MissingMainCategory, got {:?}", problems[0]);
        };
        assert_eq!(additional, &["Calculator", "Clock", "TextEditor"]);
    }

    #[test]
    fn a_matching_main_category_satisfies_the_relation() {
        // `TextEditor` is normally used with `Utility`.
        let desktop = with_categories(r#"[{ Main = "Utility" }, { Additional = "TextEditor" }]"#);
        assert!(desktop.category_problems().is_empty());
    }

    #[test]
    fn each_alternative_of_an_or_relation_is_accepted() {
        // `Player` accepts Audio or Video or AudioVideo.
        for main in ["Audio", "Video", "AudioVideo"] {
            let desktop = with_categories(&format!(
                r#"[{{ Main = "{main}" }}, {{ Additional = "Player" }}]"#
            ));
            assert!(
                desktop.category_problems().is_empty(),
                "`{main}` should satisfy Player"
            );
        }
    }

    #[test]
    fn an_unrelated_main_category_is_reported_as_advisory() {
        // `Player` does not relate to `Office`.
        let desktop = with_categories(r#"[{ Main = "Office" }, { Additional = "Player" }]"#);
        let problems = desktop.category_problems();

        assert_eq!(
            problems,
            vec![CategoryProblem::UnrelatedAdditional {
                category: "Player".to_string(),
                suggested: vec![
                    "Audio".to_string(),
                    "Video".to_string(),
                    "AudioVideo".to_string()
                ],
            }]
        );
        let message = problems[0].to_string();
        assert!(message.contains("Player"), "{message}");
        assert!(message.contains("AudioVideo"), "{message}");
    }

    #[test]
    fn conjunctive_relations_require_every_name_in_a_group() {
        // `NumericalAnalysis` maps to (Education and Math) or (Science and Math).
        let incomplete =
            with_categories(r#"[{ Main = "Science" }, { Additional = "NumericalAnalysis" }]"#);
        assert_eq!(incomplete.category_problems().len(), 1);

        let complete = with_categories(
            r#"[{ Main = "Science" }, { Additional = "Math" }, { Additional = "NumericalAnalysis" }]"#,
        );
        assert!(
            complete.category_problems().is_empty(),
            "Science + Math should satisfy NumericalAnalysis"
        );

        let other_group = with_categories(
            r#"[{ Main = "Education" }, { Additional = "Math" }, { Additional = "NumericalAnalysis" }]"#,
        );
        assert!(
            other_group.category_problems().is_empty(),
            "the Education+Math alternative should also satisfy it"
        );
    }

    #[test]
    fn the_suggestion_for_a_conjunctive_relation_reads_naturally() {
        let desktop =
            with_categories(r#"[{ Main = "Utility" }, { Additional = "NumericalAnalysis" }]"#);
        let problems = desktop.category_problems();

        let CategoryProblem::UnrelatedAdditional { suggested, .. } = &problems[0] else {
            panic!("expected UnrelatedAdditional, got {:?}", problems[0]);
        };
        assert_eq!(
            suggested,
            &[
                "Education and Math".to_string(),
                "Science and Math".to_string()
            ]
        );
    }

    #[test]
    fn toolkit_categories_relate_to_their_registry_entry() {
        // `KDE` relates to `Qt`, which is itself an additional category. The
        // registry writes it `QT`; the canonical spelling is `Qt`.
        let satisfied = with_categories(
            r#"[{ Main = "Utility" }, { Additional = "Qt" }, { Additional = "KDE" }]"#,
        );
        assert!(satisfied.category_problems().is_empty());

        // `GNOME` relates to `GTK`, which is absent here.
        let unsatisfied = with_categories(r#"[{ Main = "Utility" }, { Additional = "GNOME" }]"#);
        assert_eq!(unsatisfied.category_problems().len(), 1);
    }

    #[test]
    fn canonical_registry_spellings_are_accepted_in_toml() {
        // The Rust variant names differ from the registry spellings, so serde
        // renames must keep `pacor.toml` on the canonical names.
        let desktop = with_categories(
            r#"[{ Additional = "2DGraphics" }, { Additional = "IDE" }, { Additional = "GTK" },
                { Additional = "KDE" }, { Additional = "OCR" }, { Additional = "PDA" }]"#,
        );

        let rendered: Vec<String> = desktop.categories.iter().map(ToString::to_string).collect();
        assert_eq!(
            rendered,
            vec!["2DGraphics", "IDE", "GTK", "KDE", "OCR", "PDA"]
        );
    }

    #[test]
    fn a_renamed_category_renders_with_its_canonical_spelling() {
        let desktop = with_categories(r#"[{ Main = "Development" }, { Additional = "IDE" }]"#);
        let output = desktop.to_string();

        assert!(output.contains("Categories=Development;IDE"), "{output}");
        assert!(desktop.category_problems().is_empty());
    }

    #[test]
    fn categories_without_a_registered_relation_accept_any_main_category() {
        // `Amusement`, `GTK` and friends have a blank Related Categories column.
        for additional in ["Amusement", "GTK", "Documentation", "ConsoleOnly"] {
            let desktop = with_categories(&format!(
                r#"[{{ Main = "Utility" }}, {{ Additional = "{additional}" }}]"#
            ));
            assert!(
                desktop.category_problems().is_empty(),
                "`{additional}` should accept any main category"
            );
        }
    }

    #[test]
    fn a_duplicated_additional_category_is_reported_once() {
        let desktop = with_categories(
            r#"[{ Main = "Office" }, { Additional = "Player" }, { Additional = "Player" }]"#,
        );

        assert_eq!(desktop.category_problems().len(), 1);
    }

    #[test]
    fn reserved_categories_do_not_trigger_the_pairing_rules() {
        let desktop = with_categories(r#"[{ Main = "Utility" }, { Reserved = "TrayIcon" }]"#);
        assert!(desktop.category_problems().is_empty());
    }

    #[test]
    fn mime_type_is_rendered_when_set() {
        let mut desktop = FreeDesktop::default();
        desktop.set_mime_type("x-scheme-handler/myapp");

        assert!(
            desktop
                .to_string()
                .contains("MimeType=x-scheme-handler/myapp")
        );
    }
}

#[cfg(test)]
mod message_tests {
    use super::*;

    #[test]
    fn problem_messages_have_no_stray_whitespace() {
        let missing = CategoryProblem::MissingMainCategory {
            additional: vec!["FileTools".to_string()],
        }
        .to_string();
        assert!(
            !missing.contains("  "),
            "message has collapsed line-continuation whitespace: {missing:?}"
        );
        assert!(missing.contains("Utility)"), "{missing}");
        assert!(
            missing.contains("at least one main category is required"),
            "{missing}"
        );

        let unrelated = CategoryProblem::UnrelatedAdditional {
            category: "Player".to_string(),
            suggested: vec!["Audio".to_string()],
        }
        .to_string();
        assert!(!unrelated.contains("  "), "{unrelated:?}");
    }
}
