//! Local license verification and the free-tier usage guard.
//!
//! An instance is *licensed* when the environment variable [`LICENSE_KEY_VAR`] contains a key
//! whose hash appears in the compiled-in allow list ([`LICENSE_KEY_HASHES`]).
//! The check runs once at startup and needs no network access.
//!
//! Unlicensed instances may run [`FREE_PACK_LIMIT`] packing operations. After that every packing
//! request is rejected with a message that an active license is required.
//!
//! ## Key format
//!
//! `SORTIT-XXXXXXXX-XXXX-4XXX-YXXX-XXXXXXXXXXXX`: the prefix `SORTIT-` followed by a random
//! (version 4) UUID in upper-case hex, where `Y` is one of `8`, `9`, `A`, `B`. Keys are compared
//! case-insensitively; surrounding whitespace is ignored.
//!
//! ## Hashes
//!
//! Only SHA-512 digests of the normalized keys are compiled into the binary, so the binary (or the
//! source) never reveals a valid key. The keys carry 122 random bits, which makes guessing or
//! brute-forcing a key from its hash infeasible. A fixed domain-separation prefix
//! ([`HASH_DOMAIN`]) ensures the digests cannot be reused from, or matched against, other contexts.
//!
//! Adding a license means appending its hash to `src/license/hashes.rs`; revoking one means removing
//! its hash and shipping a new build. Use `cargo run --example license_keygen` to create keys.

use std::sync::atomic::{AtomicU64, Ordering};

use sha2::{Digest, Sha512};

mod hashes;

pub use hashes::LICENSE_KEY_HASHES;

/// Environment variable holding the license key.
pub const LICENSE_KEY_VAR: &str = "SORT_IT_NOW_LICENSE_KEY";

/// Number of packing operations an unlicensed instance may run before it is locked.
pub const FREE_PACK_LIMIT: u64 = 10;

/// Prefix of every license key.
pub const KEY_PREFIX: &str = "SORTIT-";

/// Domain-separation prefix mixed into every license hash.
pub const HASH_DOMAIN: &str = "sort-it-now:license-key:v1:";

/// Message returned to API and UI clients once the free tier is used up.
pub const LICENSE_REQUIRED_MESSAGE: &str =
    "An active license is required for further use of this instance.";

/// Why an instance is running without a valid license.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnlicensedReason {
    /// No key was configured.
    Missing,
    /// A key was configured but does not match the expected format.
    InvalidFormat,
    /// A well-formed key was configured but it is not in the allow list (unknown or revoked).
    NotRecognized,
}

impl UnlicensedReason {
    /// Machine-readable identifier, used in the `/license` endpoint.
    pub fn code(self) -> &'static str {
        match self {
            UnlicensedReason::Missing => "missing",
            UnlicensedReason::InvalidFormat => "invalid_format",
            UnlicensedReason::NotRecognized => "not_recognized",
        }
    }
}

impl std::fmt::Display for UnlicensedReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            UnlicensedReason::Missing => write!(f, "no license key configured"),
            UnlicensedReason::InvalidFormat => {
                write!(f, "the configured license key has an invalid format")
            }
            UnlicensedReason::NotRecognized => {
                write!(f, "the configured license key is not valid")
            }
        }
    }
}

/// Result of the startup license check.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LicenseStatus {
    Licensed,
    Unlicensed(UnlicensedReason),
}

impl LicenseStatus {
    /// Returns `true` if a valid license key is configured.
    pub fn is_licensed(self) -> bool {
        matches!(self, LicenseStatus::Licensed)
    }
}

/// Normalizes a raw key (trim + upper-case) and checks its format.
///
/// Returns `None` if the key does not follow `SORTIT-<UUIDv4>`.
pub fn normalize_key(raw: &str) -> Option<String> {
    let key = raw.trim().to_ascii_uppercase();
    let uuid = key.strip_prefix(KEY_PREFIX)?;
    let bytes = uuid.as_bytes();
    if bytes.len() != 36 {
        return None;
    }
    for (idx, &byte) in bytes.iter().enumerate() {
        let valid = match idx {
            8 | 13 | 18 | 23 => byte == b'-',
            14 => byte == b'4',
            19 => matches!(byte, b'8' | b'9' | b'A' | b'B'),
            _ => byte.is_ascii_hexdigit(),
        };
        if !valid {
            return None;
        }
    }
    Some(key)
}

/// Computes the lower-case hex SHA-512 digest stored in the allow list for a normalized key.
pub fn hash_key(normalized_key: &str) -> String {
    let mut hasher = Sha512::new();
    hasher.update(HASH_DOMAIN.as_bytes());
    hasher.update(normalized_key.as_bytes());
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Verifies a raw key (as found in the environment) against the compiled-in allow list.
pub fn verify_key(raw: Option<&str>) -> LicenseStatus {
    verify_key_against(raw, LICENSE_KEY_HASHES)
}

/// Verifies a raw key against an explicit allow list (useful for tests and embedders).
pub fn verify_key_against(raw: Option<&str>, allowed_hashes: &[&str]) -> LicenseStatus {
    let Some(raw) = raw.map(str::trim).filter(|value| !value.is_empty()) else {
        return LicenseStatus::Unlicensed(UnlicensedReason::Missing);
    };
    let Some(key) = normalize_key(raw) else {
        return LicenseStatus::Unlicensed(UnlicensedReason::InvalidFormat);
    };
    let digest = hash_key(&key);
    if allowed_hashes
        .iter()
        .any(|allowed| allowed.eq_ignore_ascii_case(&digest))
    {
        LicenseStatus::Licensed
    } else {
        LicenseStatus::Unlicensed(UnlicensedReason::NotRecognized)
    }
}

/// Error returned when an unlicensed instance has exhausted its free packing operations.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LicenseRequired {
    pub limit: u64,
}

impl std::fmt::Display for LicenseRequired {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{LICENSE_REQUIRED_MESSAGE} This unlicensed instance has used all {} free packing \
             operations. Set {LICENSE_KEY_VAR} to a valid license key and restart the instance.",
            self.limit
        )
    }
}

impl std::error::Error for LicenseRequired {}

/// Tracks the license state and the free-tier usage of a running instance.
///
/// The guard is thread-safe; share it via `Arc` between request handlers.
#[derive(Debug)]
pub struct LicenseGuard {
    status: LicenseStatus,
    limit: u64,
    used: AtomicU64,
}

impl LicenseGuard {
    /// Reads [`LICENSE_KEY_VAR`] and verifies it against the compiled-in allow list.
    pub fn from_env() -> Self {
        let raw = match std::env::var(LICENSE_KEY_VAR) {
            Ok(value) => Some(value),
            Err(std::env::VarError::NotPresent) => None,
            Err(err) => {
                eprintln!("⚠️ Could not read {LICENSE_KEY_VAR}: {err}");
                // Present but unreadable (non UTF-8): treat as malformed, not as missing.
                Some(String::from("\u{0}"))
            }
        };
        Self::new(verify_key(raw.as_deref()), FREE_PACK_LIMIT)
    }

    /// Creates a guard for an already determined status and free-tier limit.
    pub fn new(status: LicenseStatus, limit: u64) -> Self {
        Self {
            status,
            limit,
            used: AtomicU64::new(0),
        }
    }

    /// A licensed guard without any usage restriction.
    pub fn licensed() -> Self {
        Self::new(LicenseStatus::Licensed, FREE_PACK_LIMIT)
    }

    /// An unlicensed guard with the default free-tier limit.
    pub fn unlicensed() -> Self {
        Self::new(
            LicenseStatus::Unlicensed(UnlicensedReason::Missing),
            FREE_PACK_LIMIT,
        )
    }

    pub fn status(&self) -> LicenseStatus {
        self.status
    }

    pub fn is_licensed(&self) -> bool {
        self.status.is_licensed()
    }

    /// Free-tier limit of packing operations.
    pub fn limit(&self) -> u64 {
        self.limit
    }

    /// Number of free packing operations used so far.
    pub fn used(&self) -> u64 {
        self.used.load(Ordering::SeqCst).min(self.limit)
    }

    /// Remaining free packing operations, or `None` for licensed instances (unlimited).
    pub fn remaining(&self) -> Option<u64> {
        (!self.is_licensed()).then(|| self.limit - self.used())
    }

    /// Returns `true` if the instance no longer accepts packing requests.
    pub fn is_locked(&self) -> bool {
        !self.is_licensed() && self.used.load(Ordering::SeqCst) >= self.limit
    }

    /// Fails fast without consuming anything if the instance is already locked.
    pub fn ensure_unlocked(&self) -> Result<(), LicenseRequired> {
        if self.is_locked() {
            Err(LicenseRequired { limit: self.limit })
        } else {
            Ok(())
        }
    }

    /// Reserves one packing operation.
    ///
    /// Always succeeds for licensed instances. Unlicensed instances succeed until the free limit is
    /// reached; the reservation is atomic, so concurrent requests can never exceed the limit.
    pub fn acquire(&self) -> Result<(), LicenseRequired> {
        if self.is_licensed() {
            return Ok(());
        }
        self.used
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |used| {
                (used < self.limit).then_some(used + 1)
            })
            .map(|_| ())
            .map_err(|_| LicenseRequired { limit: self.limit })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use std::sync::Arc;

    const SAMPLE_KEY: &str = "SORTIT-3F2504E0-4F89-41D3-9A0C-0305E82C3301";

    #[test]
    fn normalize_accepts_valid_keys_case_insensitively() {
        assert_eq!(normalize_key(SAMPLE_KEY).as_deref(), Some(SAMPLE_KEY));
        assert_eq!(
            normalize_key(&format!("  {}\n", SAMPLE_KEY.to_lowercase())).as_deref(),
            Some(SAMPLE_KEY)
        );
    }

    #[test]
    fn normalize_rejects_malformed_keys() {
        for key in [
            "",
            "3F2504E0-4F89-41D3-9A0C-0305E82C3301", // missing prefix
            "SORTIT-3F2504E0-4F89-11D3-9A0C-0305E82C3301", // not version 4
            "SORTIT-3F2504E0-4F89-41D3-CA0C-0305E82C3301", // wrong variant
            "SORTIT-3F2504E0-4F89-41D3-9A0C-0305E82C330", // too short
            "SORTIT-3F2504E0-4F89-41D3-9A0C-0305E82C33011", // too long
            "SORTIT-3F2504E0X4F89-41D3-9A0C-0305E82C3301", // bad separator
            "SORTIT-3F2504E0-4F89-41D3-9A0C-0305E82C330G", // non-hex
        ] {
            assert_eq!(normalize_key(key), None, "{key:?} should be rejected");
        }
    }

    #[test]
    fn hash_is_long_hex_and_domain_separated() {
        let digest = hash_key(SAMPLE_KEY);
        assert_eq!(digest.len(), 128);
        assert!(digest.bytes().all(|b| b.is_ascii_hexdigit()));
        // Plain SHA-512 of the key must differ from the domain-separated digest.
        let plain: String = Sha512::digest(SAMPLE_KEY.as_bytes())
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        assert_ne!(digest, plain);
    }

    #[test]
    fn verify_key_against_reports_each_state() {
        let allowed = hash_key(SAMPLE_KEY);
        let list = [allowed.as_str()];
        assert_eq!(
            verify_key_against(Some(SAMPLE_KEY), &list),
            LicenseStatus::Licensed
        );
        assert_eq!(
            verify_key_against(Some(&SAMPLE_KEY.to_lowercase()), &list),
            LicenseStatus::Licensed
        );
        assert_eq!(
            verify_key_against(None, &list),
            LicenseStatus::Unlicensed(UnlicensedReason::Missing)
        );
        assert_eq!(
            verify_key_against(Some("   "), &list),
            LicenseStatus::Unlicensed(UnlicensedReason::Missing)
        );
        assert_eq!(
            verify_key_against(Some("not-a-key"), &list),
            LicenseStatus::Unlicensed(UnlicensedReason::InvalidFormat)
        );
        assert_eq!(
            verify_key_against(Some("SORTIT-3F2504E0-4F89-41D3-9A0C-0305E82C3302"), &list),
            LicenseStatus::Unlicensed(UnlicensedReason::NotRecognized)
        );
    }

    #[test]
    fn compiled_hash_list_is_well_formed() {
        assert!(
            LICENSE_KEY_HASHES.len() >= 40,
            "expected at least 40 license hashes"
        );
        let mut seen = HashSet::new();
        for hash in LICENSE_KEY_HASHES {
            assert_eq!(hash.len(), 128, "hash {hash} must be a SHA-512 hex digest");
            assert!(
                hash.bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
                "hash {hash} must be lower-case hex"
            );
            assert!(seen.insert(*hash), "duplicate hash {hash}");
        }
    }

    #[test]
    fn sample_key_is_not_licensed_by_default() {
        assert!(!verify_key(Some(SAMPLE_KEY)).is_licensed());
    }

    #[test]
    fn unlicensed_guard_locks_after_limit() {
        let guard = LicenseGuard::new(LicenseStatus::Unlicensed(UnlicensedReason::Missing), 3);
        assert_eq!(guard.remaining(), Some(3));
        for expected_remaining in [2, 1, 0] {
            assert!(guard.ensure_unlocked().is_ok());
            assert!(guard.acquire().is_ok());
            assert_eq!(guard.remaining(), Some(expected_remaining));
        }
        assert!(guard.is_locked());
        assert_eq!(guard.acquire(), Err(LicenseRequired { limit: 3 }));
        assert_eq!(guard.ensure_unlocked(), Err(LicenseRequired { limit: 3 }));
        assert_eq!(guard.used(), 3);
    }

    #[test]
    fn licensed_guard_is_never_locked() {
        let guard = LicenseGuard::new(LicenseStatus::Licensed, 1);
        for _ in 0..100 {
            assert!(guard.acquire().is_ok());
        }
        assert!(!guard.is_locked());
        assert_eq!(guard.remaining(), None);
    }

    #[test]
    fn concurrent_acquire_never_exceeds_limit() {
        let guard = Arc::new(LicenseGuard::unlicensed());
        let handles: Vec<_> = (0..32)
            .map(|_| {
                let guard = Arc::clone(&guard);
                std::thread::spawn(move || guard.acquire().is_ok())
            })
            .collect();
        let granted = handles
            .into_iter()
            .map(|h| h.join().expect("thread completes"))
            .filter(|ok| *ok)
            .count();
        assert_eq!(granted as u64, FREE_PACK_LIMIT);
        assert!(guard.is_locked());
    }

    #[test]
    fn license_required_message_mentions_env_var() {
        let message = LicenseRequired { limit: 10 }.to_string();
        assert!(message.starts_with(LICENSE_REQUIRED_MESSAGE));
        assert!(message.contains(LICENSE_KEY_VAR));
    }
}
