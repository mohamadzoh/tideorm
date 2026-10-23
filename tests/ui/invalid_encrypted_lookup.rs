// Every write stores a fresh ciphertext: a unique index over an encrypted
// column enforces nothing, and a full-text index over it matches nothing.
#[tideorm::model(table = "members", encrypted = "email,notes", searchable = "notes")]
struct Member {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    #[unique_index]
    email: String,
    notes: String,
}

fn main() {}
