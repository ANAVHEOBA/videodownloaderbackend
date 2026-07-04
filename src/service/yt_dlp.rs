use std::{
    ffi::{OsStr, OsString},
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};

use anyhow::{Context, Result, anyhow, bail};
use serde::Deserialize;
use tokio::{fs, process::Command, time::timeout};
use uuid::Uuid;

use crate::{
    config::environment::Environment,
    module::processing::model::{
        DownloadRequestRecord, PROVIDER_SOUNDCLOUD, PROVIDER_SPOTIFY, PROVIDER_TIKTOK,
        PROVIDER_YOUTUBE,
    },
};

const CLEANUP_TOKEN_FILE: &str = ".cleanup-token";
const DOWNLOADED_FILE_PATH_FILE: &str = ".downloaded-filepath";
const UNKNOWN_VIDEO_EXTENSION: &str = ".unknown_video";
const UNKNOWN_AUDIO_EXTENSION: &str = ".unknown_audio";
const YOUTUBE_PROGRESSIVE_MAX_HEIGHT: u16 = 1080;
const YOUTUBE_ADAPTIVE_MAX_HEIGHT: u16 = 720;

pub struct ExtractedMediaMetadata {
    pub title: String,
    pub thumbnail_url: Option<String>,
    pub duration_seconds: Option<i64>,
    pub uploader: Option<String>,
    pub extractor: Option<String>,
    pub webpage_url: String,
}

pub struct DownloadedMedia {
    pub filename: String,
    pub path: PathBuf,
    pub temp_dir: PathBuf,
}

#[derive(Debug, Deserialize, Clone)]
struct YtDlpInfo {
    title: Option<String>,
    fulltitle: Option<String>,
    thumbnail: Option<String>,
    thumbnails: Option<Vec<YtDlpThumbnail>>,
    duration: Option<f64>,
    uploader: Option<String>,
    channel: Option<String>,
    extractor: Option<String>,
    extractor_key: Option<String>,
    webpage_url: Option<String>,
    original_url: Option<String>,
    entries: Option<Vec<YtDlpInfo>>,
}

#[derive(Debug, Deserialize, Clone)]
struct YtDlpThumbnail {
    url: Option<String>,
    width: Option<i64>,
    height: Option<i64>,
}

pub async fn extract_metadata(
    env: &Environment,
    source_url: &str,
) -> Result<ExtractedMediaMetadata> {
    let args = vec![
        OsString::from("--dump-single-json"),
        OsString::from("--no-playlist"),
        OsString::from("--skip-download"),
        OsString::from("--no-warnings"),
        OsString::from(source_url),
    ];

    let output = run_command(env, env.ytdlp_metadata_timeout_seconds, &args).await?;

    let mut info: YtDlpInfo =
        serde_json::from_slice(&output.stdout).context("failed to parse yt-dlp metadata json")?;

    if let Some(entries) = info.entries.take() {
        if let Some(primary) = entries.into_iter().find(|entry| entry.title.is_some()) {
            info = primary;
        }
    }

    let thumbnail_url = best_thumbnail_url(&info);
    let title = info
        .title
        .clone()
        .or_else(|| info.fulltitle.clone())
        .ok_or_else(|| anyhow!("yt-dlp did not return a title"))?;
    let duration_seconds = info.duration.map(|value| value.round() as i64);
    let uploader = info.uploader.clone().or_else(|| info.channel.clone());
    let extractor = info
        .extractor_key
        .clone()
        .or_else(|| info.extractor.clone());
    let webpage_url = info
        .webpage_url
        .clone()
        .or_else(|| info.original_url.clone())
        .unwrap_or_else(|| source_url.to_owned());

    Ok(ExtractedMediaMetadata {
        title,
        thumbnail_url,
        duration_seconds,
        uploader,
        extractor,
        webpage_url,
    })
}

pub async fn download_highest_quality(
    env: &Environment,
    record: &DownloadRequestRecord,
) -> Result<DownloadedMedia> {
    let temp_dir = env.temp_dir.join(record.id.to_string());

    if let Some(path) = load_downloaded_file_path(&temp_dir).await? {
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

    let format = preferred_format(record, env.ffmpeg_enabled);
    let output_template = "%(title).180B [%(id)s].%(ext)s";

    let mut args = vec![
        OsString::from("--no-playlist"),
        OsString::from("--no-warnings"),
        OsString::from("--no-progress"),
        OsString::from("--restrict-filenames"),
        OsString::from("-f"),
        OsString::from(format),
        OsString::from("--max-filesize"),
        OsString::from(env.max_download_bytes.to_string()),
    ];

    if env.ffmpeg_enabled {
        if let Some(ffmpeg_location) = ffmpeg_location_argument(&env.ffmpeg_binary_path) {
            args.extend([OsString::from("--ffmpeg-location"), ffmpeg_location]);
        }

        args.extend([
            OsString::from("--merge-output-format"),
            OsString::from("mp4"),
        ]);
    }

    args.extend([
        OsString::from("--print"),
        OsString::from("after_move:filepath"),
        OsString::from("-P"),
        temp_dir.as_os_str().to_os_string(),
        OsString::from("-o"),
        OsString::from(output_template),
        OsString::from(record.source_url.clone()),
    ]);

    let output = run_command(env, env.ytdlp_download_timeout_seconds, &args).await?;
    let path = extract_downloaded_file_path(&output.stdout, &temp_dir)?;
    persist_downloaded_file_path(&temp_dir, &path).await?;
    let filename = public_filename(&path)?;
    validate_download_size(env, &path).await?;

    Ok(DownloadedMedia {
        filename,
        path,
        temp_dir,
    })
}

async fn run_command(
    env: &Environment,
    timeout_seconds: u64,
    args: &[OsString],
) -> Result<std::process::Output> {
    let ytdlp_command = resolve_ytdlp_command(&env.ytdlp_binary_path)?;
    let mut command = Command::new(&ytdlp_command);
    command
        .args(args.iter().map(OsStr::new))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let output = timeout(Duration::from_secs(timeout_seconds), command.output())
        .await
        .with_context(|| format!("yt-dlp timed out after {timeout_seconds} seconds"))?
        .with_context(|| {
            format!(
                "failed to spawn yt-dlp using resolved command `{}`",
                Path::new(&ytdlp_command).display()
            )
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        bail!("yt-dlp failed: {stderr}");
    }

    Ok(output)
}

fn resolve_ytdlp_command(path: &Path) -> Result<OsString> {
    if path.exists() {
        return Ok(path.as_os_str().to_os_string());
    }

    if path.components().count() == 1 {
        return Ok(path.as_os_str().to_os_string());
    }

    if let Some(file_name) = path.file_name() {
        tracing::warn!(
            configured_path = %path.display(),
            fallback_command = %Path::new(file_name).display(),
            "configured yt-dlp path does not exist; falling back to PATH lookup"
        );
        return Ok(file_name.to_os_string());
    }

    bail!("yt-dlp binary not found at `{}`", path.display())
}

fn best_thumbnail_url(info: &YtDlpInfo) -> Option<String> {
    if let Some(thumbnail) = info.thumbnail.clone() {
        return Some(thumbnail);
    }

    info.thumbnails.as_ref().and_then(|items| {
        items
            .iter()
            .filter_map(|item| {
                item.url.as_ref().map(|url| {
                    (
                        item.width.unwrap_or_default() * item.height.unwrap_or_default(),
                        url.clone(),
                    )
                })
            })
            .max_by_key(|(score, _)| *score)
            .map(|(_, url)| url)
    })
}

fn preferred_format(record: &DownloadRequestRecord, ffmpeg_enabled: bool) -> String {
    match record.provider.as_str() {
        PROVIDER_SPOTIFY | PROVIDER_SOUNDCLOUD => "bestaudio/best".to_owned(),
        PROVIDER_YOUTUBE => youtube_preferred_format(ffmpeg_enabled),
        PROVIDER_TIKTOK if ffmpeg_enabled => {
            "bestvideo*[format_note!*='watermarked']+bestaudio/best[format_note!*='watermarked']/bestvideo*+bestaudio/best"
                .to_owned()
        }
        PROVIDER_TIKTOK => "best*[format_note!*='watermarked']/best".to_owned(),
        _ if ffmpeg_enabled => "bestvideo*+bestaudio/best".to_owned(),
        _ => "best".to_owned(),
    }
}

fn youtube_preferred_format(ffmpeg_enabled: bool) -> String {
    if !ffmpeg_enabled {
        return format!(
            "best[ext=mp4][height<={0}]/best[height<={0}]/best",
            YOUTUBE_PROGRESSIVE_MAX_HEIGHT
        );
    }

    format!(
        concat!(
            "best[ext=mp4][height<={progressive}]/",
            "best[height<={progressive}]/",
            "bestvideo[ext=mp4][vcodec^=avc1][height<={adaptive}]+bestaudio[ext=m4a][acodec^=mp4a]/",
            "bestvideo[ext=mp4][height<={adaptive}]+bestaudio[ext=m4a]/",
            "bestvideo[height<={adaptive}]+bestaudio/",
            "best[height<={adaptive}]/best"
        ),
        progressive = YOUTUBE_PROGRESSIVE_MAX_HEIGHT,
        adaptive = YOUTUBE_ADAPTIVE_MAX_HEIGHT,
    )
}

fn ffmpeg_location_argument(value: &str) -> Option<OsString> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }

    let path = Path::new(trimmed);
    if path.exists() {
        return Some(OsString::from(trimmed));
    }

    if path.is_absolute() || path.components().count() > 1 {
        tracing::warn!(
            configured_path = %path.display(),
            "configured ffmpeg path does not exist; falling back to PATH lookup"
        );
        return None;
    }

    None
}

pub async fn refresh_download_lease(temp_dir: &Path) -> Result<String> {
    let token = Uuid::new_v4().to_string();
    let marker_path = temp_dir.join(CLEANUP_TOKEN_FILE);

    fs::write(&marker_path, &token)
        .await
        .with_context(|| format!("failed to write cleanup token `{}`", marker_path.display()))?;

    Ok(token)
}

pub async fn cleanup_download_dir(temp_dir: PathBuf, token: String) {
    let marker_path = temp_dir.join(CLEANUP_TOKEN_FILE);
    let current = fs::read_to_string(&marker_path).await;

    let should_delete = matches!(current, Ok(current) if current.trim() == token);
    if should_delete {
        let _ = fs::remove_dir_all(temp_dir).await;
    }
}

async fn load_downloaded_file_path(dir: &Path) -> Result<Option<PathBuf>> {
    let marker_path = dir.join(DOWNLOADED_FILE_PATH_FILE);
    if !fs::try_exists(&marker_path)
        .await
        .with_context(|| format!("failed to check download marker `{}`", marker_path.display()))?
    {
        return Ok(None);
    }

    let raw_path = fs::read_to_string(&marker_path)
        .await
        .with_context(|| format!("failed to read download marker `{}`", marker_path.display()))?;
    let path = PathBuf::from(raw_path.trim());

    if fs::try_exists(&path)
        .await
        .with_context(|| format!("failed to check downloaded file `{}`", path.display()))?
    {
        return Ok(Some(path));
    }

    Ok(None)
}

async fn persist_downloaded_file_path(dir: &Path, path: &Path) -> Result<()> {
    let marker_path = dir.join(DOWNLOADED_FILE_PATH_FILE);
    fs::write(&marker_path, path.as_os_str().as_encoded_bytes())
        .await
        .with_context(|| format!("failed to write download marker `{}`", marker_path.display()))
}

fn extract_downloaded_file_path(stdout: &[u8], temp_dir: &Path) -> Result<PathBuf> {
    let path = String::from_utf8_lossy(stdout)
        .lines()
        .map(str::trim)
        .rev()
        .find(|line| !line.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| anyhow!("yt-dlp did not report a final downloaded filepath"))?;

    if !path.starts_with(temp_dir) {
        bail!(
            "yt-dlp reported an unexpected download path outside the temp dir: {}",
            path.display()
        );
    }

    Ok(path)
}

#[cfg(test)]
async fn find_primary_download_file(dir: &Path) -> Result<Option<PathBuf>> {
    if !fs::try_exists(dir)
        .await
        .with_context(|| format!("failed to check download dir `{}`", dir.display()))?
    {
        return Ok(None);
    }

    let mut entries = fs::read_dir(dir)
        .await
        .with_context(|| format!("failed to read download dir `{}`", dir.display()))?;
    let mut largest_file: Option<(u64, PathBuf)> = None;

    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();
        let metadata = entry.metadata().await?;

        if !metadata.is_file() {
            continue;
        }

        let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };

        if name.starts_with('.')
            || name.ends_with(".part")
            || name.ends_with(".ytdl")
        {
            continue;
        }

        let size = metadata.len();
        match &largest_file {
            Some((current_size, _)) if *current_size >= size => {}
            _ => largest_file = Some((size, path)),
        }
    }

    Ok(largest_file.map(|(_, path)| path))
}

async fn validate_download_size(env: &Environment, path: &Path) -> Result<()> {
    let metadata = fs::metadata(path)
        .await
        .with_context(|| format!("failed to read downloaded media metadata `{}`", path.display()))?;

    if metadata.len() > env.max_download_bytes {
        bail!(
            "downloaded media exceeded the configured size limit of {} bytes",
            env.max_download_bytes
        );
    }

    Ok(())
}

fn public_filename(path: &Path) -> Result<String> {
    let filename = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| anyhow!("downloaded file name is invalid"))?;

    if let Some(stem) = filename.strip_suffix(UNKNOWN_VIDEO_EXTENSION) {
        return Ok(format!("{stem}.mp4"));
    }

    if let Some(stem) = filename.strip_suffix(UNKNOWN_AUDIO_EXTENSION) {
        return Ok(format!("{stem}.m4a"));
    }

    Ok(filename.to_owned())
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    use tokio::fs;

    use super::{
        YOUTUBE_ADAPTIVE_MAX_HEIGHT, YOUTUBE_PROGRESSIVE_MAX_HEIGHT, cleanup_download_dir,
        extract_downloaded_file_path, ffmpeg_location_argument, find_primary_download_file,
        load_downloaded_file_path, persist_downloaded_file_path, public_filename,
        refresh_download_lease, resolve_ytdlp_command, youtube_preferred_format,
    };

    fn unique_temp_dir(name: &str) -> PathBuf {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be after unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("videodownloaderbackend-{name}-{suffix}"))
    }

    #[tokio::test]
    async fn finds_primary_download_file_and_ignores_temp_artifacts() {
        let dir = unique_temp_dir("primary-file");
        fs::create_dir_all(&dir).await.expect("dir should be created");
        fs::write(dir.join(".cleanup-token"), "token")
            .await
            .expect("marker should be written");
        fs::write(dir.join("clip.mp4.part"), b"partial")
            .await
            .expect("partial file should be written");
        fs::write(dir.join("clip.mp4"), b"complete-video")
            .await
            .expect("media file should be written");

        let file = find_primary_download_file(&dir)
            .await
            .expect("lookup should succeed")
            .expect("file should exist");

        assert_eq!(
            file.file_name().and_then(|name| name.to_str()),
            Some("clip.mp4")
        );

        fs::remove_dir_all(&dir).await.expect("dir should be removed");
    }

    #[tokio::test]
    async fn cleanup_only_removes_the_latest_lease_holder() {
        let dir = unique_temp_dir("cleanup-token");
        fs::create_dir_all(&dir).await.expect("dir should be created");
        fs::write(dir.join("clip.mp4"), b"video")
            .await
            .expect("media file should be written");

        let first = refresh_download_lease(&dir)
            .await
            .expect("first token should be written");
        let second = refresh_download_lease(&dir)
            .await
            .expect("second token should be written");

        cleanup_download_dir(dir.clone(), first).await;
        assert!(
            fs::try_exists(&dir).await.expect("dir lookup should succeed"),
            "stale cleanup should not delete active download dir"
        );

        cleanup_download_dir(dir.clone(), second).await;
        assert!(
            !fs::try_exists(&dir).await.expect("dir lookup should succeed"),
            "latest cleanup should remove the download dir"
        );
    }

    #[test]
    fn normalizes_unknown_media_extensions_for_client_downloads() {
        let filename = public_filename(Path::new("clip.unknown_video"))
            .expect("filename normalization should succeed");
        assert_eq!(filename, "clip.mp4");

        let filename = public_filename(Path::new("track.unknown_audio"))
            .expect("filename normalization should succeed");
        assert_eq!(filename, "track.m4a");
    }

    #[test]
    fn youtube_format_prefers_progressive_then_capped_adaptive_streams() {
        let format = youtube_preferred_format(true);

        assert!(
            format.starts_with(&format!(
                "best[ext=mp4][height<={YOUTUBE_PROGRESSIVE_MAX_HEIGHT}]"
            )),
            "youtube selector should prefer progressive mp4 downloads first"
        );
        assert!(
            format.contains(&format!(
                "bestvideo[ext=mp4][vcodec^=avc1][height<={YOUTUBE_ADAPTIVE_MAX_HEIGHT}]+bestaudio[ext=m4a][acodec^=mp4a]"
            )),
            "youtube selector should cap adaptive merges and prefer widely compatible codecs"
        );
    }

    #[tokio::test]
    async fn stores_and_reloads_the_exact_downloaded_filepath() {
        let dir = unique_temp_dir("download-marker");
        fs::create_dir_all(&dir).await.expect("dir should be created");
        let file = dir.join("clip.mp4");
        fs::write(&file, b"video")
            .await
            .expect("media file should be written");

        persist_downloaded_file_path(&dir, &file)
            .await
            .expect("download marker should be written");
        let loaded = load_downloaded_file_path(&dir)
            .await
            .expect("download marker lookup should succeed")
            .expect("downloaded file path should exist");

        assert_eq!(loaded, file);

        fs::remove_dir_all(&dir).await.expect("dir should be removed");
    }

    #[test]
    fn parses_after_move_filepath_from_yt_dlp_stdout() {
        let dir = Path::new("/tmp/videodownloaderbackend/test-id");
        let stdout = b"\n/tmp/videodownloaderbackend/test-id/clip.mp4\n";

        let path = extract_downloaded_file_path(stdout, dir)
            .expect("stdout should contain a final filepath");

        assert_eq!(path, dir.join("clip.mp4"));
    }

    #[test]
    fn only_passes_ffmpeg_location_when_the_value_is_a_real_path() {
        assert_eq!(ffmpeg_location_argument("ffmpeg"), None);
        assert_eq!(
            ffmpeg_location_argument("/opt/homebrew/bin/ffmpeg"),
            Some(OsString::from("/opt/homebrew/bin/ffmpeg"))
        );
        assert_eq!(ffmpeg_location_argument("./bin/ffmpeg"), None);
    }

    #[test]
    fn falls_back_to_path_lookup_for_missing_absolute_ytdlp_path() {
        let resolved = resolve_ytdlp_command(Path::new("/definitely-missing/bin/yt-dlp"))
            .expect("missing absolute path should fall back to the binary name");
        assert_eq!(resolved, OsString::from("yt-dlp"));
    }

    #[test]
    fn keeps_plain_ytdlp_command_names() {
        let resolved = resolve_ytdlp_command(Path::new("yt-dlp"))
            .expect("plain command names should be accepted");
        assert_eq!(resolved, OsString::from("yt-dlp"));
    }
}
