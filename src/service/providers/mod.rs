mod capcut;
mod facebook;
mod instagram;
mod pinterest;
mod soundcloud;
mod spotify;
mod telegram;
mod tiktok;
mod twitter;
mod youtube;

use reqwest::Url;

use crate::{app::AppState, error::ApiError};

pub fn detect_provider(url: &Url, state: &AppState) -> Result<&'static str, ApiError> {
    if tiktok::matches(url) {
        return enabled(state.env.enable_tiktok, tiktok::PROVIDER);
    }

    if youtube::matches(url) {
        return enabled(state.env.enable_youtube, youtube::PROVIDER);
    }

    if instagram::matches(url) {
        return enabled(state.env.enable_instagram, instagram::PROVIDER);
    }

    if twitter::matches(url) {
        return enabled(state.env.enable_twitter, twitter::PROVIDER);
    }

    if facebook::matches(url) {
        return enabled(state.env.enable_facebook, facebook::PROVIDER);
    }

    if pinterest::matches(url) {
        return enabled(state.env.enable_pinterest, pinterest::PROVIDER);
    }

    if capcut::matches(url) {
        return enabled(state.env.enable_capcut, capcut::PROVIDER);
    }

    if telegram::matches(url) {
        return enabled(state.env.enable_telegram, telegram::PROVIDER);
    }

    if spotify::matches(url) {
        return enabled(state.env.enable_spotify, spotify::PROVIDER);
    }

    if soundcloud::matches(url) {
        return enabled(state.env.enable_soundcloud, soundcloud::PROVIDER);
    }

    Err(ApiError::bad_request(
        "unsupported provider; supported providers are tiktok, youtube, instagram, twitter, facebook, pinterest, capcut, telegram, spotify, and soundcloud",
    ))
}

fn enabled(flag: bool, provider: &'static str) -> Result<&'static str, ApiError> {
    if flag {
        Ok(provider)
    } else {
        Err(ApiError::service_unavailable(format!(
            "{provider} support is disabled in this environment"
        )))
    }
}
