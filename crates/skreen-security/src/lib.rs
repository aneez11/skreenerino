pub mod consent;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum SecurityError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("device fingerprint mismatch for {device}: expected {expected}, got {actual}")]
    FingerprintMismatch { device: String, expected: String, actual: String },
    #[error("invalid PIN")]
    InvalidPin,
}

pub fn fingerprint_of(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceIdentity {
    pub device_id: String,
    pub public_key: Vec<u8>,
    pub fingerprint: String,
}

impl DeviceIdentity {
    pub fn generate(device_id: impl Into<String>) -> Self {
        let mut key = vec![0u8; 32];
        getrandom::getrandom(&mut key).expect("os rng");
        let fingerprint = fingerprint_of(&key);
        Self { device_id: device_id.into(), public_key: key, fingerprint }
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct TofuStore {
    known: BTreeMap<String, String>,
}

impl TofuStore {
    pub fn load(path: &Path) -> Result<Self, SecurityError> {
        if !path.exists() { return Ok(Self::default()); }
        Ok(serde_json::from_str(&std::fs::read_to_string(path)?)?)
    }

    pub fn save(&self, path: &Path) -> Result<(), SecurityError> {
        if let Some(parent) = path.parent() { std::fs::create_dir_all(parent)?; }
        std::fs::write(path, serde_json::to_string_pretty(self)?)?;
        Ok(())
    }

    /// Trust-on-first-use check. First contact pins the fingerprint; later contacts must match.
    pub fn verify_or_pin(&mut self, device_id: &str, fingerprint: &str) -> Result<bool, SecurityError> {
        match self.known.get(device_id) {
            None => {
                self.known.insert(device_id.to_string(), fingerprint.to_string());
                Ok(true)
            }
            Some(expected) if expected == fingerprint => Ok(true),
            Some(expected) => Err(SecurityError::FingerprintMismatch {
                device: device_id.to_string(),
                expected: expected.clone(),
                actual: fingerprint.to_string(),
            }),
        }
    }
}

pub fn default_tofu_path() -> PathBuf {
    std::env::var_os("SKREENERINO_CONFIG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(".skreenerino"))
        .join("trusted_devices.json")
}

#[derive(Debug, Clone)]
pub struct PairingPin {
    digits: String,
}

impl PairingPin {
    pub fn generate() -> Self {
        let mut raw = [0u8; 4];
        getrandom::getrandom(&mut raw).expect("os rng");
        let value = u32::from_le_bytes(raw) % 1_000_000;
        Self { digits: format!("{value:06}") }
    }

    pub fn from_str(digits: &str) -> Option<Self> {
        let valid = digits.len() == 6 && digits.chars().all(|c| c.is_ascii_digit());
        valid.then(|| Self { digits: digits.to_string() })
    }

    pub fn as_str(&self) -> &str { &self.digits }

    pub fn verify(&self, presented: &str) -> Result<(), SecurityError> {
        if constant_time_eq(self.digits.as_bytes(), presented.as_bytes()) {
            Ok(())
        } else {
            Err(SecurityError::InvalidPin)
        }
    }
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() { return false; }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b) { diff |= x ^ y; }
    diff == 0
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Consent {
    Accepted,
    Rejected,
    Expired,
}

#[derive(Debug, Clone)]
pub struct ConsentRequest {
    pub requester: String,
    pub kind: ConsentKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsentKind { Push, Pull }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tofu_pins_then_rejects_change() {
        let mut store = TofuStore::default();
        assert!(store.verify_or_pin("bob", "aa").unwrap());
        assert!(store.verify_or_pin("bob", "aa").unwrap());
        assert!(store.verify_or_pin("bob", "bb").is_err());
    }

    #[test]
    fn pin_round_trip_and_reject() {
        let pin = PairingPin::from_str("123456").unwrap();
        assert!(pin.verify("123456").is_ok());
        assert!(pin.verify("123457").is_err());
        assert!(PairingPin::from_str("12345").is_none());
    }

    #[test]
    fn generated_pin_is_six_digits() {
        let pin = PairingPin::generate();
        assert_eq!(pin.as_str().len(), 6);
        assert!(pin.as_str().chars().all(|c| c.is_ascii_digit()));
    }
}


