//! Models that break the model rules of `../../tideorm.toml`: `cargo check`
//! must fail, naming every broken rule.

use tideorm::prelude::*;

#[tideorm::model(table = "accounts")]
pub struct Account {
    #[tideorm(primary_key, auto_increment)]
    pub id: i64,
    pub email: String,
    pub password: String,
    pub settings: JsonValue,
    pub meta: JsonValue,
    pub nickname: String,
}

/// Exempt from `max_columns`.
#[tideorm::model(table = "legacies")]
pub struct Legacy {
    #[tideorm(primary_key, auto_increment)]
    pub id: i64,
    pub a: String,
    pub b: String,
    pub c: String,
    pub d: String,
    pub e: String,
}

#[tideorm::model(table = "tokens")]
pub struct Token {
    #[tideorm(primary_key)]
    pub id: String,
}

#[tideorm::model(table = "BadNames")]
pub struct BadName {
    #[tideorm(primary_key, auto_increment)]
    pub id: i64,
}

#[tideorm::model(table = "authors")]
pub struct Author {
    #[tideorm(primary_key, auto_increment)]
    pub id: i64,
    #[tideorm(has_many = "Book", foreign_key = "author_id")]
    pub books: HasMany<Book>,
}

#[tideorm::model(table = "books")]
#[index("author_id")]
pub struct Book {
    #[tideorm(primary_key, auto_increment)]
    pub id: i64,
    pub author_id: i64,
    #[tideorm(has_many = "Chapter", foreign_key = "book_id")]
    pub chapters: HasMany<Chapter>,
}

/// `book_id` leads no index.
#[tideorm::model(table = "chapters")]
pub struct Chapter {
    #[tideorm(primary_key, auto_increment)]
    pub id: i64,
    pub book_id: i64,
}

#[tideorm::model(table = "teams")]
#[index("captain_id")]
pub struct Team {
    #[tideorm(primary_key, auto_increment)]
    pub id: i64,
    pub captain_id: i64,
    #[tideorm(has_many = "Member", foreign_key = "team_id")]
    pub members: HasMany<Member>,
}

#[tideorm::model(table = "members")]
#[index("team_id")]
pub struct Member {
    #[tideorm(primary_key, auto_increment)]
    pub id: i64,
    pub team_id: i64,
    #[tideorm(has_many = "Team", foreign_key = "captain_id")]
    pub captained: HasMany<Team>,
}

fn main() {}
