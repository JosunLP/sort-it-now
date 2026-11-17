use reqwest::StatusCode;
use reqwest::header::HeaderMap;
use serde::Deserialize;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::os::unix::fs::PermissionsExt;
#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
use std::path::Path;
#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
use std::path::PathBuf;

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
use sha2::{Digest, Sha256};
#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
use tokio::fs;
#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
use tokio::io::AsyncWriteExt;
#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
use tokio::task;
use tokio::task::JoinHandle;
#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
use tokio_stream::StreamExt;

use crate::config::UpdateConfig;
fn user_agent() -> String {
    let version = env!("CARGO_PKG_VERSION");
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;
    format!("sort-it-now/{version} ({os}; {arch})")
}

#[derive(Debug, Deserialize)]
struct ReleaseAsset {
    name: String,
    browser_download_url: String,
}

#[derive(Debug, Deserialize)]
struct ReleaseResponse {
    tag_name: String,
    html_url: String,
    assets: Vec<ReleaseAsset>,
}

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
struct TempDirCleanup {
    dir: Option<tempfile::TempDir>,
}

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
impl TempDirCleanup {
    fn new(dir: tempfile::TempDir) -> Self {
        Self { dir: Some(dir) }
    }

    fn path(&self) -> &Path {
        self.dir
            .as_ref()
            .expect("temporary directory already cleaned up")
            .path()
    }

    fn cleanup(&mut self) {
        if let Some(dir) = self.dir.take()
            && let Err(err) = dir.close()
        {
            eprintln!("⚠️ Could not remove temporary directory: {}", err);
        }
    }

    fn close(mut self) -> Result<(), std::io::Error> {
        if let Some(dir) = self.dir.take() {
            dir.close()
        } else {
            Ok(())
        }
    }
}

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
impl Drop for TempDirCleanup {
    fn drop(&mut self) {
        self.cleanup();
    }
}

/// How the background updater should behave.
///
/// Controlled by the `SORT_IT_NOW_UPDATE_MODE` environment variable:
/// * `install` (default) — check for a newer release and install it automatically.
/// * `notify` — check for a newer release but only report it; never download or install.
/// * `off` — skip the update check entirely.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum UpdateMode {
    Install,
    Notify,
    Off,
}

fn update_mode() -> UpdateMode {
    const VAR: &str = "SORT_IT_NOW_UPDATE_MODE";
    match std::env::var(VAR) {
        Ok(value) => match value.trim().to_ascii_lowercase().as_str() {
            "" | "install" | "auto" => UpdateMode::Install,
            "notify" | "check" => UpdateMode::Notify,
            "off" | "disabled" | "none" => UpdateMode::Off,
            other => {
                eprintln!(
                    "⚠️ Unknown {VAR} value '{other}'. Expected 'install', 'notify', or 'off'. Using 'install'."
                );
                UpdateMode::Install
            }
        },
        Err(std::env::VarError::NotPresent) => UpdateMode::Install,
        Err(err) => {
            eprintln!("⚠️ Failed to access {VAR}: {err}. Using 'install'.");
            UpdateMode::Install
        }
    }
}

/// Launches an asynchronous update check without waiting for completion.
///
/// The returned handle can be awaited if the caller wants to observe
/// the outcome; otherwise, dropping it keeps the task running in the
/// background.
pub fn check_for_updates_background(update_config: UpdateConfig) -> Option<JoinHandle<()>> {
    if std::env::var("SORT_IT_NOW_SKIP_UPDATE_CHECK").is_ok() {
        println!("ℹ️ Update check disabled (SORT_IT_NOW_SKIP_UPDATE_CHECK set).");
        return None;
    }

    let mode = update_mode();
    if mode == UpdateMode::Off {
        println!("ℹ️ Update check disabled (SORT_IT_NOW_UPDATE_MODE=off).");
        return None;
    }

    Some(tokio::spawn(async move {
        if let Err(err) = check_for_updates(&update_config, mode).await {
            eprintln!("⚠️ Update check failed: {err}");
        }
    }))
}

async fn check_for_updates(
    config: &UpdateConfig,
    mode: UpdateMode,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let token = github_token();
    // Use per-phase timeouts (connect + per-read) instead of a total request deadline: a total
    // deadline of e.g. 30s would abort large release-artifact downloads mid-stream on ordinary
    // connections, while per-read timeouts still detect stalled transfers.
    let client = reqwest::Client::builder()
        .connect_timeout(http_timeout())
        .read_timeout(http_timeout())
        .user_agent(user_agent())
        .build()?;

    let url = config.latest_release_endpoint();

    let mut request = client.get(&url);
    if let Some(ref token) = token {
        request = request.bearer_auth(token);
    }

    let response = request.send().await?;
    let status = response.status();

    if status == StatusCode::FORBIDDEN {
        let headers = response.headers().clone();
        if is_rate_limit_response(&headers) {
            let mut message = String::from("⏱️ GitHub rate limit reached. Update check skipped.");
            if let Some(wait) = rate_limit_reset_duration(&headers) {
                message.push_str(&format!(" Please try again in {}.", format_wait(wait)));
            }
            println!("{message}");
            if token.is_none() {
                println!(
                    "💡 Tip: Set SORT_IT_NOW_GITHUB_TOKEN or GITHUB_TOKEN with a personal access token to increase the limit."
                );
            }
            return Ok(());
        }

        let body = match response.text().await {
            Ok(body) => body,
            Err(_) => String::from("unknown response"),
        };
        return Err(format!("GitHub API responded with 403 Forbidden: {body}").into());
    }

    // GitHub signals secondary rate limits with 429 + `retry-after`; treat that as a graceful
    // skip like the primary rate limit above instead of surfacing a scary error.
    if status == StatusCode::TOO_MANY_REQUESTS {
        let headers = response.headers().clone();
        let mut message = String::from("⏱️ GitHub rate limit reached (429). Update check skipped.");
        let wait = retry_after_duration(&headers).or_else(|| rate_limit_reset_duration(&headers));
        if let Some(wait) = wait {
            message.push_str(&format!(" Please try again in {}.", format_wait(wait)));
        }
        println!("{message}");
        return Ok(());
    }

    if status == StatusCode::UNAUTHORIZED {
        eprintln!(
            "⚠️ GitHub rejected the token (401 Unauthorized). Check SORT_IT_NOW_GITHUB_TOKEN or GITHUB_TOKEN."
        );
        return Ok(());
    }

    if status == StatusCode::NOT_FOUND {
        println!(
            "ℹ️ Could not find a release for {}/{} (404 Not Found).",
            config.owner(),
            config.repo()
        );
        return Ok(());
    }

    let response = response.error_for_status()?;
    let release: ReleaseResponse = response.json().await?;

    let latest = release.tag_name.trim_start_matches('v');
    let current = env!("CARGO_PKG_VERSION");

    match (
        semver::Version::parse(current),
        semver::Version::parse(latest),
    ) {
        (Ok(current_ver), Ok(latest_ver)) if latest_ver > current_ver => {
            println!(
                "✨ A new version ({}) is available! Download it at {}.",
                release.tag_name, release.html_url
            );
            if mode == UpdateMode::Notify {
                println!("ℹ️ Automatic installation is disabled (SORT_IT_NOW_UPDATE_MODE=notify).");
                return Ok(());
            }
            println!(
                "🛠️ Preparing automatic update to {} – downloading and installing release artifact.",
                release.tag_name
            );
            if let Err(err) = download_and_install_update(&client, &release, token.as_deref()).await
            {
                eprintln!("⚠️ Automatic update failed: {err}");
            } else {
                println!("✅ Update to {} installed.", release.tag_name);
            }
        }
        (Ok(_), Ok(_)) => {
            println!("✅ You are using the latest version (v{current}).");
        }
        _ => {
            println!(
                "ℹ️ Could not perform version comparison. Current: v{current}, Server: {}",
                release.tag_name
            );
        }
    }

    Ok(())
}

async fn download_and_install_update(
    client: &reqwest::Client,
    release: &ReleaseResponse,
    auth_token: Option<&str>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        let _ = auth_token;
        println!("ℹ️ Automatic updates are not supported on this operating system.");
        return Ok(());
    }

    #[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
    {
        // The tag name flows into asset names and filesystem paths below; reject anything that
        // is not a plain version-like identifier before touching the filesystem with it.
        if !is_safe_tag(&release.tag_name) {
            return Err(format!(
                "Release tag '{}' contains unexpected characters; refusing to install.",
                release.tag_name
            )
            .into());
        }

        let asset_names = expected_asset_names(&release.tag_name);
        let asset = release
            .assets
            .iter()
            .find(|asset| asset_names.iter().any(|candidate| candidate == &asset.name))
            .ok_or_else(|| {
                let candidates = asset_names.join(", ");
                format!("Could not find release asset. Expected names: {candidates}")
            })?;

        let checksum_asset =
            find_checksum_asset(&release.assets, &asset.name).ok_or_else(|| {
                let expected = checksum_asset_names(&asset.name).join(", ");
                format!("Could not find checksum file. Expected names: {}", expected)
            })?;

        println!("⬇️ Downloading update package {}...", asset.name);
        println!("🔒 Downloading checksum {}...", checksum_asset.name);

        let expected_checksum =
            fetch_checksum(client, checksum_asset, &asset.name, auth_token).await?;
        println!("🔐 Verifying SHA-256 checksum for {}.", asset.name);

        let mut request = client.get(&asset.browser_download_url);
        if let Some(token) = auth_token {
            request = request.bearer_auth(token);
        }

        let response = request.send().await?.error_for_status()?;
        let limit = max_download_size_bytes();
        let mut hasher = Sha256::new();
        let mut temp_dir = TempDirCleanup::new(tempfile::tempdir()?);

        if let (Some(limit_bytes), Some(content_length)) = (limit, response.content_length())
            && content_length > limit_bytes
        {
            temp_dir.cleanup();
            return Err(format!(
                "Release asset {} exceeds download limit of {} MB",
                asset.name,
                limit_bytes / (1024 * 1024)
            )
            .into());
        }

        let archive_path = temp_dir.path().join(&asset.name);
        let mut file = fs::File::create(&archive_path).await?;
        let mut downloaded: u64 = 0;
        let mut stream = response.bytes_stream();

        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            downloaded += chunk.len() as u64;
            if let Some(limit_bytes) = limit
                && downloaded > limit_bytes
            {
                drop(file);
                let _ = fs::remove_file(&archive_path).await;
                temp_dir.cleanup();
                return Err(format!(
                    "Release asset {} exceeds download limit of {} MB",
                    asset.name,
                    limit_bytes / (1024 * 1024)
                )
                .into());
            }
            hasher.update(&chunk);
            file.write_all(&chunk).await?;
        }
        file.flush().await?;

        let computed_checksum: String = hasher
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        if computed_checksum != expected_checksum {
            drop(file);
            let _ = fs::remove_file(&archive_path).await;
            temp_dir.cleanup();
            return Err(format!(
                "Checksum mismatch (expected {}, got {}). Update aborted.",
                expected_checksum, computed_checksum
            )
            .into());
        }

        #[cfg(target_os = "linux")]
        install_on_linux(&archive_path, temp_dir.path(), &release.tag_name).await?;

        #[cfg(target_os = "macos")]
        install_on_macos(&archive_path, temp_dir.path(), &release.tag_name).await?;

        #[cfg(target_os = "windows")]
        install_on_windows(&archive_path, temp_dir.path(), &release.tag_name).await?;

        temp_dir.close()?;

        Ok(())
    }
}

#[cfg(target_os = "linux")]
const TARGET_SUFFIX: &str = "linux-x86_64";
#[cfg(target_os = "linux")]
const TARGET_EXTENSION: &str = "tar.gz";

#[cfg(target_os = "macos")]
const TARGET_EXTENSION: &str = "tar.gz";

#[cfg(all(target_os = "macos", target_arch = "x86_64"))]
const TARGET_SUFFIX: &str = "macos-x86_64";

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
const TARGET_SUFFIX: &str = "macos-arm64";

#[cfg(all(
    target_os = "macos",
    not(any(target_arch = "x86_64", target_arch = "aarch64"))
))]
compile_error!(
    "Unsupported macOS architecture for updates; build target must be x86_64 or aarch64."
);

#[cfg(target_os = "windows")]
const TARGET_SUFFIX: &str = "windows-x86_64";
#[cfg(target_os = "windows")]
const TARGET_EXTENSION: &str = "zip";

fn expected_asset_names(tag: &str) -> Vec<String> {
    #[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
    {
        let mut candidates = Vec::new();
        let base = format!("sort-it-now-{}-{}.{}", tag, TARGET_SUFFIX, TARGET_EXTENSION);
        candidates.push(base);

        let trimmed = tag.trim_start_matches('v');
        if trimmed != tag {
            candidates.push(format!(
                "sort-it-now-{}-{}.{}",
                trimmed, TARGET_SUFFIX, TARGET_EXTENSION
            ));
        } else if !trimmed.is_empty() {
            candidates.push(format!(
                "sort-it-now-v{}-{}.{}",
                trimmed, TARGET_SUFFIX, TARGET_EXTENSION
            ));
        }

        candidates
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        let _ = tag;
        Vec::new()
    }
}

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
fn checksum_asset_names(asset_name: &str) -> Vec<String> {
    vec![
        format!("{}.sha256", asset_name),
        format!("{}.sha256sum", asset_name),
        format!("{}.sha256.txt", asset_name),
    ]
}

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
fn find_checksum_asset<'a>(
    assets: &'a [ReleaseAsset],
    asset_name: &str,
) -> Option<&'a ReleaseAsset> {
    let candidates = checksum_asset_names(asset_name);
    assets
        .iter()
        .find(|asset| candidates.iter().any(|candidate| candidate == &asset.name))
}

/// Validates that a release tag is a plain version-like identifier.
///
/// The tag is interpolated into expected asset names and filesystem paths during installation,
/// so anything beyond alphanumerics, dots, underscores, and hyphens is rejected outright.
fn is_safe_tag(tag: &str) -> bool {
    !tag.is_empty()
        && tag.len() <= 128
        && tag
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
fn parse_checksum_file(contents: &str, expected_name: &str) -> Option<String> {
    for line in contents.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let mut fields = trimmed.split_whitespace();
        let hash = fields.next()?;
        if hash.len() != 64 || !hash.chars().all(|c| c.is_ascii_hexdigit()) {
            continue;
        }
        // `sha256sum`-style files carry the filename as a second field (prefixed with `*` in
        // binary mode). When present it must match the asset we are verifying, otherwise a
        // multi-entry checksum file could silently verify the wrong artifact.
        if let Some(name_field) = fields.next() {
            let name = name_field.trim_start_matches('*');
            if name != expected_name {
                continue;
            }
        }
        return Some(hash.to_ascii_lowercase());
    }
    None
}

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
async fn fetch_checksum(
    client: &reqwest::Client,
    checksum_asset: &ReleaseAsset,
    expected_asset_name: &str,
    auth_token: Option<&str>,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    /// A checksum file is a few lines of hex digests; anything larger is not a checksum file.
    const MAX_CHECKSUM_BYTES: usize = 64 * 1024;

    let mut request = client.get(&checksum_asset.browser_download_url);
    if let Some(token) = auth_token {
        request = request.bearer_auth(token);
    }

    let response = request.send().await?.error_for_status()?;
    if let Some(length) = response.content_length()
        && length > MAX_CHECKSUM_BYTES as u64
    {
        return Err(format!(
            "Checksum asset {} is unexpectedly large ({} bytes).",
            checksum_asset.name, length
        )
        .into());
    }

    let mut body = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        if body.len() + chunk.len() > MAX_CHECKSUM_BYTES {
            return Err(format!(
                "Checksum asset {} exceeds the {} byte limit.",
                checksum_asset.name, MAX_CHECKSUM_BYTES
            )
            .into());
        }
        body.extend_from_slice(&chunk);
    }

    let body = String::from_utf8_lossy(&body);
    let expected = parse_checksum_file(&body, expected_asset_name)
        .ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "Could not find valid SHA-256 checksum for {} in {}.",
                    expected_asset_name, checksum_asset.name
                ),
            )
        })
        .map_err(|err| Box::new(err) as Box<dyn std::error::Error + Send + Sync>)?;

    Ok(expected)
}

fn max_download_size_bytes() -> Option<u64> {
    const DEFAULT_LIMIT_MB: u64 = 200;
    match std::env::var("SORT_IT_NOW_MAX_DOWNLOAD_MB") {
        Ok(value) => {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                Some(DEFAULT_LIMIT_MB * 1024 * 1024)
            } else if let Ok(parsed) = trimmed.parse::<u64>() {
                if parsed == 0 {
                    None
                } else {
                    Some(parsed * 1024 * 1024)
                }
            } else {
                eprintln!(
                    "⚠️ Could not parse SORT_IT_NOW_MAX_DOWNLOAD_MB ('{}'). Using default limit {} MB.",
                    trimmed, DEFAULT_LIMIT_MB
                );
                Some(DEFAULT_LIMIT_MB * 1024 * 1024)
            }
        }
        Err(std::env::VarError::NotPresent) => Some(DEFAULT_LIMIT_MB * 1024 * 1024),
        Err(err) => {
            eprintln!(
                "⚠️ Failed to access SORT_IT_NOW_MAX_DOWNLOAD_MB: {err}. Using default limit {} MB.",
                DEFAULT_LIMIT_MB
            );
            Some(DEFAULT_LIMIT_MB * 1024 * 1024)
        }
    }
}

fn http_timeout() -> Duration {
    const DEFAULT_TIMEOUT_SECS: u64 = 30;
    match std::env::var("SORT_IT_NOW_HTTP_TIMEOUT_SECS") {
        Ok(value) => {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                Duration::from_secs(DEFAULT_TIMEOUT_SECS)
            } else if let Ok(parsed) = trimmed.parse::<u64>() {
                if parsed == 0 {
                    Duration::from_secs(DEFAULT_TIMEOUT_SECS)
                } else {
                    Duration::from_secs(parsed)
                }
            } else {
                eprintln!(
                    "⚠️ Could not parse SORT_IT_NOW_HTTP_TIMEOUT_SECS ('{}'). Using default timeout {}s.",
                    trimmed, DEFAULT_TIMEOUT_SECS
                );
                Duration::from_secs(DEFAULT_TIMEOUT_SECS)
            }
        }
        Err(std::env::VarError::NotPresent) => Duration::from_secs(DEFAULT_TIMEOUT_SECS),
        Err(err) => {
            eprintln!(
                "⚠️ Failed to access SORT_IT_NOW_HTTP_TIMEOUT_SECS: {err}. Using default timeout {}s.",
                DEFAULT_TIMEOUT_SECS
            );
            Duration::from_secs(DEFAULT_TIMEOUT_SECS)
        }
    }
}

fn github_token() -> Option<String> {
    env_token("SORT_IT_NOW_GITHUB_TOKEN").or_else(|| env_token("GITHUB_TOKEN"))
}

fn env_token(name: &str) -> Option<String> {
    match std::env::var(name) {
        Ok(value) => {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                eprintln!(
                    "⚠️ Environment variable {} is set but contains no value.",
                    name
                );
                None
            } else {
                Some(trimmed.to_owned())
            }
        }
        Err(std::env::VarError::NotPresent) => None,
        Err(err) => {
            eprintln!("⚠️ Failed to access {}: {}. Ignoring value.", name, err);
            None
        }
    }
}

fn is_rate_limit_response(headers: &HeaderMap) -> bool {
    headers
        .get("x-ratelimit-remaining")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
        .is_some_and(|remaining| remaining == 0)
}

/// Parses a `retry-after` header given in whole seconds (the form GitHub uses).
fn retry_after_duration(headers: &HeaderMap) -> Option<Duration> {
    let seconds = headers
        .get("retry-after")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.trim().parse::<u64>().ok())?;
    if seconds == 0 {
        None
    } else {
        Some(Duration::from_secs(seconds))
    }
}

fn rate_limit_reset_duration(headers: &HeaderMap) -> Option<Duration> {
    let reset_epoch = headers
        .get("x-ratelimit-reset")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())?;

    let now = SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_secs();

    let wait_secs = reset_epoch.saturating_sub(now);
    if wait_secs == 0 {
        None
    } else {
        Some(Duration::from_secs(wait_secs))
    }
}

fn format_wait(duration: Duration) -> String {
    let total_secs = duration.as_secs();
    let hours = total_secs / 3600;
    let minutes = (total_secs % 3600) / 60;
    let seconds = total_secs % 60;

    let mut parts = Vec::new();
    if hours > 0 {
        parts.push(format!("{}h", hours));
    }
    if minutes > 0 {
        parts.push(format!("{}m", minutes));
    }
    if seconds > 0 || parts.is_empty() {
        parts.push(format!("{}s", seconds));
    }

    parts.join(" ")
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
async fn install_on_unix(
    archive_path: &Path,
    extract_root: &Path,
    tag_name: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let archive_path = archive_path.to_path_buf();
    let extract_root = extract_root.to_path_buf();

    task::spawn_blocking({
        let extract_root = extract_root.clone();
        move || -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            let file = std::fs::File::open(&archive_path)?;
            let decoder = flate2::read::GzDecoder::new(file);
            let mut archive = tar::Archive::new(decoder);
            archive.unpack(&extract_root)?;
            Ok(())
        }
    })
    .await??;

    let bundle_dir = bundle_directory(&extract_root, tag_name);
    let binary_path = bundle_dir.join("sort_it_now");
    if !binary_path.exists() {
        return Err("Binary sort_it_now was not found in the extracted package".into());
    }

    let current_exe = std::env::current_exe()?;
    let install_dir = current_exe
        .parent()
        .ok_or("Could not determine installation directory")?
        .to_path_buf();
    // Replace the binary that is actually running (whatever it is named) instead of assuming a
    // hardcoded file name. Otherwise a renamed or repackaged install would receive a new file
    // next to the old one, report success, and keep running (and re-downloading) the old build.
    let exe_name = current_exe
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .ok_or("Could not determine executable file name")?;

    let staged_path = install_dir.join(format!("{exe_name}.tmp"));
    let final_path = current_exe.clone();
    if let Err(err) = fs::remove_file(&staged_path).await
        && err.kind() != std::io::ErrorKind::NotFound
    {
        return Err(err.into());
    }

    let next_launch_path = install_dir.join(format!("{exe_name}.new"));
    if let Err(err) = fs::remove_file(&next_launch_path).await
        && err.kind() != std::io::ErrorKind::NotFound
    {
        return Err(err.into());
    }

    fs::copy(&binary_path, &staged_path).await?;
    let metadata = fs::metadata(&staged_path).await?;
    let mut permissions = metadata.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&staged_path, permissions).await?;

    if let Err(err) = fs::rename(&staged_path, &final_path).await {
        if err.kind() == std::io::ErrorKind::PermissionDenied {
            let _ = fs::remove_file(&next_launch_path).await;
            fs::rename(&staged_path, &next_launch_path).await?;
            println!("⚠️ Could not replace the running application: {}.", err);
            println!(
                "💡 The updated version was saved as {}. Rename it to {} after a restart.",
                next_launch_path.display(),
                exe_name
            );
            return Ok(());
        }

        let _ = fs::remove_file(&staged_path).await;
        return Err(err.into());
    }

    println!(
        "✅ Update to {} installed (installation target: {}).",
        tag_name,
        install_dir.display()
    );
    Ok(())
}

#[cfg(target_os = "linux")]
async fn install_on_linux(
    archive_path: &Path,
    extract_root: &Path,
    tag_name: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    install_on_unix(archive_path, extract_root, tag_name).await
}

#[cfg(target_os = "macos")]
async fn install_on_macos(
    archive_path: &Path,
    extract_root: &Path,
    tag_name: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    install_on_unix(archive_path, extract_root, tag_name).await
}

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
fn bundle_directory(extract_root: &Path, tag_name: &str) -> PathBuf {
    extract_root.join(format!("sort-it-now-{}-{}", tag_name, TARGET_SUFFIX))
}

#[cfg(target_os = "windows")]
async fn install_on_windows(
    archive_path: &Path,
    extract_root: &Path,
    tag_name: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let archive_path = archive_path.to_path_buf();
    let extract_root = extract_root.to_path_buf();

    task::spawn_blocking({
        let extract_root = extract_root.clone();
        move || -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            let file = std::fs::File::open(&archive_path)?;
            let mut archive = zip::ZipArchive::new(file)?;
            archive.extract(&extract_root)?;
            Ok(())
        }
    })
    .await??;

    let bundle_dir = bundle_directory(&extract_root, tag_name);
    let binary_path = bundle_dir.join("sort_it_now.exe");
    if !binary_path.exists() {
        return Err("Binary sort_it_now.exe was not found in the extracted package".into());
    }

    let current_exe = std::env::current_exe()?;
    let install_dir = current_exe
        .parent()
        .ok_or("Could not determine installation directory")?
        .to_path_buf();
    // Replace the binary that is actually running (whatever it is named); see install_on_unix.
    let exe_name = current_exe
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .ok_or("Could not determine executable file name")?;
    let target_path = current_exe.clone();

    match fs::copy(&binary_path, &target_path).await {
        Ok(_) => {
            copy_readme_if_present(&bundle_dir, &install_dir).await;
            match ensure_windows_path(&install_dir) {
                Ok(true) => println!(
                    "ℹ️ The installation directory was added to user PATH. You may need to open a new terminal."
                ),
                Ok(false) => {}
                Err(err) => eprintln!(
                    "⚠️ Could not update PATH: {}. Add {} manually.",
                    err,
                    install_dir.display()
                ),
            }
            println!(
                "✅ Update to {} installed (installation target: {}).",
                tag_name,
                install_dir.display()
            );
            println!("ℹ️ Start the service with: {exe_name}");
            Ok(())
        }
        Err(err) => {
            let raw = err.raw_os_error();
            if err.kind() == std::io::ErrorKind::PermissionDenied
                || matches!(raw, Some(5) | Some(32))
            {
                let staged_path = install_dir.join(format!("{exe_name}.new"));
                if let Err(remove_err) = fs::remove_file(&staged_path).await
                    && remove_err.kind() != std::io::ErrorKind::NotFound
                {
                    return Err(remove_err.into());
                }
                fs::copy(&binary_path, &staged_path).await?;
                println!("⚠️ Could not replace the running application: {}.", err);
                println!(
                    "💡 The updated version was saved as {}. Rename it to {} after a restart.",
                    staged_path.display(),
                    exe_name
                );
                Ok(())
            } else {
                Err(err.into())
            }
        }
    }
}

#[cfg(target_os = "windows")]
async fn copy_readme_if_present(bundle_dir: &Path, install_dir: &Path) {
    let readme_src = bundle_dir.join("README.md");
    if !readme_src.exists() {
        return;
    }

    let readme_dst = install_dir.join("README.md");
    if let Err(err) = fs::copy(&readme_src, &readme_dst).await {
        eprintln!("⚠️ Could not update README.md: {}", err);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_tags_accept_version_like_identifiers() {
        assert!(is_safe_tag("v1.4.0"));
        assert!(is_safe_tag("1.4.0-rc.1"));
        assert!(is_safe_tag("release_2026-07-03"));
    }

    #[test]
    fn unsafe_tags_are_rejected() {
        assert!(!is_safe_tag(""));
        assert!(!is_safe_tag("../../etc"));
        assert!(!is_safe_tag("v1.0/evil"));
        assert!(!is_safe_tag("v1.0 evil"));
        assert!(!is_safe_tag(&"a".repeat(129)));
    }

    #[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
    #[test]
    fn checksum_parser_accepts_bare_hash() {
        let hash = "a".repeat(64);
        assert_eq!(
            parse_checksum_file(&hash, "artifact.tar.gz"),
            Some(hash.clone())
        );
    }

    #[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
    #[test]
    fn checksum_parser_matches_filename_in_multi_entry_files() {
        let contents = format!(
            "# comment\n{}  other-file.tar.gz\n{}  *artifact.tar.gz\n",
            "b".repeat(64),
            "c".repeat(64)
        );
        // The entry for the requested artifact wins, not the first syntactically valid hash.
        assert_eq!(
            parse_checksum_file(&contents, "artifact.tar.gz"),
            Some("c".repeat(64))
        );
        assert_eq!(parse_checksum_file(&contents, "missing.tar.gz"), None);
    }

    #[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
    #[test]
    fn checksum_parser_normalizes_case_and_skips_invalid_lines() {
        let contents = format!("not-a-hash\n{}  artifact.tar.gz\n", "AB".repeat(32));
        assert_eq!(
            parse_checksum_file(&contents, "artifact.tar.gz"),
            Some("ab".repeat(32))
        );
    }

    #[test]
    fn retry_after_header_parses_whole_seconds() {
        let mut headers = HeaderMap::new();
        headers.insert("retry-after", "120".parse().unwrap());
        assert_eq!(
            retry_after_duration(&headers),
            Some(Duration::from_secs(120))
        );

        headers.insert("retry-after", "0".parse().unwrap());
        assert_eq!(retry_after_duration(&headers), None);

        headers.insert("retry-after", "soon".parse().unwrap());
        assert_eq!(retry_after_duration(&headers), None);
    }

    #[test]
    fn format_wait_renders_compound_durations() {
        assert_eq!(format_wait(Duration::from_secs(0)), "0s");
        assert_eq!(format_wait(Duration::from_secs(61)), "1m 1s");
        assert_eq!(format_wait(Duration::from_secs(3661)), "1h 1m 1s");
    }
}

#[cfg(target_os = "windows")]
fn ensure_windows_path(install_dir: &Path) -> Result<bool, std::io::Error> {
    use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_WRITE, REG_EXPAND_SZ, REG_SZ};
    use winreg::{RegKey, RegValue};

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let env = hkcu.open_subkey_with_flags("Environment", KEY_READ | KEY_WRITE)?;

    // The user PATH is conventionally REG_EXPAND_SZ (it may contain %VAR% references). Preserve
    // the existing value type when rewriting: writing it back as plain REG_SZ would stop those
    // references from being expanded and silently corrupt the user's PATH.
    let existing = env.get_raw_value("Path").ok();
    let value_type = existing
        .as_ref()
        .map(|value| value.vtype.clone())
        .unwrap_or(REG_EXPAND_SZ);
    if !matches!(value_type, REG_SZ | REG_EXPAND_SZ) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "user PATH registry value has an unexpected type; not modifying it",
        ));
    }

    let current: String = env.get_value("Path").unwrap_or_default();
    let install_str = install_dir.to_string_lossy();
    let already_present = current
        .split(';')
        .map(|part| part.trim())
        .any(|part| part.eq_ignore_ascii_case(&install_str));

    if already_present {
        return Ok(false);
    }

    let new_path = if current.trim().is_empty() {
        install_str.to_string()
    } else {
        format!("{};{}", current, install_str)
    };

    // Encode as null-terminated UTF-16LE, the on-disk representation of both string types.
    let bytes: Vec<u8> = new_path
        .encode_utf16()
        .chain(std::iter::once(0))
        .flat_map(u16::to_le_bytes)
        .collect();
    env.set_raw_value(
        "Path",
        &RegValue {
            bytes: bytes.into(),
            vtype: value_type,
        },
    )?;
    Ok(true)
}
