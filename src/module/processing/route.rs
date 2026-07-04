use axum::{
    Router,
    routing::{get, post},
};

use crate::app::AppState;

use super::controller;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/resolve", post(controller::resolve))
        .route("/{request_id}", get(controller::show))
        .route("/{request_id}/download", get(controller::download))
}
