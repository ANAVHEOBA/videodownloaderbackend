use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use reqwest::{
    Client, Url,
    header::{REFERER, USER_AGENT},
};
use serde::Deserialize;
use tokio::{
    fs,
    io::{AsyncWriteExt, BufWriter},
};

use crate::{
    config::environment::Environment,
    module::processing::model::{DownloadRequestRecord, PROVIDER_INSTAGRAM, PROVIDER_TIKTOK},
    service::yt_dlp::{DownloadedMedia, ExtractedMediaMetadata},
};

const TIKTOK_FALLBACK_EXTRACTOR: &str = "TikWM";
const INSTAGRAM_OEMBED_EXTRACTOR: &str = "InstagramOEmbed";
const FALLBACK_DOWNLOADED_FILE_PATH_FILE: &str = ".fallback-downloaded-filepath";
const BROWSER_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36";
const INSTAGRAM_MOBILE_USER_AGENT: &str = "Instagram 275.0.0.27.98 Android (33/13; 280dpi; 720x1423; Xiaomi; Redmi 7; onclite; qcom; en_US; 458229237)";

pub async fn extract_metadata(
    env: &Environment,
    client: &Client,
    provider: &str,
    source_url: &str,
) -> Result<Option<ExtractedMediaMetadata>> {
    match provider {
        PROVIDER_TIKTOK if env.tiktok_fallback_enabled => {
            extract_tiktok_metadata(env, client, source_url)
                .await
                .map(Some)
        }
        PROVIDER_INSTAGRAM if env.instagram_oembed_fallback_enabled => {
            extract_instagram_oembed_metadata(client, source_url)
                .await
                .map(Some)
        }
        _ => Ok(None),
    }
}

pub fn should_prefer_direct_download(record: &DownloadRequestRecord) -> bool {
    record.provider == PROVIDER_TIKTOK
        && record.extractor.as_deref() == Some(TIKTOK_FALLBACK_EXTRACTOR)
}

pub async fn download_direct_highest_quality(
    env: &Environment,
    client: &Client,
    record: &DownloadRequestRecord,
) -> Result<Option<DownloadedMedia>> {
    if record.provider != PROVIDER_TIKTOK || !env.tiktok_fallback_enabled {
        return Ok(None);
    }

    download_tiktok_direct(env, client, record).await.map(Some)
}

async fn extract_tiktok_metadata(
    env: &Environment,
    client: &Client,
    source_url: &str,
) -> Result<ExtractedMediaMetadata> {
    let data = fetch_tiktok_data(env, client, source_url).await?;
    let title = non_empty(data.title.as_deref())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| format!("TikTok video {}", data.id));
    let webpage_url = tiktok_webpage_url(&data).unwrap_or_else(|| source_url.to_owned());

    Ok(ExtractedMediaMetadata {
        title,
        thumbnail_url: data.cover.or(data.origin_cover),
        duration_seconds: data.duration,
        uploader: data
            .author
            .as_ref()
            .and_then(|author| {
                non_empty(author.unique_id.as_deref()).or(author.nickname.as_deref())
            })
            .map(ToOwned::to_owned),
        extractor: Some(TIKTOK_FALLBACK_EXTRACTOR.to_owned()),
        webpage_url,
    })
}

async fn download_tiktok_direct(
    env: &Environment,
    client: &Client,
    record: &DownloadRequestRecord,
) -> Result<DownloadedMedia> {
    let temp_dir = env.temp_dir.join(record.id.to_string());

    if let Some(path) = load_fallback_downloaded_file_path(&temp_dir).await? {
        let filename = public_filename(&path)?;
        validate_download_size(env, &path).await?;

        return Ok(DownloadedMedia {
            filename,
            path,
            temp_dir,
        });
    }

    if fs::try_exists(&temp_dir).await.unwrap_or(false) {
        let _ = fs::remove_dir_all(&temp_dir).await;
    }

    fs::create_dir_all(&temp_dir)
        .await
        .with_context(|| format!("failed to create temp dir `{}`", temp_dir.display()))?;

    let data = fetch_tiktok_data(env, client, &record.source_url).await?;
    let media_url = data
        .play
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| anyhow!("TikTok fallback did not return a no-watermark media URL"))?;
    let filename = format!("tiktok_{}.mp4", sanitize_filename_part(&data.id));
    let path = temp_dir.join(&filename);
    let part_path = temp_dir.join(format!("{filename}.part"));

    download_to_file(env, client, media_url, &part_path).await?;
    fs::rename(&part_path, &path)
        .await
        .with_context(|| format!("failed to finalize downloaded media `{}`", path.display()))?;
    persist_fallback_downloaded_file_path(&temp_dir, &path).await?;
    validate_download_size(env, &path).await?;

    Ok(DownloadedMedia {
        filename,
        path,
        temp_dir,
    })
}

async fn fetch_tiktok_data(
    env: &Environment,
    client: &Client,
    source_url: &str,
) -> Result<TikwmData> {
    let mut last_error: Option<anyhow::Error> = None;

    for attempt in 0..2 {
        let mut url = Url::parse(&env.tiktok_fallback_api_url).with_context(|| {
            format!(
                "invalid TIKTOK_FALLBACK_API_URL `{}`",
                env.tiktok_fallback_api_url
            )
        })?;
        url.query_pairs_mut().append_pair("url", source_url);

        let response = client
            .get(url)
            .header(USER_AGENT, BROWSER_USER_AGENT)
            .send()
            .await
            .context("TikTok fallback request failed")?
            .error_for_status()
            .context("TikTok fallback returned an error status")?;
        let payload = response
            .json::<TikwmResponse>()
            .await
            .context("failed to parse TikTok fallback response")?;

        if payload.code == 0 {
            return payload
                .data
                .ok_or_else(|| anyhow!("TikTok fallback returned success without data"));
        }

        let message = payload
            .msg
            .unwrap_or_else(|| "unknown fallback error".to_owned());
        last_error = Some(anyhow!("TikTok fallback failed: {message}"));

        if attempt == 0 && message.to_ascii_lowercase().contains("limit") {
            tokio::time::sleep(std::time::Duration::from_millis(1200)).await;
            continue;
        }
    }

    Err(last_error.unwrap_or_else(|| anyhow!("TikTok fallback failed")))
}

async fn extract_instagram_oembed_metadata(
    client: &Client,
    source_url: &str,
) -> Result<ExtractedMediaMetadata> {
    let shortcode = instagram_shortcode(source_url)
        .ok_or_else(|| anyhow!("failed to parse Instagram shortcode from URL"))?;
    let media_url = format!("https://www.instagram.com/p/{shortcode}/");
    let mut url = Url::parse("https://i.instagram.com/api/v1/oembed/")
        .expect("hard-coded Instagram oEmbed URL should be valid");
    url.query_pairs_mut().append_pair("url", &media_url);

    let response = client
        .get(url)
        .header(USER_AGENT, INSTAGRAM_MOBILE_USER_AGENT)
        .header("x-ig-app-locale", "en_US")
        .header("x-ig-device-locale", "en_US")
        .header("x-ig-mapped-locale", "en_US")
        .header("accept-language", "en-US")
        .header("x-fb-http-engine", "Liger")
        .header("x-fb-client-ip", "True")
        .header("x-fb-server-cluster", "True")
        .send()
        .await
        .context("Instagram oEmbed request failed")?
        .error_for_status()
        .context("Instagram oEmbed returned an error status")?;
    let payload = response
        .json::<InstagramOembedResponse>()
        .await
        .context("failed to parse Instagram oEmbed response")?;
    let title = non_empty(payload.title.as_deref())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| format!("Instagram media {shortcode}"));

    Ok(ExtractedMediaMetadata {
        title,
        thumbnail_url: payload.thumbnail_url,
        duration_seconds: None,
        uploader: payload.author_name,
        extractor: Some(INSTAGRAM_OEMBED_EXTRACTOR.to_owned()),
        webpage_url: source_url.to_owned(),
    })
}

async fn download_to_file(
    env: &Environment,
    client: &Client,
    media_url: &str,
    path: &Path,
) -> Result<()> {
    let mut response = client
        .get(media_url)
        .header(USER_AGENT, BROWSER_USER_AGENT)
        .header(REFERER, "https://www.tiktok.com/")
        .send()
        .await
        .context("failed to request fallback media URL")?
        .error_for_status()
        .context("fallback media URL returned an error status")?;

    if let Some(content_length) = response.content_length() {
        if content_length > env.max_download_bytes {
            bail!(
                "fallback media exceeds the configured size limit of {} bytes",
                env.max_download_bytes
            );
        }
    }

    let file = fs::File::create(path)
        .await
        .with_context(|| format!("failed to create fallback media file `{}`", path.display()))?;
    let mut writer = BufWriter::new(file);
    let mut downloaded = 0_u64;

    while let Some(chunk) = response
        .chunk()
        .await
        .context("failed to read fallback media chunk")?
    {
        downloaded += chunk.len() as u64;
        if downloaded > env.max_download_bytes {
            bail!(
                "fallback media exceeded the configured size limit of {} bytes",
                env.max_download_bytes
            );
        }
        writer
            .write_all(&chunk)
            .await
            .context("failed to write fallback media chunk")?;
    }

    writer
        .flush()
        .await
        .context("failed to flush fallback media file")?;

    if downloaded == 0 {
        bail!("fallback media URL returned an empty response");
    }

    Ok(())
}

fn instagram_shortcode(source_url: &str) -> Option<String> {
    let url = Url::parse(source_url).ok()?;
    let mut segments = url.path_segments()?;

    while let Some(segment) = segments.next() {
        if matches!(segment, "p" | "reel" | "reels" | "tv") {
            return segments
                .next()
                .filter(|value| !value.is_empty())
                .map(str::to_owned);
        }
    }

    None
}

fn tiktok_webpage_url(data: &TikwmData) -> Option<String> {
    let username = data
        .author
        .as_ref()
        .and_then(|author| non_empty(author.unique_id.as_deref()))?;

    Some(format!(
        "https://www.tiktok.com/@{username}/video/{}",
        data.id
    ))
}

async fn load_fallback_downloaded_file_path(dir: &Path) -> Result<Option<PathBuf>> {
    let marker_path = dir.join(FALLBACK_DOWNLOADED_FILE_PATH_FILE);
    if !fs::try_exists(&marker_path).await.with_context(|| {
        format!(
            "failed to check fallback marker `{}`",
            marker_path.display()
        )
    })? {
        return Ok(None);
    }

    let raw_path = fs::read_to_string(&marker_path)
        .await
        .with_context(|| format!("failed to read fallback marker `{}`", marker_path.display()))?;
    let path = PathBuf::from(raw_path.trim());

    if fs::try_exists(&path)
        .await
        .with_context(|| format!("failed to check fallback media file `{}`", path.display()))?
    {
        return Ok(Some(path));
    }

    Ok(None)
}

async fn persist_fallback_downloaded_file_path(dir: &Path, path: &Path) -> Result<()> {
    let marker_path = dir.join(FALLBACK_DOWNLOADED_FILE_PATH_FILE);
    fs::write(&marker_path, path.as_os_str().as_encoded_bytes())
        .await
        .with_context(|| {
            format!(
                "failed to write fallback marker `{}`",
                marker_path.display()
            )
        })
}

async fn validate_download_size(env: &Environment, path: &Path) -> Result<()> {
    let metadata = fs::metadata(path).await.with_context(|| {
        format!(
            "failed to read fallback media metadata `{}`",
            path.display()
        )
    })?;

    if metadata.len() > env.max_download_bytes {
        bail!(
            "fallback media exceeded the configured size limit of {} bytes",
            env.max_download_bytes
        );
    }

    Ok(())
}

fn public_filename(path: &Path) -> Result<String> {
    path.file_name()
        .and_then(|value| value.to_str())
        .map(ToOwned::to_owned)
        .ok_or_else(|| anyhow!("fallback media file name is invalid"))
}

fn sanitize_filename_part(value: &str) -> String {
    let sanitized = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_') {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>();

    if sanitized.is_empty() {
        "media".to_owned()
    } else {
        sanitized
    }
}

fn non_empty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

#[derive(Debug, Deserialize)]
struct TikwmResponse {
    code: i64,
    msg: Option<String>,
    data: Option<TikwmData>,
}

#[derive(Debug, Deserialize)]
struct TikwmData {
    id: String,
    title: Option<String>,
    cover: Option<String>,
    origin_cover: Option<String>,
    duration: Option<i64>,
    play: Option<String>,
    author: Option<TikwmAuthor>,
}

#[derive(Debug, Deserialize)]
struct TikwmAuthor {
    unique_id: Option<String>,
    nickname: Option<String>,
}

#[derive(Debug, Deserialize)]
struct InstagramOembedResponse {
    title: Option<String>,
    author_name: Option<String>,
    thumbnail_url: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::{instagram_shortcode, sanitize_filename_part};

    #[test]
    fn parses_instagram_shortcodes_from_common_paths() {
        assert_eq!(
            instagram_shortcode("https://www.instagram.com/reel/DaWAZhQSSYQ/?igsh=test"),
            Some("DaWAZhQSSYQ".to_owned())
        );
        assert_eq!(
            instagram_shortcode("https://www.instagram.com/p/ABC123/"),
            Some("ABC123".to_owned())
        );
    }

    #[test]
    fn sanitizes_filename_parts() {
        assert_eq!(sanitize_filename_part("abc-123_DEF"), "abc-123_DEF");
        assert_eq!(sanitize_filename_part("a/b c"), "a_b_c");
    }
}
