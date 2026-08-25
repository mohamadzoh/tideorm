use argon2::password_hash::PasswordVerifier;
use serde::{Deserialize, Serialize};
use std::fmt;

/// Hashed string wrapper (one-way hash, e.g., for passwords).
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Hashed {
    /// The hashed value (stored)
    hash: String,
}

impl Hashed {
    /// Create a new hashed value from plain text.
    ///
    /// # Panics
    ///
    /// Panics if Argon2 refuses to hash the input. This is unreachable in practice:
    /// the salt is generated locally and is always well-formed, so the only remaining
    /// failure mode is a plain-text value longer than `u32::MAX` bytes (4 GiB). Use
    /// [`Hashed::try_new`] when the input length is not under your control.
    pub fn new(plain_text: &str) -> Self {
        Self::try_new(plain_text).expect(
            "Argon2 hashing failed; the plain-text value exceeds the Argon2 password length limit",
        )
    }

    /// Create a new hashed value from plain text, reporting Argon2 failures as an error.
    ///
    /// This is the fallible counterpart to [`Hashed::new`]; prefer it whenever the
    /// plain-text value comes from an untrusted or unbounded source.
    pub fn try_new(plain_text: &str) -> crate::error::Result<Self> {
        Ok(Self {
            hash: Self::compute_hash(plain_text)?,
        })
    }

    /// Create from an existing Argon2 hash.
    pub fn from_hash(hash: String) -> Self {
        Self { hash }
    }

    /// Get the hash value
    pub fn hash(&self) -> &str {
        &self.hash
    }

    /// Verify a plain text value against the stored Argon2 hash.
    pub fn verify(&self, plain_text: &str) -> bool {
        let Ok(parsed_hash) = argon2::password_hash::PasswordHash::new(&self.hash) else {
            return false;
        };

        argon2::Argon2::default()
            .verify_password(plain_text.as_bytes(), &parsed_hash)
            .is_ok()
    }

    /// Compute an Argon2 hash suitable for password storage.
    fn compute_hash(input: &str) -> crate::error::Result<String> {
        use argon2::password_hash::{PasswordHasher, SaltString, rand_core::OsRng};

        let salt = SaltString::generate(&mut OsRng);
        argon2::Argon2::default()
            .hash_password(input.as_bytes(), &salt)
            .map(|hash| hash.to_string())
            .map_err(|error| {
                crate::error::Error::internal(format!("Argon2 hashing failed: {error}"))
            })
    }
}

/// `From` cannot report failure, so this delegates to [`Hashed::new`] and inherits its
/// (unreachable in practice) panic. Use [`Hashed::try_new`] for a fallible conversion.
impl From<&str> for Hashed {
    fn from(s: &str) -> Self {
        Self::new(s)
    }
}

/// `From` cannot report failure, so this delegates to [`Hashed::new`] and inherits its
/// (unreachable in practice) panic. Use [`Hashed::try_new`] for a fallible conversion.
impl From<String> for Hashed {
    fn from(s: String) -> Self {
        Self::new(&s)
    }
}

impl fmt::Display for Hashed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "***HASHED***")
    }
}

impl Serialize for Hashed {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str("***HASHED***")
    }
}

impl<'de> Deserialize<'de> for Hashed {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let hash = String::deserialize(deserializer)?;
        if hash == "***HASHED***" {
            return Err(serde::de::Error::custom(
                "Hashed values use a redacted serialization format and cannot be deserialized from ***HASHED***",
            ));
        }

        Ok(Self { hash })
    }
}
