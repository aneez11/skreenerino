//! Certificate fingerprinting used for trust-on-first-use pinning.

use sha2::{Digest, Sha256};

/// Lowercase hex SHA-256 of a DER-encoded certificate.
pub fn fingerprint_hex(der: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(der);
    hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Outcome of checking a peer certificate against a stored pin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PinDecision {
    /// No pin existed; the caller should store this fingerprint.
    FirstContact(String),
    /// The certificate matches the stored pin.
    Trusted,
    /// A pin exists and this certificate is different.
    Mismatch { expected: String, actual: String },
}

pub fn check_pin(stored: Option<&str>, presented: &[u8]) -> PinDecision {
    let actual = fingerprint_hex(presented);
    match stored {
        None => PinDecision::FirstContact(actual),
        Some(expected) if expected.eq_ignore_ascii_case(&actual) => PinDecision::Trusted,
        Some(expected) => PinDecision::Mismatch {
            expected: expected.to_string(),
            actual,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprint_is_64_hex_chars() {
        let fp = fingerprint_hex(b"some-certificate-bytes");
        assert_eq!(fp.len(), 64);
        assert!(fp.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn fingerprint_changes_with_content() {
        assert_ne!(fingerprint_hex(b"a"), fingerprint_hex(b"b"));
    }

    #[test]
    fn first_contact_then_trusted_then_mismatch() {
        let cert = b"cert-1";
        let PinDecision::FirstContact(pinned) = check_pin(None, cert) else {
            panic!("expected first contact");
        };
        assert_eq!(check_pin(Some(&pinned), cert), PinDecision::Trusted);
        assert!(matches!(
            check_pin(Some(&pinned), b"cert-2"),
            PinDecision::Mismatch { .. }
        ));
    }

    #[test]
    fn comparison_is_case_insensitive() {
        let fp = fingerprint_hex(b"cert");
        assert_eq!(
            check_pin(Some(&fp.to_uppercase()), b"cert"),
            PinDecision::Trusted
        );
    }
}
