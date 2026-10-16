//! The external repository that holds pacor's build templates and helper tools.
//!
//! Templates are fetched from a **tag**, never from a branch, so a change to them
//! can never reach a pacor build that does not understand it: the templates and
//! the code that renders them move together, and a new contract is published as
//! a new tag.
//!
//! Every backend pins the same tag, so a pacor release has exactly one notion of
//! which templates it speaks to. Because the tag is part of each backend's
//! download cache path, bumping it is also what invalidates stale cached copies —
//! tag contents are immutable, so nothing else is needed.

use std::path::PathBuf;

use crate::platforms::Platform;

/// Repository holding the build templates and helper tools.
pub const TOOLS_REPOSITORY: &str = "pacor-rs/tools";

/// Tag of [`TOOLS_REPOSITORY`] that this build of pacor is written against.
///
/// Bump this when pacor starts providing a different set of template variables,
/// gives an existing one a different meaning, or when a template fix must reach
/// installations that already cached the previous tag. The tag message in the
/// repository documents the contract it describes.
///
/// Tags are GNU-style version numbers (`MAJOR.MINOR`, optionally with a patch
/// level) without a `v` prefix: a backward-compatible template addition is a
/// minor bump, an incompatible one a major bump. `v1` predates that convention
/// and holds the tree that `1.0` would name; `1.1` adds the WiX templates the
/// MSI backend renders.
///
/// See <https://github.com/pacor-rs/tools> for the released tags.
pub const TEMPLATE_REF: &str = "1.1";

/// Raw URL of `path` inside the pinned tag, e.g. `linux/deb/template.cc`.
///
/// `path` is relative to the repository root and uses `/` separators.
pub fn template_url(path: &str) -> String {
    format!("https://raw.githubusercontent.com/{TOOLS_REPOSITORY}/refs/tags/{TEMPLATE_REF}/{path}")
}

/// Root of pacor's cache of downloaded tools and templates.
///
/// `None` when the platform has no user configuration directory.
pub fn cache_root() -> Option<PathBuf> {
    dirs::config_dir().map(|directory| directory.join("pacor"))
}

/// Cache directory holding `platform`'s downloaded tools.
///
/// The directory name comes from [`Platform::as_str`], so a backend cannot end
/// up reading a differently-named directory than the one `pacor cache clean`
/// reports.
///
/// Android packages straight from the Flutter build output and caches nothing,
/// so it has no directory of its own.
pub fn platform_cache_dir(platform: Platform) -> Option<PathBuf> {
    match platform {
        Platform::Linux | Platform::MacOS | Platform::Windows => {
            cache_root().map(|root| root.join(platform.as_str()))
        }
        Platform::Android => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_are_pinned_to_the_tag_and_never_a_branch() {
        let url = template_url("linux/deb/my_application_template.cc");

        assert!(
            url.contains(&format!("/refs/tags/{TEMPLATE_REF}/")),
            "{url}"
        );
        assert!(
            !url.contains("/refs/heads/"),
            "a branch URL would let a template change reach an incompatible \
             pacor build: {url}"
        );
    }

    /// Tags are GNU-style version numbers, without the `v` prefix git users
    /// often add, so a tag cannot be confused with a branch name.
    #[test]
    fn the_tag_is_a_gnu_style_version_number() {
        let (major, minor) = TEMPLATE_REF
            .split_once('.')
            .expect("the tag needs at least MAJOR.MINOR");

        assert!(!TEMPLATE_REF.starts_with('v'), "{TEMPLATE_REF}");
        assert!(
            major.parse::<u32>().is_ok(),
            "the major version must be numeric: {TEMPLATE_REF}"
        );
        assert!(
            minor.split('.').all(|part| part.parse::<u32>().is_ok()),
            "every component must be numeric: {TEMPLATE_REF}"
        );
        assert!(
            !TEMPLATE_REF.contains('/') && !TEMPLATE_REF.contains(' '),
            "{TEMPLATE_REF}"
        );
    }

    #[test]
    fn the_pinned_tag_really_appears_in_the_url() {
        // Guards against the constant being documented as the pin while the URL
        // hard-codes something else.
        for path in ["linux/appimage/x.cc", "windows/nsis/template.nsi"] {
            assert!(template_url(path).contains(TEMPLATE_REF), "{path}");
        }
    }

    #[test]
    fn the_path_is_appended_verbatim() {
        assert!(
            template_url("windows/nsis/languages/English.nsh")
                .ends_with("/windows/nsis/languages/English.nsh")
        );
    }

    #[test]
    fn cache_directories_live_under_the_pacor_config_directory() {
        for platform in [Platform::Linux, Platform::MacOS, Platform::Windows] {
            let dir = platform_cache_dir(platform).expect("these platforms cache tools");

            assert_eq!(dir.file_name().unwrap(), platform.as_str());
            assert_eq!(dir.parent(), cache_root().as_deref());
        }

        // Android caches nothing, so it must not claim a directory.
        assert!(platform_cache_dir(Platform::Android).is_none());
    }

    #[test]
    fn both_platforms_share_one_pin() {
        // A single tag covers every template, so the two backends cannot drift
        // onto different contracts.
        let linux = template_url("linux/deb/x");
        let windows = template_url("windows/nsis/x");

        assert_eq!(
            linux.split("/refs/tags/").nth(1).unwrap().split('/').next(),
            Some(TEMPLATE_REF)
        );
        assert_eq!(
            windows
                .split("/refs/tags/")
                .nth(1)
                .unwrap()
                .split('/')
                .next(),
            Some(TEMPLATE_REF)
        );
    }
}
