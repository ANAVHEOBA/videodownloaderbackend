use std::time::Duration;

use anyhow::Result;
use reqwest::redirect::Policy;
use sqlx::migrate;
use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};
use videodownloaderbackend::{
    app::{AppState, build_router},
    config::{db, environment::Environment},
    service::download_coordinator::DownloadCoordinator,
};

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    init_tracing();

    let env = Environment::load()?;
    let db = db::create_pool(&env).await?;

    if env.database_run_migrations {
        migrate!("./migrations").run(&db).await?;
    }

    let http_client = reqwest::Client::builder()
        .user_agent(env.upstream_user_agent.clone())
        .connect_timeout(Duration::from_secs(env.upstream_connect_timeout_seconds))
        .timeout(Duration::from_secs(env.upstream_request_timeout_seconds))
        .redirect(Policy::limited(env.upstream_max_redirects))
        .build()?;

    let state = AppState {
        db,
        env: env.clone(),
        http_client,
        download_coordinator: DownloadCoordinator::default(),
    };

    let app = build_router(state)?;
    let bind_address = format!("{}:{}", env.host, env.port);
    let listener = tokio::net::TcpListener::bind(&bind_address).await?;

    tracing::info!(bind_address, "server listening");

    axum::serve(listener, app).await?;

    Ok(())
}

fn init_tracing() {
    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .with(fmt::layer())
        .init();
}
