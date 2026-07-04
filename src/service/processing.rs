use std::time::Duration;

use axum::{
    body::Body,
    http::{
        HeaderMap, HeaderValue, StatusCode,
        header::{CONTENT_DISPOSITION, CONTENT_LENGTH, CONTENT_TYPE},
    },
    response::Response,
};
use reqwest::Url;
use tokio::fs::File;
use tokio_util::io::ReaderStream;
use uuid::Uuid;

use crate::{
    app::AppState,
    error::ApiError,
    module::processing::{
        crud,
        model::{
            DOWNLOAD_STATUS_FAILED, DOWNLOAD_STATUS_PENDING, DOWNLOAD_STATUS_RESOLVED,
            DownloadRequestRecord, NewDownloadRequest, ResolvedDownloadRequest,
        },
        schema::{ProcessingRequestResponse, ResolveUrlRequest, ResolveUrlResponse},
    },
    service::{providers::detect_provider, yt_dlp},
};

pub async fn resolve_url(
    state: &AppState,
    payload: ResolveUrlRequest,
) -> Result<ResolveUrlResponse, ApiError> {
    let source_url = payload.url.trim();

    if source_url.is_empty() {
        return Err(ApiError::bad_request("url is required"));
    }

    if source_url.len() > state.env.max_url_length {
        return Err(ApiError::bad_request(
            "url exceeds the maximum allowed length",
        ));
    }

    let parsed_url =
        Url::parse(source_url).map_err(|_| ApiError::bad_request("invalid url provided"))?;

    validate_supported_scheme(&parsed_url)?;

    let provider = detect_provider(&parsed_url, state)?;
    let normalized_url = normalize_url(&parsed_url);
    let source_host = parsed_url
        .host_str()
        .ok_or_else(|| ApiError::bad_request("url host is missing"))?
        .to_ascii_lowercase();

    let request_id = Uuid::new_v4();

    crud::insert_download_request(
        &state.db,
        &NewDownloadRequest {
            id: request_id,
            provider: provider.to_owned(),
            source_url: source_url.to_owned(),
            normalized_url,
            source_host,
            status: DOWNLOAD_STATUS_PENDING.to_owned(),
        },
    )
    .await
    .map_err(|error| {
        tracing::error!(?error, "failed to persist download request");
        ApiError::internal("failed to persist download request")
    })?;

    match yt_dlp::extract_metadata(&state.env, source_url).await {
        Ok(metadata) => {
            if let Some(duration_seconds) = metadata.duration_seconds {
                if duration_seconds > state.env.max_video_duration_seconds as i64 {
                    let error_message = format!(
                        "requested media duration of {duration_seconds} seconds exceeds the configured maximum of {} seconds",
                        state.env.max_video_duration_seconds
                    );

                    if let Err(db_error) =
                        crud::mark_download_request_failed(&state.db, request_id, &error_message)
                            .await
                    {
                        tracing::error!(
                            ?db_error,
                            request_id = %request_id,
                            "failed to mark oversized request as failed"
                        );
                    }

                    return Err(ApiError::bad_request(
                        "requested media exceeds the maximum allowed duration",
                    ));
                }
            }

            let record = crud::mark_download_request_resolved(
                &state.db,
                request_id,
                &ResolvedDownloadRequest {
                    status: DOWNLOAD_STATUS_RESOLVED.to_owned(),
                    title: Some(metadata.title),
                    thumbnail_url: metadata.thumbnail_url,
                    duration_seconds: metadata.duration_seconds,
                    uploader: metadata.uploader,
                    extractor: metadata.extractor,
                    webpage_url: Some(metadata.webpage_url),
                },
            )
            .await
            .map_err(|error| {
                tracing::error!(?error, request_id = %request_id, "failed to update resolved request");
                ApiError::internal("failed to update resolved request")
            })?;

            Ok(ResolveUrlResponse {
                request: ProcessingRequestResponse::from_record(record),
            })
        }
        Err(error) => {
            let error_message = error.to_string();

            if let Err(db_error) =
                crud::mark_download_request_failed(&state.db, request_id, &error_message).await
            {
                tracing::error!(
                    ?db_error,
                    request_id = %request_id,
                    "failed to mark request as failed"
                );
            }

            tracing::error!(
                ?error,
                request_id = %request_id,
                "yt-dlp metadata extraction failed"
            );

            Err(ApiError::service_unavailable(
                "failed to extract media metadata from the supplied URL",
            ))
        }
    }
}

pub async fn get_request(
    state: &AppState,
    request_id: Uuid,
) -> Result<ResolveUrlResponse, ApiError> {
    let record = load_request(state, request_id).await?;

    Ok(ResolveUrlResponse {
        request: ProcessingRequestResponse::from_record(record),
    })
}

pub async fn download_request(state: &AppState, request_id: Uuid) -> Result<Response, ApiError> {
    let record = load_request(state, request_id).await?;

    if record.status == DOWNLOAD_STATUS_FAILED {
        return Err(ApiError::service_unavailable(
            "this request failed during metadata extraction",
        ));
    }

    if record.status != DOWNLOAD_STATUS_RESOLVED {
        return Err(ApiError::service_unavailable(
            "this request is not ready for download yet",
        ));
    }

    let _download_guard = state.download_coordinator.acquire(request_id).await;

    let downloaded = yt_dlp::download_highest_quality(&state.env, &record)
        .await
        .map_err(|error| {
            tracing::error!(?error, request_id = %request_id, "yt-dlp download failed");
            ApiError::service_unavailable("failed to download the requested media")
        })?;

    let filename = downloaded.filename.clone();
    let cleanup_dir = downloaded.temp_dir.clone();
    let cleanup_token = yt_dlp::refresh_download_lease(&downloaded.temp_dir)
        .await
        .map_err(|error| {
            tracing::error!(
                ?error,
                request_id = %request_id,
                temp_dir = ?downloaded.temp_dir,
                "failed to refresh download cleanup lease"
            );
            ApiError::internal("failed to prepare downloaded media")
        })?;
    let file = File::open(&downloaded.path).await.map_err(|error| {
        tracing::error!(?error, path = ?downloaded.path, "failed to open downloaded media");
        ApiError::internal("failed to open downloaded media")
    })?;
    let metadata = file.metadata().await.map_err(|error| {
        tracing::error!(?error, path = ?downloaded.path, "failed to read downloaded media metadata");
        ApiError::internal("failed to read downloaded media metadata")
    })?;
    drop(_download_guard);

    let cleanup_after = state.env.download_retention_seconds;
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(cleanup_after)).await;
        yt_dlp::cleanup_download_dir(cleanup_dir, cleanup_token).await;
    });

    let body = Body::from_stream(ReaderStream::new(file));
    let mut headers = HeaderMap::new();
    headers.insert(
        CONTENT_TYPE,
        HeaderValue::from_static("application/octet-stream"),
    );
    headers.insert(
        CONTENT_DISPOSITION,
        build_content_disposition(&filename)
            .map_err(|_| ApiError::internal("failed to build download headers"))?,
    );
    headers.insert(
        CONTENT_LENGTH,
        HeaderValue::from_str(&metadata.len().to_string())
            .map_err(|_| ApiError::internal("failed to build download headers"))?,
    );

    let mut response = Response::new(body);
    *response.status_mut() = StatusCode::OK;
    *response.headers_mut() = headers;
    Ok(response)
}

async fn load_request(
    state: &AppState,
    request_id: Uuid,
) -> Result<DownloadRequestRecord, ApiError> {
    crud::get_download_request_by_id(&state.db, request_id)
        .await
        .map_err(|error| {
            tracing::error!(?error, request_id = %request_id, "failed to fetch download request");
            ApiError::internal("failed to fetch download request")
        })?
        .ok_or_else(|| ApiError::not_found("download request not found"))
}

fn validate_supported_scheme(url: &Url) -> Result<(), ApiError> {
    match url.scheme() {
        "http" | "https" => Ok(()),
        _ => Err(ApiError::bad_request(
            "only http and https URLs are supported",
        )),
    }
}

fn normalize_url(url: &Url) -> String {
    let mut normalized = url.clone();
    normalized.set_fragment(None);
    normalized.to_string()
}

fn build_content_disposition(
    filename: &str,
) -> Result<HeaderValue, axum::http::header::InvalidHeaderValue> {
    let fallback = filename.replace('"', "");
    HeaderValue::from_str(&format!("attachment; filename=\"{fallback}\""))
}
