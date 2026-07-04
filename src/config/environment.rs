use std::{env, path::PathBuf, str::FromStr};

use anyhow::{Context, Result, bail};

#[derive(Debug, Clone)]
pub struct Environment {
    pub app_env: String,
    pub host: String,
    pub port: u16,
    pub public_base_url: String,
    pub rust_log: String,
    pub database_url: String,
    pub database_max_connections: u32,
    pub database_min_connections: u32,
    pub database_acquire_timeout_seconds: u64,
    pub database_run_migrations: bool,
    pub cors_allowed_origins: Vec<String>,
    pub download_token_secret: String,
    pub download_token_ttl_seconds: i64,
    pub max_url_length: usize,
    pub max_download_bytes: u64,
    pub max_video_duration_seconds: u64,
    pub upstream_connect_timeout_seconds: u64,
    pub upstream_request_timeout_seconds: u64,
    pub upstream_max_redirects: usize,
    pub upstream_user_agent: String,
    pub stream_chunk_size_bytes: usize,
    pub ytdlp_binary_path: PathBuf,
    pub ytdlp_metadata_timeout_seconds: u64,
    pub ytdlp_download_timeout_seconds: u64,
    pub download_retention_seconds: u64,
    pub rate_limit_requests_per_minute: u32,
    pub turnstile_site_key: Option<String>,
    pub turnstile_secret_key: Option<String>,
    pub enable_tiktok: bool,
    pub enable_instagram: bool,
    pub enable_youtube: bool,
    pub enable_twitter: bool,
    pub enable_facebook: bool,
    pub enable_pinterest: bool,
    pub enable_capcut: bool,
    pub enable_telegram: bool,
    pub enable_spotify: bool,
    pub enable_soundcloud: bool,
    pub cookies_file_path: Option<PathBuf>,
    pub ffmpeg_enabled: bool,
    pub ffmpeg_binary_path: String,
    pub temp_dir: PathBuf,
}

impl Environment {
    pub fn load() -> Result<Self> {
        let cors_allowed_origins = read_var("ALLOWED_ORIGINS")?
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
            .collect::<Vec<_>>();

        if cors_allowed_origins.is_empty() {
            bail!("ALLOWED_ORIGINS must contain at least one origin");
        }

        Ok(Self {
            app_env: read_var("APP_ENV")?,
            host: read_var("HOST")?,
            port: read_parse("PORT")?,
            public_base_url: read_var("PUBLIC_BASE_URL")?,
            rust_log: read_var("RUST_LOG")?,
            database_url: read_var("DATABASE_URL")?,
            database_max_connections: read_parse("DATABASE_MAX_CONNECTIONS")?,
            database_min_connections: read_parse("DATABASE_MIN_CONNECTIONS")?,
            database_acquire_timeout_seconds: read_parse("DATABASE_ACQUIRE_TIMEOUT_SECONDS")?,
            database_run_migrations: read_parse("DATABASE_RUN_MIGRATIONS")?,
            cors_allowed_origins,
            download_token_secret: read_var("DOWNLOAD_TOKEN_SECRET")?,
            download_token_ttl_seconds: read_parse("DOWNLOAD_TOKEN_TTL_SECONDS")?,
            max_url_length: read_parse("MAX_URL_LENGTH")?,
            max_download_bytes: read_parse("MAX_DOWNLOAD_BYTES")?,
            max_video_duration_seconds: read_parse("MAX_VIDEO_DURATION_SECONDS")?,
            upstream_connect_timeout_seconds: read_parse("UPSTREAM_CONNECT_TIMEOUT_SECONDS")?,
            upstream_request_timeout_seconds: read_parse("UPSTREAM_REQUEST_TIMEOUT_SECONDS")?,
            upstream_max_redirects: read_parse("UPSTREAM_MAX_REDIRECTS")?,
            upstream_user_agent: read_var("UPSTREAM_USER_AGENT")?,
            stream_chunk_size_bytes: read_parse("STREAM_CHUNK_SIZE_BYTES")?,
            ytdlp_binary_path: PathBuf::from(read_var("YTDLP_BINARY_PATH")?),
            ytdlp_metadata_timeout_seconds: read_parse("YTDLP_METADATA_TIMEOUT_SECONDS")?,
            ytdlp_download_timeout_seconds: read_parse("YTDLP_DOWNLOAD_TIMEOUT_SECONDS")?,
            download_retention_seconds: read_parse("DOWNLOAD_RETENTION_SECONDS")?,
            rate_limit_requests_per_minute: read_parse("RATE_LIMIT_REQUESTS_PER_MINUTE")?,
            turnstile_site_key: read_optional_var("TURNSTILE_SITE_KEY"),
            turnstile_secret_key: read_optional_var("TURNSTILE_SECRET_KEY"),
            enable_tiktok: read_parse("ENABLE_TIKTOK")?,
            enable_instagram: read_parse("ENABLE_INSTAGRAM")?,
            enable_youtube: read_parse("ENABLE_YOUTUBE")?,
            enable_twitter: read_parse("ENABLE_TWITTER")?,
            enable_facebook: read_parse("ENABLE_FACEBOOK")?,
            enable_pinterest: read_parse("ENABLE_PINTEREST")?,
            enable_capcut: read_parse("ENABLE_CAPCUT")?,
            enable_telegram: read_parse("ENABLE_TELEGRAM")?,
            enable_spotify: read_parse("ENABLE_SPOTIFY")?,
            enable_soundcloud: read_parse("ENABLE_SOUNDCLOUD")?,
            cookies_file_path: read_optional_var("COOKIES_FILE_PATH").map(PathBuf::from),
            ffmpeg_enabled: read_parse("FFMPEG_ENABLED")?,
            ffmpeg_binary_path: read_var("FFMPEG_BINARY_PATH")?,
            temp_dir: PathBuf::from(read_var("TEMP_DIR")?),
        })
    }
}

fn read_var(key: &str) -> Result<String> {
    env::var(key).with_context(|| format!("missing required environment variable `{key}`"))
}

fn read_optional_var(key: &str) -> Option<String> {
    match env::var(key) {
        Ok(value) if !value.trim().is_empty() => Some(value),
        _ => None,
    }
}

fn read_parse<T>(key: &str) -> Result<T>
where
    T: FromStr,
    T::Err: std::fmt::Display,
{
    let raw = read_var(key)?;
    raw.parse::<T>()
        .map_err(|error| anyhow::anyhow!("invalid `{key}` value `{raw}`: {error}"))
}
