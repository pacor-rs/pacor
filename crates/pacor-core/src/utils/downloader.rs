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

use std::{
    borrow::Cow,
    fmt,
    io::{self, Read},
    thread,
    time::Duration,
};

use ureq::{
    Agent, Proxy,
    config::ConfigBuilder,
    tls::{RootCerts, TlsConfig, TlsProvider},
    typestate::AgentScope,
};

use anyhow::{Context, Result};

use tracing::{debug, info, warn};

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

/// Windows and macOS verify with their own TLS stack (`native-tls`); Linux has no
/// `native-tls` in its graph and verifies with rustls. Both end up trusting the operating
/// system's roots — that is what `RootCerts::PlatformVerifier` asks for: the native-tls
/// default on Windows and macOS, and `ureq`'s `platform-verifier` feature on Linux.
#[cfg(target_os = "linux")]
const TLS_PROVIDER: TlsProvider = TlsProvider::Rustls;
#[cfg(not(target_os = "linux"))]
const TLS_PROVIDER: TlsProvider = TlsProvider::NativeTls;

fn create_base_agent_builder() -> ConfigBuilder<AgentScope> {
    Agent::config_builder()
        .tls_config(
            TlsConfig::builder()
                .provider(TLS_PROVIDER)
                .root_certs(RootCerts::PlatformVerifier)
                .build(),
        )
        // Statuses are inspected by the download loop rather than turned into
        // errors here: only it knows whether a 416 means "already complete" and
        // whether a 5xx is worth another attempt.
        .http_status_as_error(false)
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

// === Downloading, with resume ===

/// How many times a transfer is attempted before it is reported as failed.
///
/// A proxy or mirror that drops a large file part way through is common enough
/// that a single attempt is not a policy — and every attempt after the first
/// resumes instead of starting over.
const DOWNLOAD_ATTEMPTS: u32 = 4;

/// Pause between attempts, so an overloaded mirror gets room to recover.
const RETRY_DELAY: Duration = Duration::from_secs(2);

/// How much has to arrive before progress is reported again.
///
/// The WiX toolset is nearly 40 MB: a transfer that prints nothing for minutes is
/// indistinguishable from one that has hung.
const PROGRESS_STEP: u64 = 8 * 1024 * 1024;

/// Size of the buffer a response body is read through.
const READ_CHUNK: usize = 64 * 1024;

/// Attempts and pacing of the retry loop.
///
/// A struct rather than constants so tests can exercise the loop without waiting.
#[derive(Debug, Clone, Copy)]
struct RetryPolicy {
    attempts: u32,
    delay: Duration,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            attempts: DOWNLOAD_ATTEMPTS,
            delay: RETRY_DELAY,
        }
    }
}

/// Downloads a file from the given URL, optionally using a configured GitHub mirror.
///
/// An interrupted transfer is resumed rather than restarted: the bytes already
/// received are kept and the next attempt asks for the remainder with a `Range`
/// header. This matters for the WiX toolset, which is nearly 40 MB and is
/// regularly cut short by mirrors and proxies.
pub fn download_file(url: &str) -> Result<Vec<u8>> {
    let (agent, final_url) = prepare_download(url);
    info!(url = %final_url, "Downloading");

    download_with_resume(&agent, &final_url, RetryPolicy::default())
        .with_context(|| format!("failed to download {final_url}"))
}

/// Runs [`fetch_into`] until the file is complete or the attempts run out.
fn download_with_resume(agent: &Agent, url: &str, policy: RetryPolicy) -> Result<Vec<u8>> {
    let attempts = policy.attempts.max(1);
    let mut bytes = Vec::new();
    // Kept across attempts so that a failure can say how far it got, even when
    // the interruption surfaces as a read error instead of a short body.
    let mut expected: Option<u64> = None;
    let mut last_failure = None;

    for attempt in 1..=attempts {
        if attempt > 1 {
            warn!(
                url,
                attempt,
                attempts,
                received = bytes.len(),
                "Retrying the download, keeping what has been received"
            );
            thread::sleep(policy.delay);
        }

        match fetch_into(agent, url, &mut bytes, &mut expected) {
            Ok(()) => {
                debug!(url, bytes = bytes.len(), "Download complete");
                return Ok(bytes);
            }
            Err(failure) if failure.is_retryable() => {
                debug!(url, error = %failure, "Download attempt failed");
                last_failure = Some(failure);
            }
            Err(failure) => return Err(failure.into_error(bytes.len(), expected)),
        }
    }

    Err(last_failure
        .expect("the loop always runs at least once")
        .into_error(bytes.len(), expected))
}

/// Performs one attempt, appending whatever arrives to `bytes`.
///
/// Returns `Ok(())` only once the body is complete, including when the server
/// reports that the requested range starts past the end of the file.
fn fetch_into(
    agent: &Agent,
    url: &str,
    bytes: &mut Vec<u8>,
    expected: &mut Option<u64>,
) -> Result<(), FetchFailure> {
    let resume_from = bytes.len() as u64;

    let mut request = agent.get(url);
    if resume_from > 0 {
        request = request.header("Range", format!("bytes={resume_from}-"));
    }

    let response = request.call().map_err(FetchFailure::Transport)?;
    let status = response.status().as_u16();
    let content_length = header_number(response.headers(), "content-length");
    let content_range = response
        .headers()
        .get("content-range")
        .and_then(|value| value.to_str().ok())
        .and_then(parse_content_range);
    let is_encoded = response
        .headers()
        .get("content-encoding")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|encoding| !encoding.eq_ignore_ascii_case("identity"));

    let expected_total = match status {
        // The whole file, so anything received before is superseded.
        200 => {
            if resume_from > 0 {
                debug!(url, "The server ignored the range request, starting over");
            }
            bytes.clear();
            content_length
        }
        // The remainder, which has to start where the previous attempt stopped.
        206 => {
            let Some(range) = content_range else {
                return Err(FetchFailure::Unexpected(format!(
                    "{url} answered 206 without a Content-Range header"
                )));
            };
            match range.start {
                Some(start) if start < bytes.len() as u64 => {
                    // Resumed earlier than asked: drop the bytes past that point.
                    bytes.truncate(start as usize);
                }
                Some(start) if start == bytes.len() as u64 => {}
                // Resumed past what we hold: appending would splice a body that starts
                // further on, and the file would be missing `start - bytes.len()` bytes in
                // the middle with nothing to show for it. One attempt cannot repair that.
                Some(start) => {
                    let have = bytes.len();
                    return Err(FetchFailure::Unexpected(format!(
                        "{url} resumed at {start} after {have} bytes: the file would have a gap"
                    )));
                }
                None => {
                    return Err(FetchFailure::Unexpected(format!(
                        "{url} answered 206 without a range start"
                    )));
                }
            }
            range
                .total
                .or_else(|| content_length.map(|length| bytes.len() as u64 + length))
        }
        // The range starts past the end: there is nothing left to fetch.
        416 => {
            return match content_range.and_then(|range| range.total) {
                Some(total) if total == bytes.len() as u64 => Ok(()),
                _ => Err(FetchFailure::Unexpected(format!(
                    "{url} answered 416 Range Not Satisfiable"
                ))),
            };
        }
        // A client error will not improve by asking again.
        status if (400..500).contains(&status) => {
            return Err(FetchFailure::Unexpected(format!("{url} answered {status}")));
        }
        status => return Err(FetchFailure::Status(status)),
    };

    *expected = expected_total;

    let mut reader = response.into_body().into_reader();
    let mut chunk = vec![0u8; READ_CHUNK];
    let mut reported = bytes.len() as u64;

    loop {
        let read = reader.read(&mut chunk).map_err(FetchFailure::Read)?;
        if read == 0 {
            break;
        }
        bytes.extend_from_slice(&chunk[..read]);

        if bytes.len() as u64 - reported >= PROGRESS_STEP {
            reported = bytes.len() as u64;
            info!(
                url,
                received = bytes.len(),
                total = ?expected_total,
                "Downloading"
            );
        }
    }

    // A body shorter than its declared length means the peer hung up; the bytes
    // are kept so that the next attempt can resume from them.
    if !is_encoded
        && let Some(total) = expected_total
        && bytes.len() as u64 != total
    {
        return Err(FetchFailure::Truncated {
            received: bytes.len() as u64,
            expected: total,
        });
    }

    Ok(())
}

/// Reads a header that holds a number, ignoring anything unparsable.
fn header_number(headers: &ureq::http::HeaderMap, name: &str) -> Option<u64> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.trim().parse().ok())
}

/// The start offset and total size a `Content-Range` header names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ContentRange {
    /// Offset of the first byte in this response, `None` for `bytes */<total>`.
    start: Option<u64>,

    /// Size of the whole file, `None` when the server does not know it.
    total: Option<u64>,
}

/// Parses `bytes 100-499/1234`, `bytes 0-0/*` and the `bytes */1234` form.
fn parse_content_range(value: &str) -> Option<ContentRange> {
    let range = value.trim().strip_prefix("bytes")?.trim_start();
    let (span, total) = range.split_once('/')?;

    let total = match total.trim() {
        "*" => None,
        total => Some(total.parse().ok()?),
    };

    let span = span.trim();
    if span == "*" {
        return Some(ContentRange { start: None, total });
    }

    let (start, _end) = span.split_once('-')?;
    Some(ContentRange {
        start: Some(start.trim().parse().ok()?),
        total,
    })
}

/// Why one attempt at a transfer failed.
#[derive(Debug)]
enum FetchFailure {
    /// The request itself failed: connection, TLS, timeout, or a status ureq
    /// reports as an error.
    Transport(ureq::Error),

    /// The response body could not be read to the end.
    Read(io::Error),

    /// The peer stopped short; the bytes received so far are kept.
    Truncated { received: u64, expected: u64 },

    /// A response the retry loop cannot do anything about.
    Unexpected(String),

    /// A status that is worth another attempt.
    Status(u16),
}

impl FetchFailure {
    /// Whether another attempt — resuming from what has been received — can help.
    fn is_retryable(&self) -> bool {
        match self {
            Self::Read(_) | Self::Truncated { .. } => true,
            Self::Status(status) => *status >= 500,
            Self::Transport(error) => match error {
                ureq::Error::Io(_)
                | ureq::Error::Timeout(_)
                | ureq::Error::ConnectionFailed
                | ureq::Error::HostNotFound
                | ureq::Error::Protocol(_) => true,
                ureq::Error::StatusCode(status) => *status >= 500,
                _ => false,
            },
            Self::Unexpected(_) => false,
        }
    }

    /// The error a caller sees once the attempts are exhausted.
    ///
    /// `received` and `expected` describe the transfer as a whole: after several
    /// attempts the question that matters is how much of the file arrived, and a
    /// bare `Peer disconnected` says nothing about that.
    fn into_error(self, received: usize, expected: Option<u64>) -> anyhow::Error {
        let progress = match expected {
            Some(total) => format!("the transfer stopped after {received} of {total} bytes"),
            None => format!("the transfer stopped after {received} bytes"),
        };

        match self {
            Self::Transport(error) => anyhow::anyhow!("{progress}: {error}"),
            Self::Read(error) => anyhow::anyhow!("{progress}: {error}"),
            Self::Truncated { .. } => anyhow::anyhow!(progress),
            Self::Unexpected(message) => anyhow::anyhow!(message),
            Self::Status(status) => anyhow::anyhow!("the server answered {status}"),
        }
    }
}

impl fmt::Display for FetchFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Transport(error) => write!(formatter, "{error}"),
            Self::Read(error) => write!(formatter, "{error}"),
            Self::Truncated { received, expected } => {
                write!(formatter, "truncated after {received} of {expected} bytes")
            }
            Self::Unexpected(message) => write!(formatter, "{message}"),
            Self::Status(status) => write!(formatter, "status {status}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        net::{SocketAddr, TcpListener, TcpStream},
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
        thread,
        time::Duration,
    };

    use std::io::Write as _;

    use super::*;

    // ==================== Release URL parsing tests ====================

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

    // ==================== Raw URL parsing tests ====================

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

    // ==================== Mirror URL construction tests ====================

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

    // ==================== Alternative download source tests ====================

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

    // ==================== Integration tests ====================

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

                // The template takes precedence over the base URL
                assert!(final_url.starts_with("https://template.com"));
            },
        );
    }

    // ==================== Resumed transfer tests ====================

    /// A one-file HTTP server, so the resume logic is exercised against a real
    /// socket instead of a mock.
    struct TestServer {
        address: SocketAddr,
        requests: Arc<AtomicUsize>,
        _handle: thread::JoinHandle<()>,
    }

    /// How [`TestServer`] answers a request.
    #[derive(Debug, Clone, Copy)]
    enum Behavior {
        /// Honours `Range` and always sends the whole requested span.
        Complete,
        /// The first response stops after `bytes`; later ones honour `Range`.
        TruncateFirstOnce { bytes: usize },
        /// Like [`Behavior::TruncateFirstOnce`], but later responses ignore
        /// `Range` and start from the beginning again.
        TruncateFirstOnceIgnoringRange { bytes: usize },
        /// The first response stops after `bytes`; the second answers the
        /// range request from `bytes` further on, leaving a gap.
        TruncateFirstOnceThenSkip { bytes: usize, skip: usize },
        /// Every response stops after `bytes`, honouring `Range`, so each
        /// attempt makes a little progress.
        AlwaysTruncate { bytes: usize },
        /// Every response carries this status code.
        Status(u16),
    }

    impl TestServer {
        /// Serves `body` to at most `max_requests` connections.
        fn start(body: Vec<u8>, behavior: Behavior, max_requests: usize) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").expect("bind a loopback port");
            let address = listener.local_addr().expect("read back the port");
            let requests = Arc::new(AtomicUsize::new(0));
            let counter = Arc::clone(&requests);

            let handle = thread::spawn(move || {
                for _ in 0..max_requests {
                    let Ok((mut stream, _)) = listener.accept() else {
                        return;
                    };
                    let request_index = counter.fetch_add(1, Ordering::SeqCst) + 1;

                    let mut head = Vec::new();
                    let mut byte = [0u8; 1];
                    while !head.ends_with(b"\r\n\r\n") {
                        match stream.read(&mut byte) {
                            Ok(1) => head.push(byte[0]),
                            _ => break,
                        }
                    }

                    respond(
                        &mut stream,
                        &body,
                        behavior,
                        request_index,
                        parse_request_range(&String::from_utf8_lossy(&head)),
                    );
                }
            });

            Self {
                address,
                requests,
                _handle: handle,
            }
        }

        fn url(&self) -> String {
            format!("http://{}/file.bin", self.address)
        }

        fn requests(&self) -> usize {
            self.requests.load(Ordering::SeqCst)
        }
    }

    /// The offset a `Range: bytes=<start>-` request asks for.
    fn parse_request_range(head: &str) -> usize {
        head.lines()
            .map(str::to_ascii_lowercase)
            .find_map(|line| {
                line.strip_prefix("range: bytes=")
                    .and_then(|value| value.split('-').next())
                    .and_then(|start| start.trim().parse().ok())
            })
            .unwrap_or(0)
    }

    /// Answers one request, declaring the full length even when it then stops
    /// early — which is exactly what a dropped transfer looks like.
    fn respond(
        stream: &mut TcpStream,
        body: &[u8],
        behavior: Behavior,
        request_index: usize,
        requested_start: usize,
    ) {
        if let Behavior::Status(code) = behavior {
            let response =
                format!("HTTP/1.1 {code} Nope\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.flush();
            return;
        }

        let honours_range = !matches!(behavior, Behavior::TruncateFirstOnceIgnoringRange { .. });
        let mut start = if honours_range {
            requested_start.min(body.len())
        } else {
            0
        };
        if let Behavior::TruncateFirstOnceThenSkip { bytes, skip } = behavior
            && request_index > 1
        {
            start = (start.max(bytes) + skip).min(body.len());
        }
        let slice = &body[start..];

        let truncate_after = match behavior {
            Behavior::TruncateFirstOnce { bytes } if request_index == 1 => Some(bytes),
            Behavior::TruncateFirstOnceIgnoringRange { bytes } if request_index == 1 => Some(bytes),
            Behavior::TruncateFirstOnceThenSkip { bytes, .. } if request_index == 1 => Some(bytes),
            Behavior::AlwaysTruncate { bytes } => Some(bytes),
            _ => None,
        };

        let mut response = if start == 0 {
            format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n", body.len())
        } else {
            format!(
                "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {}-{}/{}\r\n",
                slice.len(),
                start,
                body.len() - 1,
                body.len()
            )
        };
        response.push_str("Connection: close\r\n\r\n");

        let _ = stream.write_all(response.as_bytes());
        let to_send = truncate_after.map_or(slice.len(), |limit| limit.min(slice.len()));
        let _ = stream.write_all(&slice[..to_send]);
        let _ = stream.flush();
        // Dropping the stream closes the connection, which is the failure the
        // resume logic is there to survive.
    }

    /// The client used by the tests: no proxy, no mirror, no retry delay.
    fn test_agent() -> Agent {
        create_base_agent_builder().build().into()
    }

    fn test_policy(attempts: u32) -> RetryPolicy {
        RetryPolicy {
            attempts,
            delay: Duration::ZERO,
        }
    }

    fn body_of(len: usize) -> Vec<u8> {
        (0..len).map(|index| (index % 251) as u8).collect()
    }

    /// A server that resumes past the bytes already received would leave a hole in the middle
    /// of the file. Appending the remainder anyway produced an artifact that looks complete —
    /// plausible length, wrong contents — so the transfer has to fail instead.
    #[test]
    fn a_resume_that_skips_bytes_is_refused() {
        let body = body_of(300_000);
        let server = TestServer::start(
            body.clone(),
            Behavior::TruncateFirstOnceThenSkip {
                bytes: 100_000,
                skip: 1_024,
            },
            4,
        );

        let error = download_with_resume(&test_agent(), &server.url(), test_policy(3))
            .expect_err("a gap in the middle of the file cannot be accepted")
            .to_string();

        assert!(error.contains("gap"), "{error}");
        assert_eq!(server.requests(), 2, "the second answer is unusable");
    }
    #[test]
    fn a_dropped_transfer_resumes_from_where_it_stopped() {
        let body = body_of(300_000);
        let server = TestServer::start(
            body.clone(),
            Behavior::TruncateFirstOnce { bytes: 100_000 },
            4,
        );

        let downloaded = download_with_resume(&test_agent(), &server.url(), test_policy(3))
            .expect("the second attempt must finish the file");

        assert_eq!(downloaded.len(), body.len());
        assert_eq!(downloaded, body, "the resumed bytes must line up");
        assert_eq!(
            server.requests(),
            2,
            "the download should have resumed instead of restarting"
        );
    }

    /// A server that ignores `Range` cannot be resumed against, so the retry has
    /// to start over — and still end up with the right bytes.
    #[test]
    fn a_server_that_ignores_range_still_yields_the_file() {
        let body = body_of(120_000);
        let server = TestServer::start(
            body.clone(),
            Behavior::TruncateFirstOnceIgnoringRange { bytes: 40_000 },
            4,
        );

        let downloaded = download_with_resume(&test_agent(), &server.url(), test_policy(3))
            .expect("the second attempt must finish the file");

        assert_eq!(downloaded, body);
    }

    #[test]
    fn attempts_that_never_finish_report_how_far_they_got() {
        let body = body_of(50_000);
        let server = TestServer::start(body, Behavior::AlwaysTruncate { bytes: 1_000 }, 5);

        let error = download_with_resume(&test_agent(), &server.url(), test_policy(3))
            .expect_err("three truncated attempts cannot complete the file")
            .to_string();

        assert!(
            error.contains("3000") && error.contains("50000"),
            "the error should say how much arrived: {error}"
        );
        assert_eq!(server.requests(), 3, "every attempt should have been made");
    }

    /// A 404 is not worth another attempt, and retrying it would only make a
    /// wrong URL slower to report.
    #[test]
    fn a_client_error_is_not_retried() {
        let server = TestServer::start(body_of(10), Behavior::Status(404), 4);

        let error = download_with_resume(&test_agent(), &server.url(), test_policy(3))
            .expect_err("a 404 must fail")
            .to_string();

        assert!(error.contains("404"), "{error}");
        assert_eq!(server.requests(), 1, "a 404 must not be retried");
    }

    #[test]
    fn a_complete_transfer_takes_a_single_request() {
        let body = body_of(64_000);
        let server = TestServer::start(body.clone(), Behavior::Complete, 2);

        let downloaded = download_with_resume(&test_agent(), &server.url(), test_policy(3))
            .expect("the file must arrive");

        assert_eq!(downloaded, body);
        assert_eq!(server.requests(), 1);
    }

    #[test]
    fn content_range_headers_are_parsed() {
        assert_eq!(
            parse_content_range("bytes 100-499/1234"),
            Some(ContentRange {
                start: Some(100),
                total: Some(1234)
            })
        );
        assert_eq!(
            parse_content_range(" bytes 0-0/* "),
            Some(ContentRange {
                start: Some(0),
                total: None
            })
        );
        assert_eq!(
            parse_content_range("bytes */1234"),
            Some(ContentRange {
                start: None,
                total: Some(1234)
            })
        );
        assert_eq!(parse_content_range("items 0-1/2"), None);
        assert_eq!(parse_content_range("bytes 0-1"), None);
        assert_eq!(parse_content_range(""), None);
    }
}
