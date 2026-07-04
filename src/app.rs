use anyhow::{Context, Result};
use axum::{
    Json, Router,
    extract::State,
    http::{HeaderValue, Method, StatusCode, header},
    response::IntoResponse,
    routing::get,
};
use reqwest::Client;
use serde::Serialize;
use sqlx::Executor;
use tower_http::{
    LatencyUnit,
    cors::{AllowOrigin, Any, CorsLayer},
    trace::{DefaultMakeSpan, DefaultOnResponse, TraceLayer},
};
use tracing::Level;

use crate::{
    config::{db::DbPool, environment::Environment},
    module::processing::route::router as processing_router,
    service::download_coordinator::DownloadCoordinator,
};

#[derive(Clone)]
pub struct AppState {
    pub db: DbPool,
    pub env: Environment,
    pub http_client: Client,
    pub download_coordinator: DownloadCoordinator,
}

pub fn build_router(state: AppState) -> Result<Router> {
    let cors_layer = build_cors_layer(&state.env)?;

    Ok(Router::new()
        .route("/health", get(health_check))
        .nest("/processing", processing_router())
        .with_state(state)
        .layer(cors_layer)
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(DefaultMakeSpan::new().level(Level::INFO))
                .on_response(
                    DefaultOnResponse::new()
                        .level(Level::INFO)
                        .latency_unit(LatencyUnit::Millis),
                ),
        ))
}

fn build_cors_layer(env: &Environment) -> Result<CorsLayer> {
    if env.cors_allowed_origins.iter().any(|origin| origin == "*") {
        return Ok(CorsLayer::new()
            .allow_origin(Any)
            .allow_methods([
                Method::GET,
                Method::POST,
                Method::PATCH,
                Method::DELETE,
                Method::OPTIONS,
            ])
            .allow_headers([header::ACCEPT, header::AUTHORIZATION, header::CONTENT_TYPE]));
    }

    let allowed_origins = env
        .cors_allowed_origins
        .iter()
        .map(|origin| {
            HeaderValue::from_str(origin)
                .with_context(|| format!("invalid ALLOWED_ORIGINS value `{origin}`"))
        })
        .collect::<Result<Vec<_>>>()?;

    Ok(CorsLayer::new()
        .allow_origin(AllowOrigin::list(allowed_origins))
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PATCH,
            Method::DELETE,
            Method::OPTIONS,
        ])
        .allow_headers([header::ACCEPT, header::AUTHORIZATION, header::CONTENT_TYPE]))
}

#[derive(Serialize)]
struct HealthResponse<'a> {
    status: &'a str,
}

async fn health_check(State(state): State<AppState>) -> impl IntoResponse {
    match state.db.execute("SELECT 1").await {
        Ok(_) => (StatusCode::OK, Json(HealthResponse { status: "ok" })),
        Err(error) => {
            tracing::error!(?error, "database health check failed");
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(HealthResponse { status: "degraded" }),
            )
        }
    }
}
