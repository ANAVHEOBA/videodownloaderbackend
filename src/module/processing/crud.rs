use sqlx::query_as;

use crate::{
    config::db::DbPool,
    module::processing::model::{
        DownloadRequestRecord, NewDownloadRequest, ResolvedDownloadRequest,
    },
};

pub async fn insert_download_request(
    db: &DbPool,
    new_request: &NewDownloadRequest,
) -> Result<DownloadRequestRecord, sqlx::Error> {
    query_as::<_, DownloadRequestRecord>(
        r#"
        INSERT INTO download_requests (
            id,
            provider,
            source_url,
            normalized_url,
            source_host,
            status
        )
        VALUES ($1, $2, $3, $4, $5, $6)
        RETURNING
            id,
            provider,
            source_url,
            normalized_url,
            source_host,
            status,
            title,
            thumbnail_url,
            duration_seconds,
            uploader,
            extractor,
            webpage_url,
            last_error,
            resolved_at,
            created_at,
            updated_at
        "#,
    )
    .bind(new_request.id)
    .bind(&new_request.provider)
    .bind(&new_request.source_url)
    .bind(&new_request.normalized_url)
    .bind(&new_request.source_host)
    .bind(&new_request.status)
    .fetch_one(db)
    .await
}

pub async fn mark_download_request_resolved(
    db: &DbPool,
    request_id: uuid::Uuid,
    resolved: &ResolvedDownloadRequest,
) -> Result<DownloadRequestRecord, sqlx::Error> {
    query_as::<_, DownloadRequestRecord>(
        r#"
        UPDATE download_requests
        SET
            status = $2,
            title = $3,
            thumbnail_url = $4,
            duration_seconds = $5,
            uploader = $6,
            extractor = $7,
            webpage_url = $8,
            last_error = NULL,
            resolved_at = NOW(),
            updated_at = NOW()
        WHERE id = $1
        RETURNING
            id,
            provider,
            source_url,
            normalized_url,
            source_host,
            status,
            title,
            thumbnail_url,
            duration_seconds,
            uploader,
            extractor,
            webpage_url,
            last_error,
            resolved_at,
            created_at,
            updated_at
        "#,
    )
    .bind(request_id)
    .bind(&resolved.status)
    .bind(&resolved.title)
    .bind(&resolved.thumbnail_url)
    .bind(resolved.duration_seconds)
    .bind(&resolved.uploader)
    .bind(&resolved.extractor)
    .bind(&resolved.webpage_url)
    .fetch_one(db)
    .await
}

pub async fn mark_download_request_failed(
    db: &DbPool,
    request_id: uuid::Uuid,
    error_message: &str,
) -> Result<DownloadRequestRecord, sqlx::Error> {
    query_as::<_, DownloadRequestRecord>(
        r#"
        UPDATE download_requests
        SET
            status = 'failed',
            last_error = $2,
            updated_at = NOW()
        WHERE id = $1
        RETURNING
            id,
            provider,
            source_url,
            normalized_url,
            source_host,
            status,
            title,
            thumbnail_url,
            duration_seconds,
            uploader,
            extractor,
            webpage_url,
            last_error,
            resolved_at,
            created_at,
            updated_at
        "#,
    )
    .bind(request_id)
    .bind(error_message)
    .fetch_one(db)
    .await
}

pub async fn get_download_request_by_id(
    db: &DbPool,
    request_id: uuid::Uuid,
) -> Result<Option<DownloadRequestRecord>, sqlx::Error> {
    query_as::<_, DownloadRequestRecord>(
        r#"
        SELECT
            id,
            provider,
            source_url,
            normalized_url,
            source_host,
            status,
            title,
            thumbnail_url,
            duration_seconds,
            uploader,
            extractor,
            webpage_url,
            last_error,
            resolved_at,
            created_at,
            updated_at
        FROM download_requests
        WHERE id = $1
        "#,
    )
    .bind(request_id)
    .fetch_optional(db)
    .await
}
