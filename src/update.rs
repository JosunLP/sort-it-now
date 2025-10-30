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
        if let Some(dir) = self.dir.take() {
            if let Err(err) = dir.close() {
                eprintln!("⚠️ Konnte temporäres Verzeichnis nicht entfernen: {}", err);
            }
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

/// Launches an asynchronous update check without waiting for completion.
///
/// The returned handle can be awaited if the caller wants to observe
/// the outcome; otherwise, dropping it keeps the task running in the
/// background.
pub fn check_for_updates_background(update_config: UpdateConfig) -> Option<JoinHandle<()>> {
    if std::env::var("SORT_IT_NOW_SKIP_UPDATE_CHECK").is_ok() {
        println!("ℹ️ Update-Check deaktiviert (SORT_IT_NOW_SKIP_UPDATE_CHECK gesetzt).");
        return None;
    }

    Some(tokio::spawn(async move {
        if let Err(err) = check_for_updates(&update_config).await {
            eprintln!("⚠️ Update-Check fehlgeschlagen: {err}");
        }
    }))
}

async fn check_for_updates(
    config: &UpdateConfig,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let token = github_token();
    let client = reqwest::Client::builder()
        .timeout(http_timeout())
        .user_agent(&user_agent())
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
            let mut message =
                String::from("⏱️ GitHub-Rate-Limit erreicht. Der Update-Check wurde übersprungen.");
            if let Some(wait) = rate_limit_reset_duration(&headers) {
                message.push_str(&format!(
                    " Bitte versuche es in {} erneut.",
                    format_wait(wait)
                ));
            }
            println!("{message}");
            if token.is_none() {
                println!(
                    "💡 Tipp: Setze SORT_IT_NOW_GITHUB_TOKEN oder GITHUB_TOKEN mit einem persönlichen Zugriffstoken, um das Limit zu erhöhen."
                );
            }
            return Ok(());
        }

        let body = match response.text().await {
            Ok(body) => body,
            Err(_) => String::from("unbekannte Antwort"),
        };
        return Err(format!("GitHub-API antwortete mit 403 Forbidden: {body}").into());
    }

    if status == StatusCode::UNAUTHORIZED {
        eprintln!(
            "⚠️ GitHub hat das verwendete Token zurückgewiesen (401 Unauthorized). Prüfe SORT_IT_NOW_GITHUB_TOKEN oder GITHUB_TOKEN."
        );
        return Ok(());
    }

    if status == StatusCode::NOT_FOUND {
        println!(
            "ℹ️ Konnte kein Release für {}/{} finden (404 Not Found).",
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
                "✨ Eine neue Version ({}) ist verfügbar! Lade sie unter {} herunter.",
                release.tag_name, release.html_url
            );
            println!(
                "🛠️ Automatisches Update auf {} wird vorbereitet – Release-Artefakt wird heruntergeladen und installiert.",
                release.tag_name
            );
            if let Err(err) = download_and_install_update(&client, &release, token.as_deref()).await
            {
                eprintln!("⚠️ Automatisches Update fehlgeschlagen: {err}");
            } else {
                println!("✅ Update auf {} wurde installiert.", release.tag_name);
            }
        }
        (Ok(_), Ok(_)) => {
            println!("✅ Du verwendest die aktuelle Version (v{current}).");
        }
        _ => {
            println!(
                "ℹ️ Konnte Versionsvergleich nicht durchführen. Aktuell: v{current}, Server: {}",
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
        println!("ℹ️ Automatische Updates werden auf diesem Betriebssystem nicht unterstützt.");
        return Ok(());
    }

    #[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
    {
        let asset_names = expected_asset_names(&release.tag_name);
        let asset = release
            .assets
            .iter()
            .find(|asset| asset_names.iter().any(|candidate| candidate == &asset.name))
            .ok_or_else(|| {
                let candidates = asset_names.join(", ");
                format!("Konnte kein Release-Asset finden. Erwartete Namen: {candidates}")
            })?;

        let checksum_asset =
            find_checksum_asset(&release.assets, &asset.name).ok_or_else(|| {
                let expected = checksum_asset_names(&asset.name).join(", ");
                format!(
                    "Konnte keine Prüfsummen-Datei finden. Erwartete Namen: {}",
                    expected
                )
            })?;

        println!("⬇️ Lade Update-Paket {} herunter...", asset.name);
        println!("🔒 Lade Prüfsumme {} ...", checksum_asset.name);

        let expected_checksum = fetch_checksum(client, checksum_asset, auth_token).await?;
        println!("🔐 Verifiziere SHA-256-Checksumme für {}.", asset.name);

        let mut request = client.get(&asset.browser_download_url);
        if let Some(token) = auth_token {
            request = request.bearer_auth(token);
        }

        let response = request.send().await?.error_for_status()?;
        let limit = max_download_size_bytes();
        let mut hasher = Sha256::new();
        let mut temp_dir = TempDirCleanup::new(tempfile::tempdir()?);

        if let (Some(limit_bytes), Some(content_length)) = (limit, response.content_length()) {
            if content_length > limit_bytes {
                temp_dir.cleanup();
                return Err(format!(
                    "Release-Asset {} überschreitet das Download-Limit von {} MB",
                    asset.name,
                    limit_bytes / (1024 * 1024)
                )
                .into());
            }
        }

        let archive_path = temp_dir.path().join(&asset.name);
        let mut file = fs::File::create(&archive_path).await?;
        let mut downloaded: u64 = 0;
        let mut stream = response.bytes_stream();

        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            downloaded += chunk.len() as u64;
            if let Some(limit_bytes) = limit {
                if downloaded > limit_bytes {
                    drop(file);
                    let _ = fs::remove_file(&archive_path).await;
                    temp_dir.cleanup();
                    return Err(format!(
                        "Release-Asset {} überschreitet das Download-Limit von {} MB",
                        asset.name,
                        limit_bytes / (1024 * 1024)
                    )
                    .into());
                }
            }
            hasher.update(&chunk);
            file.write_all(&chunk).await?;
        }
        file.flush().await?;

        let computed_checksum = format!("{:x}", hasher.finalize());
        if computed_checksum != expected_checksum {
            drop(file);
            let _ = fs::remove_file(&archive_path).await;
            temp_dir.cleanup();
            return Err(format!(
                "Checksumme stimmt nicht überein (erwartet {}, erhalten {}). Update abgebrochen.",
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

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
fn parse_checksum_file(contents: &str) -> Option<String> {
    for line in contents.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let hash = trimmed.split_whitespace().next()?;
        if hash.len() == 64 && hash.chars().all(|c| c.is_ascii_hexdigit()) {
            return Some(hash.to_ascii_lowercase());
        }
    }
    None
}

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
async fn fetch_checksum(
    client: &reqwest::Client,
    checksum_asset: &ReleaseAsset,
    auth_token: Option<&str>,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let mut request = client.get(&checksum_asset.browser_download_url);
    if let Some(token) = auth_token {
        request = request.bearer_auth(token);
    }

    let response = request.send().await?.error_for_status()?;
    let body = response.text().await?;
    let expected = parse_checksum_file(&body)
        .ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "Konnte gültige SHA-256-Checksumme in {} nicht finden.",
                    checksum_asset.name
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
                    "⚠️ Konnte SORT_IT_NOW_MAX_DOWNLOAD_MB ('{}') nicht parsen. Verwende Standardlimit {} MB.",
                    trimmed, DEFAULT_LIMIT_MB
                );
                Some(DEFAULT_LIMIT_MB * 1024 * 1024)
            }
        }
        Err(std::env::VarError::NotPresent) => Some(DEFAULT_LIMIT_MB * 1024 * 1024),
        Err(err) => {
            eprintln!(
                "⚠️ Zugriff auf SORT_IT_NOW_MAX_DOWNLOAD_MB fehlgeschlagen: {err}. Verwende Standardlimit {} MB.",
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
                    "⚠️ Konnte SORT_IT_NOW_HTTP_TIMEOUT_SECS ('{}') nicht parsen. Verwende Standardtimeout {}s.",
                    trimmed, DEFAULT_TIMEOUT_SECS
                );
                Duration::from_secs(DEFAULT_TIMEOUT_SECS)
            }
        }
        Err(std::env::VarError::NotPresent) => Duration::from_secs(DEFAULT_TIMEOUT_SECS),
        Err(err) => {
            eprintln!(
                "⚠️ Zugriff auf SORT_IT_NOW_HTTP_TIMEOUT_SECS fehlgeschlagen: {err}. Verwende Standardtimeout {}s.",
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
                    "⚠️ Umgebungsvariable {} ist gesetzt, enthält aber keinen Wert.",
                    name
                );
                None
            } else {
                Some(trimmed.to_owned())
            }
        }
        Err(std::env::VarError::NotPresent) => None,
        Err(err) => {
            eprintln!(
                "⚠️ Zugriff auf {} fehlgeschlagen: {}. Ignoriere Wert.",
                name, err
            );
            None
        }
    }
}

fn is_rate_limit_response(headers: &HeaderMap) -> bool {
    headers
        .get("x-ratelimit-remaining")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
        .map_or(false, |remaining| remaining == 0)
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
        return Err("Binärdatei sort_it_now wurde im entpackten Paket nicht gefunden".into());
    }

    let current_exe = std::env::current_exe()?;
    let install_dir = current_exe
        .parent()
        .ok_or("Konnte Installationsverzeichnis nicht bestimmen")?
        .to_path_buf();

    let staged_path = install_dir.join("sort_it_now.tmp");
    let final_path = install_dir.join("sort_it_now");
    if let Err(err) = fs::remove_file(&staged_path).await {
        if err.kind() != std::io::ErrorKind::NotFound {
            return Err(err.into());
        }
    }

    let next_launch_path = install_dir.join("sort_it_now.new");
    if let Err(err) = fs::remove_file(&next_launch_path).await {
        if err.kind() != std::io::ErrorKind::NotFound {
            return Err(err.into());
        }
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
            println!(
                "⚠️ Die laufende Anwendung konnte nicht ersetzt werden: {}.",
                err
            );
            println!(
                "💡 Die aktualisierte Version wurde als {} abgelegt. Benenne sie nach einem Neustart in sort_it_now um.",
                next_launch_path.display()
            );
            return Ok(());
        }

        let _ = fs::remove_file(&staged_path).await;
        return Err(err.into());
    }

    println!(
        "✅ Update nach {} installiert (Installationsziel: {}).",
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
        return Err("Binärdatei sort_it_now.exe wurde im entpackten Paket nicht gefunden".into());
    }

    let current_exe = std::env::current_exe()?;
    let install_dir = current_exe
        .parent()
        .ok_or("Konnte Installationsverzeichnis nicht bestimmen")?
        .to_path_buf();
    let target_path = install_dir.join("sort_it_now.exe");

    match fs::copy(&binary_path, &target_path).await {
        Ok(_) => {
            copy_readme_if_present(&bundle_dir, &install_dir).await;
            match ensure_windows_path(&install_dir) {
                Ok(true) => println!(
                    "ℹ️ Das Installationsverzeichnis wurde zum Benutzer-PATH hinzugefügt. Du musst eventuell ein neues Terminal öffnen."
                ),
                Ok(false) => {}
                Err(err) => eprintln!(
                    "⚠️ Konnte PATH nicht aktualisieren: {}. Füge {} manuell hinzu.",
                    err,
                    install_dir.display()
                ),
            }
            println!(
                "✅ Update nach {} installiert (Installationsziel: {}).",
                tag_name,
                install_dir.display()
            );
            println!("ℹ️ Starte den Dienst mit: sort_it_now.exe");
            Ok(())
        }
        Err(err) => {
            let raw = err.raw_os_error();
            if err.kind() == std::io::ErrorKind::PermissionDenied
                || matches!(raw, Some(5) | Some(32))
            {
                let staged_path = install_dir.join("sort_it_now.new.exe");
                if let Err(remove_err) = fs::remove_file(&staged_path).await {
                    if remove_err.kind() != std::io::ErrorKind::NotFound {
                        return Err(remove_err.into());
                    }
                }
                fs::copy(&binary_path, &staged_path).await?;
                println!(
                    "⚠️ Die laufende Anwendung konnte nicht ersetzt werden: {}.",
                    err
                );
                println!(
                    "💡 Die aktualisierte Version wurde als {} abgelegt. Benenne sie nach einem Neustart in sort_it_now.exe um.",
                    staged_path.display()
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
        eprintln!("⚠️ Konnte README.md nicht aktualisieren: {}", err);
    }
}

#[cfg(target_os = "windows")]
fn ensure_windows_path(install_dir: &Path) -> Result<bool, std::io::Error> {
    use winreg::RegKey;
    use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_WRITE};

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let env = hkcu.open_subkey_with_flags("Environment", KEY_READ | KEY_WRITE)?;

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

    env.set_value("Path", &new_path)?;
    Ok(true)
}
