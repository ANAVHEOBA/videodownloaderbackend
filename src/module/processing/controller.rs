use axum::{
    Json,
    extract::{Path, State},
    response::IntoResponse,
};
use uuid::Uuid;

use crate::{
    app::AppState,
    error::ApiError,
    module::processing::schema::{ResolveUrlRequest, ResolveUrlResponse},
    service::processing::{download_request, get_request, resolve_url},
};

pub async fn resolve(
    State(state): State<AppState>,
    Json(payload): Json<ResolveUrlRequest>,
) -> Result<Json<ResolveUrlResponse>, ApiError> {
    Ok(Json(resolve_url(&state, payload).await?))
}

pub async fn show(
    State(state): State<AppState>,
    Path(request_id): Path<Uuid>,
) -> Result<Json<ResolveUrlResponse>, ApiError> {
    Ok(Json(get_request(&state, request_id).await?))
}

pub async fn download(
    State(state): State<AppState>,
    Path(request_id): Path<Uuid>,
) -> Result<impl IntoResponse, ApiError> {
    download_request(&state, request_id).await
}
