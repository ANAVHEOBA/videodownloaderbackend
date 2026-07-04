use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::module::processing::model::DownloadRequestRecord;

#[derive(Debug, Deserialize)]
pub struct ResolveUrlRequest {
    pub url: String,
}

#[derive(Debug, Serialize)]
pub struct ResolveUrlResponse {
    pub request: ProcessingRequestResponse,
}

#[derive(Debug, Serialize)]
pub struct ProcessingRequestResponse {
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
    pub download_url: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub message: String,
}

impl ProcessingRequestResponse {
    pub fn from_record(record: DownloadRequestRecord) -> Self {
        let status = record.status.clone();
        let download_url =
            (status == "resolved").then(|| format!("/processing/{}/download", record.id));

        Self {
            id: record.id,
            provider: record.provider,
            source_url: record.source_url,
            normalized_url: record.normalized_url,
            source_host: record.source_host,
            status,
            title: record.title,
            thumbnail_url: record.thumbnail_url,
            duration_seconds: record.duration_seconds,
            uploader: record.uploader,
            extractor: record.extractor,
            webpage_url: record.webpage_url,
            last_error: record.last_error,
            resolved_at: record.resolved_at,
            download_url,
            created_at: record.created_at,
            updated_at: record.updated_at,
            message: match record.status.as_str() {
                "resolved" => {
                    "request resolved; preview is ready and the download endpoint will fetch the best available media"
                        .to_owned()
                }
                "failed" => "request failed during metadata extraction".to_owned(),
                _ => "request accepted; metadata extraction is in progress".to_owned(),
            },
        }
    }
}
