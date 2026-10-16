//! GitHub download utilities with optional mirror support.
//!
//! # Mirror Configuration
//!
//! This module supports two types of GitHub mirrors via environment variables:
//!
//! ## Template-based Mirror
//!
//! - `PACOR_GITHUB_RELEASE_MIRROR_TEMPLATE`: Template for release assets
//!   - Placeholders: `<owner>`, `<repo>`, `<version>`, `<asset>`
//!   - Example: `https://mirror.example.com/<owner>/<repo>/releases/<version>/<asset>`
//!
//! - `PACOR_GITHUB_RAW_MIRROR_TEMPLATE`: Template for raw content
//!   - Placeholders: `<owner>`, `<repo>`, `<branch>`, `<path>`
//!   - Example: `https://mirror.example.com/<owner>/<repo>/raw/<branch>/<path>`
//!
//! ## Base URL Mirror
//!
//! - `PACOR_GITHUB_MIRROR_BASE_URL`: Base URL that will be prefixed to the full GitHub URL
//!   - Example: `https://mirror.example.com` → `https://mirror.example.com/github.com/...`
//!
//! # Mirror URL Validation
//!
//! **This module does NOT validate mirror URLs.** Users are responsible for:
//! - Ensuring mirror URLs are well-formed and accessible
//! - Configuring correct template placeholders
//! - Verifying mirror service availability
//!
//! Invalid mirror URLs will result in download failures with network errors.

use std::{borrow::Cow, io::Read};

use ureq::{
    Agent, Proxy,
    config::ConfigBuilder,
    tls::{RootCerts, TlsConfig, TlsProvider},
    typestate::AgentScope,
};

use anyhow::Result;

// === Environment variable keys ===
const ENV_GITHUB_MIRROR_BASE_URL: &str = "PACOR_GITHUB_MIRROR_BASE_URL";
const ENV_GITHUB_RELEASE_MIRROR_TEMPLATE: &str = "PACOR_GITHUB_RELEASE_MIRROR_TEMPLATE";
const ENV_GITHUB_RAW_MIRROR_TEMPLATE: &str = "PACOR_GITHUB_RAW_MIRROR_TEMPLATE";

// === GitHub URL prefixes ===
const GITHUB_RELEASE_PREFIX: &str = "https://github.com/";
const GITHUB_RAW_PREFIX: &str = "https://raw.githubusercontent.com/";

// === Parsed URL structs ===

/// Represents a parsed GitHub release URL:  
/// `https://github.com/<owner>/<repo>/releases/download/<version>/<asset>`
#[derive(Debug)]
struct ParsedReleaseUrl<'a> {
    owner: &'a str,
    repo: &'a str,
    version: &'a str,
    asset_name: &'a str,
}

/// Represents a parsed GitHub raw content URL:  
/// `https://raw.githubusercontent.com/<owner>/<repo>/<ref>/<path>`
#[derive(Debug)]
struct ParsedRawUrl<'a> {
    owner: Cow<'a, str>,
    repo: Cow<'a, str>,
    git_ref: Cow<'a, str>,
    file_path: Cow<'a, str>,
}

// === URL parsing ===

fn parse_github_release_url(url: &str) -> Option<ParsedReleaseUrl<'_>> {
    if !url.starts_with(GITHUB_RELEASE_PREFIX) {
        return None;
    }

    let path = url.strip_prefix(GITHUB_RELEASE_PREFIX)?;
    let parts: Vec<&str> = path.splitn(5, '/').collect();

    // Expected: [owner, repo, "releases", "download", "v1.0.0/file.tar.gz"]
    if parts.len() != 5 || parts[2] != "releases" || parts[3] != "download" {
        return None;
    }

    let (version, asset_name) = parts[4].split_once('/')?;

    Some(ParsedReleaseUrl {
        owner: parts[0],
        repo: parts[1],
        version,
        asset_name,
    })
}

fn parse_github_raw_url(url: &str) -> Option<ParsedRawUrl<'_>> {
    if !url.starts_with(GITHUB_RAW_PREFIX) {
        return None;
    }

    let path = url.strip_prefix(GITHUB_RAW_PREFIX)?;
    let parts: Vec<&str> = path.split('/').collect();

    if parts.len() < 4 {
        return None;
    }

    let owner = parts[0];
    let repo = parts[1];

    // Handle both `/refs/heads/<branch>/...` and direct `<ref>/...`
    if parts.len() >= 6 && parts[2] == "refs" && (parts[3] == "heads" || parts[3] == "tags") {
        let git_ref = parts[4];
        let file_path = parts[5..].join("/");
        Some(ParsedRawUrl {
            owner: Cow::Borrowed(owner),
            repo: Cow::Borrowed(repo),
            git_ref: Cow::Borrowed(git_ref),
            file_path: Cow::Owned(file_path),
        })
    } else {
        let git_ref = parts[2];
        let file_path = parts[3..].join("/");
        Some(ParsedRawUrl {
            owner: Cow::Borrowed(owner),
            repo: Cow::Borrowed(repo),
            git_ref: Cow::Borrowed(git_ref),
            file_path: Cow::Owned(file_path),
        })
    }
}

// === Mirror URL construction ===

fn build_release_mirror_url(original_url: &str) -> Option<String> {
    let template = std::env::var(ENV_GITHUB_RELEASE_MIRROR_TEMPLATE).ok()?;
    let parsed = parse_github_release_url(original_url)?;

    Some(
        template
            .replace("<owner>", parsed.owner)
            .replace("<repo>", parsed.repo)
            .replace("<version>", parsed.version)
            .replace("<asset>", parsed.asset_name),
    )
}

fn build_raw_mirror_url(original_url: &str) -> Option<String> {
    let template = std::env::var(ENV_GITHUB_RAW_MIRROR_TEMPLATE).ok()?;
    let parsed = parse_github_raw_url(original_url)?;

    Some(
        template
            .replace("<owner>", &parsed.owner)
            .replace("<repo>", &parsed.repo)
            .replace("<branch>", &parsed.git_ref)
            .replace("<path>", &parsed.file_path),
    )
}

fn build_mirror_url_from_template(original_url: &str) -> Option<String> {
    if original_url.starts_with(GITHUB_RELEASE_PREFIX) {
        build_release_mirror_url(original_url)
    } else if original_url.starts_with(GITHUB_RAW_PREFIX) {
        build_raw_mirror_url(original_url)
    } else {
        None
    }
}

fn build_mirror_url_from_base(original_url: &str) -> Option<String> {
    let base_url = std::env::var(ENV_GITHUB_MIRROR_BASE_URL).ok()?;

    if !base_url.starts_with("http://") && !base_url.starts_with("https://") {
        warn!(
            base_url,
            "Mirror base URL should start with http:// or https://"
        );
        return None;
    }

    let base_url = base_url.trim_end_matches('/');
    let path = original_url
        .strip_prefix("https://")
        .or_else(|| original_url.strip_prefix("http://"))
        .unwrap_or(original_url);

    Some(format!("{}/{}", base_url, path))
}

// === HTTP client setup ===

fn create_base_agent_builder() -> ConfigBuilder<AgentScope> {
    Agent::config_builder().tls_config(
        TlsConfig::builder()
            .provider(TlsProvider::NativeTls)
            .root_certs(RootCerts::PlatformVerifier)
            .build(),
    )
}

fn create_default_agent() -> Agent {
    create_base_agent_builder()
        .proxy(Proxy::try_from_env())
        .build()
        .into()
}

// === Main logic ===

/// Attempts to resolve an alternative (mirror) URL for GitHub-hosted content.
fn resolve_mirror_url(original_url: &str) -> Option<String> {
    // Only support known GitHub patterns
    if !original_url.starts_with(GITHUB_RELEASE_PREFIX)
        && !original_url.starts_with(GITHUB_RAW_PREFIX)
    {
        return None;
    }

    build_mirror_url_from_template(original_url)
        .or_else(|| build_mirror_url_from_base(original_url))
}

/// Prepares the HTTP agent and final download URL (original or mirrored).
fn prepare_download(original_url: &str) -> (Agent, String) {
    if let Some(mirror_url) = resolve_mirror_url(original_url) {
        debug!(
            original = original_url,
            mirror = mirror_url,
            "Using mirror URL"
        );
        // If mirror_url is some, use a agent without proxy
        let agent_no_proxy = create_base_agent_builder().build().into();
        (agent_no_proxy, mirror_url)
    } else {
        debug!(url = original_url, "Using original URL");
        (create_default_agent(), original_url.to_owned())
    }
}

/// Downloads a file from the given URL, optionally using a configured GitHub mirror.
pub fn download_file(url: &str) -> Result<Vec<u8>> {
    let (agent, final_url) = prepare_download(url);
    info!(url = %final_url, "Downloading");

    let response = agent.get(&final_url).call()?;
    let mut bytes = Vec::new();
    response.into_body().into_reader().read_to_end(&mut bytes)?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ==================== Release URL 解析测试 ====================

    #[test]
    fn test_parse_github_release_url_valid() {
        let url = "https://github.com/user/repo/releases/download/v1.0.0/file.tar.gz";
        let result = parse_github_release_url(url).unwrap();

        assert_eq!(result.owner, "user");
        assert_eq!(result.repo, "repo");
        assert_eq!(result.version, "v1.0.0");
        assert_eq!(result.asset_name, "file.tar.gz");
    }

    #[test]
    fn test_parse_github_release_url_with_complex_asset_name() {
        let url = "https://github.com/rust-lang/rust/releases/download/1.78.0/rustc-1.78.0-x86_64-unknown-linux-gnu.tar.gz";
        let result = parse_github_release_url(url).unwrap();

        assert_eq!(result.owner, "rust-lang");
        assert_eq!(result.repo, "rust");
        assert_eq!(result.version, "1.78.0");
        assert_eq!(
            result.asset_name,
            "rustc-1.78.0-x86_64-unknown-linux-gnu.tar.gz"
        );
    }

    #[test]
    fn test_parse_github_release_url_invalid_missing_parts() {
        assert!(parse_github_release_url("https://github.com/user/repo").is_none());
        assert!(parse_github_release_url("https://github.com/user/repo/releases").is_none());
        assert!(parse_github_release_url("https://github.com/a/b/releases/download/").is_none());
    }

    #[test]
    fn test_parse_github_release_url_invalid_wrong_domain() {
        assert!(
            parse_github_release_url("https://example.com/user/repo/releases/download/v1/file")
                .is_none()
        );
        assert!(
            parse_github_release_url("https://gitlab.com/user/repo/releases/download/v1/file")
                .is_none()
        );
    }

    #[test]
    fn test_parse_github_release_url_invalid_wrong_path() {
        assert!(parse_github_release_url("https://github.com/user/repo/tags/v1.0.0").is_none());
        assert!(
            parse_github_release_url("https://github.com/user/repo/release/download/v1/file")
                .is_none()
        );
    }

    // ==================== Raw URL 解析测试 ====================

    #[test]
    fn test_parse_github_raw_url_simple_format() {
        let url = "https://raw.githubusercontent.com/user/repo/main/README.md";
        let result = parse_github_raw_url(url).unwrap();

        assert_eq!(result.owner.as_ref(), "user");
        assert_eq!(result.repo.as_ref(), "repo");
        assert_eq!(result.git_ref.as_ref(), "main");
        assert_eq!(result.file_path.as_ref(), "README.md");
    }

    #[test]
    fn test_parse_github_raw_url_refs_heads_format() {
        let url = "https://raw.githubusercontent.com/user/repo/refs/heads/main/README.md";
        let result = parse_github_raw_url(url).unwrap();

        assert_eq!(result.owner.as_ref(), "user");
        assert_eq!(result.repo.as_ref(), "repo");
        assert_eq!(result.git_ref.as_ref(), "main");
        assert_eq!(result.file_path.as_ref(), "README.md");
    }

    #[test]
    fn test_parse_github_raw_url_refs_tags_format() {
        let url = "https://raw.githubusercontent.com/user/repo/refs/tags/v1.0.0/src/lib.rs";
        let result = parse_github_raw_url(url).unwrap();

        assert_eq!(result.owner.as_ref(), "user");
        assert_eq!(result.repo.as_ref(), "repo");
        assert_eq!(result.git_ref.as_ref(), "v1.0.0");
        assert_eq!(result.file_path.as_ref(), "src/lib.rs");
    }

    #[test]
    fn test_parse_github_raw_url_nested_path() {
        let url = "https://raw.githubusercontent.com/user/repo/dev/docs/api/guide.md";
        let result = parse_github_raw_url(url).unwrap();

        assert_eq!(result.owner.as_ref(), "user");
        assert_eq!(result.repo.as_ref(), "repo");
        assert_eq!(result.git_ref.as_ref(), "dev");
        assert_eq!(result.file_path.as_ref(), "docs/api/guide.md");
    }

    #[test]
    fn test_parse_github_raw_url_commit_hash() {
        let url = "https://raw.githubusercontent.com/user/repo/abc123def456/file.txt";
        let result = parse_github_raw_url(url).unwrap();

        assert_eq!(result.git_ref.as_ref(), "abc123def456");
    }

    #[test]
    fn test_parse_github_raw_url_invalid_too_short() {
        assert!(parse_github_raw_url("https://raw.githubusercontent.com/user/repo").is_none());
        assert!(parse_github_raw_url("https://raw.githubusercontent.com/user/repo/main").is_none());
    }

    #[test]
    fn test_parse_github_raw_url_invalid_wrong_domain() {
        assert!(parse_github_raw_url("https://github.com/user/repo/main/README.md").is_none());
        assert!(parse_github_raw_url("https://raw.example.com/user/repo/main/file").is_none());
    }

    // ==================== 镜像 URL 构建测试 ====================

    #[test]
    fn test_build_release_mirror_url_with_template() {
        temp_env::with_var(
            ENV_GITHUB_RELEASE_MIRROR_TEMPLATE,
            Some("https://mirror.com/<owner>/<repo>/<version>/<asset>"),
            || {
                let url = "https://github.com/rust-lang/rust/releases/download/1.78.0/rustc-1.78.0-x86_64-unknown-linux-gnu.tar.gz";
                let mirror = build_release_mirror_url(url).unwrap();

                assert_eq!(
                    mirror,
                    "https://mirror.com/rust-lang/rust/1.78.0/rustc-1.78.0-x86_64-unknown-linux-gnu.tar.gz"
                );
            },
        );
    }

    #[test]
    fn test_build_release_mirror_url_no_env() {
        temp_env::with_var_unset(ENV_GITHUB_RELEASE_MIRROR_TEMPLATE, || {
            let url = "https://github.com/user/repo/releases/download/v1/file";
            assert!(build_release_mirror_url(url).is_none());
        });
    }

    #[test]
    fn test_build_raw_mirror_url_with_template() {
        temp_env::with_var(
            ENV_GITHUB_RAW_MIRROR_TEMPLATE,
            Some("https://cdn.example.com/<owner>/<repo>/<branch>/<path>"),
            || {
                let url = "https://raw.githubusercontent.com/user/repo/main/src/lib.rs";
                let mirror = build_raw_mirror_url(url).unwrap();

                assert_eq!(mirror, "https://cdn.example.com/user/repo/main/src/lib.rs");
            },
        );
    }

    #[test]
    fn test_build_raw_mirror_url_no_env() {
        temp_env::with_var_unset(ENV_GITHUB_RAW_MIRROR_TEMPLATE, || {
            let url = "https://raw.githubusercontent.com/user/repo/main/file";
            assert!(build_raw_mirror_url(url).is_none());
        });
    }

    #[test]
    fn test_build_mirror_url_from_base() {
        temp_env::with_var(
            ENV_GITHUB_MIRROR_BASE_URL,
            Some("https://mirror.example.com"),
            || {
                let url = "https://github.com/user/repo/releases/download/v1/file.tar.gz";
                let mirror = build_mirror_url_from_base(url).unwrap();

                assert_eq!(
                    mirror,
                    "https://mirror.example.com/github.com/user/repo/releases/download/v1/file.tar.gz"
                );
            },
        );
    }

    /// The pinned-tag template URLs are the ones pacor builds itself, and a
    /// mirror has to reproduce their path exactly — including `refs/tags/<tag>`
    /// and the trailing slash of a base URL that came with one.
    #[test]
    fn test_build_mirror_url_from_base_keeps_pinned_tag_paths() {
        temp_env::with_var(
            ENV_GITHUB_MIRROR_BASE_URL,
            Some("https://mirror.example.com/"),
            || {
                let url = pacor_core_template_url_for_test();
                let mirror = build_mirror_url_from_base(&url).unwrap();

                assert_eq!(
                    mirror,
                    format!(
                        "https://mirror.example.com/raw.githubusercontent.com/pacor-rs/tools/refs/tags/\
                         {}/windows/wix/main.wxs",
                        crate::tools::TEMPLATE_REF
                    )
                );
                // The scheme is dropped, so the mirror host is the only host in
                // the result: a stray `https://` in the middle would make the
                // request go somewhere else entirely.
                assert_eq!(mirror.matches("://").count(), 1, "{mirror}");
            },
        );
    }

    fn pacor_core_template_url_for_test() -> String {
        crate::tools::template_url("windows/wix/main.wxs")
    }

    #[test]
    fn test_build_mirror_url_from_base_no_env() {
        temp_env::with_var_unset(ENV_GITHUB_MIRROR_BASE_URL, || {
            let url = "https://github.com/user/repo/releases/download/v1/file";
            assert!(build_mirror_url_from_base(url).is_none());
        });
    }

    #[test]
    fn test_build_mirror_url_from_template_release() {
        temp_env::with_var(
            ENV_GITHUB_RELEASE_MIRROR_TEMPLATE,
            Some("https://m.com/<owner>/<repo>/<version>/<asset>"),
            || {
                let url = "https://github.com/user/repo/releases/download/v1.0/app.zip";
                let mirror = build_mirror_url_from_template(url).unwrap();

                assert_eq!(mirror, "https://m.com/user/repo/v1.0/app.zip");
            },
        );
    }

    #[test]
    fn test_build_mirror_url_from_template_raw() {
        temp_env::with_var(
            ENV_GITHUB_RAW_MIRROR_TEMPLATE,
            Some("https://cdn.com/<owner>/<repo>/<branch>/<path>"),
            || {
                let url = "https://raw.githubusercontent.com/user/repo/main/config.yml";
                let mirror = build_mirror_url_from_template(url).unwrap();

                assert_eq!(mirror, "https://cdn.com/user/repo/main/config.yml");
            },
        );
    }

    #[test]
    fn test_build_mirror_url_from_template_non_github() {
        let url = "https://example.com/file.zip";
        assert!(build_mirror_url_from_template(url).is_none());
    }

    // ==================== 替代下载信息测试 ====================

    #[test]
    fn test_resolve_mirror_url_with_mirror() {
        temp_env::with_var(
            ENV_GITHUB_MIRROR_BASE_URL,
            Some("https://mirror.test.com"),
            || {
                let url = "https://github.com/user/repo/releases/download/v1/file";
                let result = resolve_mirror_url(url);

                assert!(result.is_some());
                let mirror_url = result.unwrap();
                assert!(mirror_url.starts_with("https://mirror.test.com"));
            },
        );
    }

    #[test]
    fn test_resolve_mirror_url_no_mirror() {
        temp_env::with_vars_unset(
            vec![
                ENV_GITHUB_MIRROR_BASE_URL,
                ENV_GITHUB_RELEASE_MIRROR_TEMPLATE,
                ENV_GITHUB_RAW_MIRROR_TEMPLATE,
            ],
            || {
                let url = "https://github.com/user/repo/releases/download/v1/file";
                assert!(resolve_mirror_url(url).is_none());
            },
        );
    }

    #[test]
    fn test_resolve_mirror_url_non_github_url() {
        temp_env::with_var(
            ENV_GITHUB_MIRROR_BASE_URL,
            Some("https://mirror.test.com"),
            || {
                let url = "https://example.com/file.zip";
                assert!(resolve_mirror_url(url).is_none());
            },
        );
    }

    // ==================== 集成测试 ====================

    #[test]
    fn test_prepare_download_with_mirror() {
        temp_env::with_var(
            ENV_GITHUB_MIRROR_BASE_URL,
            Some("https://mirror.example.com"),
            || {
                let url = "https://github.com/user/repo/releases/download/v1/file";
                let (_, final_url) = prepare_download(url);

                assert!(final_url.contains("mirror.example.com"));
            },
        );
    }

    #[test]
    fn test_prepare_download_without_mirror() {
        temp_env::with_vars_unset(
            vec![
                ENV_GITHUB_MIRROR_BASE_URL,
                ENV_GITHUB_RELEASE_MIRROR_TEMPLATE,
            ],
            || {
                let url = "https://example.com/file.zip";
                let (_, final_url) = prepare_download(url);

                assert_eq!(final_url, url);
            },
        );
    }

    #[test]
    fn test_mirror_priority_template_over_base() {
        temp_env::with_vars(
            vec![
                (ENV_GITHUB_MIRROR_BASE_URL, Some("https://base.com")),
                (
                    ENV_GITHUB_RELEASE_MIRROR_TEMPLATE,
                    Some("https://template.com/<owner>/<repo>/<version>/<asset>"),
                ),
            ],
            || {
                let url = "https://github.com/user/repo/releases/download/v1/file";
                let (_, final_url) = prepare_download(url);

                // 模板优先级高于基础 URL
                assert!(final_url.starts_with("https://template.com"));
            },
        );
    }
}
