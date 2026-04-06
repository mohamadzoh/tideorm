#[tideorm::model(table = "config_path_match_users")]
pub(crate) struct ConfigPathMatchUser {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    name: String,
}

#[tideorm::model(table = "config_path_match_posts")]
pub(crate) struct ConfigPathMatchPost {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    title: String,
}
