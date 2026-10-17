//! What pacor can learn about the certificate an artifact carries.
//!
//! Shared by the two signers: APKs are signed with `apksigner` and Android App
//! Bundles with `jarsigner`, but the question they answer is the same — is this
//! artifact signed with a key that can actually be published?

/// The Android debug keystore's certificate subject.
///
/// An artifact signed with it cannot be published: Google Play rejects it, and a
/// later properly signed release cannot be installed over it. Flutter's own
/// project template points release builds at the debug keystore, which makes
/// this the default outcome for a project that never configured signing.
const DEBUG_CERTIFICATE_MARKER: &str = "CN=Android Debug";

/// What pacor could learn about who signed an artifact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Signer {
    /// Signed with the well-known Android debug certificate.
    Debug(String),

    /// Signed with some other certificate.
    Named(String),

    /// The signing tool could not verify the artifact (for example it is unsigned).
    Unverified(String),

    /// Nothing could be inspected: no tool, or no certificate in the output.
    Unknown,
}

impl Signer {
    /// Classifies a certificate subject read out of a signing tool.
    pub(super) fn from_subject(subject: String) -> Self {
        if is_debug_certificate(&subject) {
            Self::Debug(subject)
        } else {
            Self::Named(subject)
        }
    }
}

/// Whether a certificate subject belongs to the Android debug keystore.
pub(super) fn is_debug_certificate(subject: &str) -> bool {
    subject.contains(DEBUG_CERTIFICATE_MARKER)
}

/// First non-empty line, trimmed — used to summarise tool output.
pub(super) fn first_line(text: &str) -> Option<String> {
    text.lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(ToString::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_debug_certificate_is_recognised() {
        assert!(is_debug_certificate("CN=Android Debug, O=Android, C=US"));
        assert!(!is_debug_certificate(
            "CN=My Upload Key, OU=Mobile, O=Example Ltd, C=CN"
        ));
    }

    #[test]
    fn subjects_are_classified() {
        assert_eq!(
            Signer::from_subject("CN=Android Debug, O=Android, C=US".to_string()),
            Signer::Debug("CN=Android Debug, O=Android, C=US".to_string())
        );
        assert_eq!(
            Signer::from_subject("CN=Upload".to_string()),
            Signer::Named("CN=Upload".to_string())
        );
    }

    #[test]
    fn the_first_line_summarises_tool_output() {
        assert_eq!(
            first_line("\n  jar is unsigned. \nmore\n"),
            Some("jar is unsigned.".to_string())
        );
        assert_eq!(first_line("   \n\n"), None);
    }
}
