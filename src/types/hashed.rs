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
    /// Create a new hashed value from plain text
    pub fn new(plain_text: &str) -> Self {
        Self {
            hash: Self::compute_hash(plain_text),
        }
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
    fn compute_hash(input: &str) -> String {
        use argon2::password_hash::{PasswordHasher, SaltString, rand_core::OsRng};

        let salt = SaltString::generate(&mut OsRng);
        argon2::Argon2::default()
            .hash_password(input.as_bytes(), &salt)
            .map(|hash| hash.to_string())
            .unwrap()
    }
}

impl From<&str> for Hashed {
    fn from(s: &str) -> Self {
        Self::new(s)
    }
}

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
