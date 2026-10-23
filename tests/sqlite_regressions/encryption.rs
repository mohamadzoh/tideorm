//! Encrypted columns in queries: decrypted wherever a projection reads them, and
//! refused wherever SQL would have to compare ciphertext.

#[path = "encryption/aliases_and_aggregates.rs"]
mod aliases_and_aggregates;

#[cfg(feature = "fulltext")]
#[path = "encryption/fulltext.rs"]
mod fulltext;

#[path = "encryption/joins.rs"]
mod joins;

#[path = "encryption/self_joins_and_windows.rs"]
mod self_joins_and_windows;

#[path = "encryption/unions.rs"]
mod unions;

#[path = "encryption/wildcards_and_aliases.rs"]
mod wildcards_and_aliases;
