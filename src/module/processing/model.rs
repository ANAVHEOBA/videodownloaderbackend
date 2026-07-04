use chrono::{DateTime, Utc};
use sqlx::FromRow;
use uuid::Uuid;

pub const PROVIDER_TIKTOK: &str = "tiktok";
pub const PROVIDER_YOUTUBE: &str = "youtube";
pub const PROVIDER_INSTAGRAM: &str = "instagram";
pub const PROVIDER_TWITTER: &str = "twitter";
pub const PROVIDER_FACEBOOK: &str = "facebook";
pub const PROVIDER_PINTEREST: &str = "pinterest";
pub const PROVIDER_CAPCUT: &str = "capcut";
pub const PROVIDER_TELEGRAM: &str = "telegram";
pub const PROVIDER_SPOTIFY: &str = "spotify";
pub const PROVIDER_SOUNDCLOUD: &str = "soundcloud";
pub const DOWNLOAD_STATUS_PENDING: &str = "pending";
pub const DOWNLOAD_STATUS_RESOLVED: &str = "resolved";
pub const DOWNLOAD_STATUS_FAILED: &str = "failed";

#[derive(Debug, Clone, FromRow)]
pub struct DownloadRequestRecord {
    pub id: Uuid,
    pub provider: String,
    pub source_url: String,
    pub normalized_url: String,
    pub source_host: String,
    pub status: String,
    pub title: Option<String>,
    pub thumbnail_url: Option<String>,
    pub duration_seconds: Option<i64>,
    pub uploader: Option<String>,
    pub extractor: Option<String>,
    pub webpage_url: Option<String>,
    pub last_error: Option<String>,
    pub resolved_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct NewDownloadRequest {
    pub id: Uuid,
    pub provider: String,
    pub source_url: String,
    pub normalized_url: String,
    pub source_host: String,
    pub status: String,
}

#[derive(Debug, Clone)]
pub struct ResolvedDownloadRequest {
    pub status: String,
    pub title: Option<String>,
    pub thumbnail_url: Option<String>,
    pub duration_seconds: Option<i64>,
    pub uploader: Option<String>,
    pub extractor: Option<String>,
    pub webpage_url: Option<String>,
}
