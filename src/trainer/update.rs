//! Explicit GitHub release checks and staged executable replacement.
//! Requests carry no credentials, history, or settings.
use flate2::read::GzDecoder;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Read,
    path::{Component, Path, PathBuf},
    sync::mpsc,
    time::Duration,
};
use tar::Archive;

pub const RELEASES_URL: &str =
    "https://api.github.com/repos/jdavies69/openfelt/releases?per_page=20";
const MAX_RELEASE_LIST: usize = 1_000_000;
const MAX_ARCHIVE: usize = 64 * 1024 * 1024;
const MAX_BINARY: usize = 64 * 1024 * 1024;

pub fn current_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

pub fn target_triple() -> &'static str {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => "aarch64-apple-darwin",
        ("macos", "x86_64") => "x86_64-apple-darwin",
        ("linux", "x86_64") => "x86_64-unknown-linux-gnu",
        ("windows", "x86_64") => "x86_64-pc-windows-msvc",
        _ => "unsupported",
    }
}

pub fn archive_name(triple: &str) -> String {
    format!("openfelt-{triple}.tar.gz")
}

pub fn binary_name(triple: &str) -> &'static str {
    if triple.contains("windows") {
        "openfelt.exe"
    } else {
        "openfelt"
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    Managed,
    Cargo,
    Source,
    Package,
    Unsupported,
}

impl Origin {
    pub fn allows_replacement(self) -> bool {
        matches!(self, Self::Managed)
    }

    pub fn guidance(self) -> &'static str {
        match self {
            Self::Managed => "This install can be replaced from Settings.",
            Self::Cargo => "This copy was installed with Cargo. Upgrade with: cargo install --locked --git https://github.com/jdavies69/openfelt.git --bin openfelt",
            Self::Source => "This is a source build. Settings will not replace it. Use the release installer or: cargo install --locked --path . --bin openfelt",
            Self::Package => "This copy is managed by a package manager. Upgrade with that package manager or the release installer.",
            Self::Unsupported => "This path is not updated in place. Use the release installer documented in the README.",
        }
    }
}

pub fn classify_install(path: &Path) -> Origin {
    let key = path
        .to_string_lossy()
        .replace('\\', "/")
        .to_ascii_lowercase();
    if key.contains("/target/debug/")
        || key.contains("/target/release/")
        || key.contains("/target/dist/")
    {
        return Origin::Source;
    }
    if key.contains("/.cargo/bin/") {
        return Origin::Cargo;
    }
    if key.contains("/cellar/")
        || key.contains("/homebrew/")
        || key.contains("/program files/")
        || key.starts_with("/usr/")
        || key.starts_with("/opt/")
        || key.contains(":/program files/")
    {
        return Origin::Package;
    }
    let managed = key.ends_with("/.local/bin/openfelt")
        || key.ends_with("/.local/bin/openfelt.exe")
        || key.ends_with("/.openfelt/bin/openfelt")
        || key.ends_with("/.openfelt/bin/openfelt.exe")
        || key.contains("/openfelt/bin/openfelt");
    if managed {
        Origin::Managed
    } else {
        Origin::Unsupported
    }
}

pub fn current_origin() -> Origin {
    std::env::current_exe()
        .map(|path| classify_install(&path))
        .unwrap_or(Origin::Unsupported)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AvailableUpdate {
    pub version: String,
    pub notes_url: String,
    pub archive_name: String,
    pub archive_url: String,
    pub checksum_url: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckResult {
    UpToDate { version: String },
    Available(AvailableUpdate),
    NoRelease,
}

pub fn select_release(current: &str, triple: &str, body: &[u8]) -> Result<CheckResult, String> {
    if body.len() > MAX_RELEASE_LIST {
        return Err("GitHub release list was larger than expected".into());
    }
    if triple == "unsupported" {
        return Err("This operating system or architecture has no published OpenFelt build".into());
    }
    let current = parse_stable_version(current)
        .ok_or_else(|| "The installed version is not a stable release number".to_string())?;
    let releases: Vec<Release> = match serde_json::from_slice(body) {
        Ok(releases) => releases,
        Err(_) => {
            let message = serde_json::from_slice::<ApiError>(body)
                .ok()
                .map(|e| e.message)
                .unwrap_or_default();
            if message.to_ascii_lowercase().contains("rate limit") {
                return Err("GitHub rate limit. Try again later.".into());
            }
            return Err("Could not read the GitHub release list".into());
        }
    };
    let mut stable = Vec::new();
    for release in releases {
        if release.draft || release.prerelease {
            continue;
        }
        if let Some(version) = parse_stable_version(&release.tag_name) {
            stable.push((version, release));
        }
    }
    if stable.is_empty() {
        return Ok(CheckResult::NoRelease);
    }
    stable.sort_by_key(|release| std::cmp::Reverse(release.0));
    let Some((version, release)) = stable.into_iter().find(|(version, _)| *version > current)
    else {
        return Ok(CheckResult::UpToDate {
            version: format_version(current),
        });
    };
    let archive_name = archive_name(triple);
    let checksum_name = format!("{archive_name}.sha256");
    let archive = release
        .assets
        .iter()
        .find(|a| a.name == archive_name)
        .ok_or_else(|| {
            format!(
                "Release {} is missing {archive_name}",
                format_version(version)
            )
        })?;
    let checksum = release
        .assets
        .iter()
        .find(|a| a.name == checksum_name)
        .ok_or_else(|| {
            format!(
                "Release {} is missing the published checksum for {archive_name}",
                format_version(version)
            )
        })?;
    https_github(&archive.browser_download_url)?;
    https_github(&checksum.browser_download_url)?;
    Ok(CheckResult::Available(AvailableUpdate {
        version: format_version(version),
        notes_url: release.html_url,
        archive_name,
        archive_url: archive.browser_download_url.clone(),
        checksum_url: checksum.browser_download_url.clone(),
    }))
}

pub fn parse_checksum(text: &str, archive_name: &str) -> Result<String, String> {
    let trimmed = text.trim();
    if is_sha256_hex(trimmed) {
        return Ok(trimmed.to_ascii_lowercase());
    }
    let mut found = None;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.split_whitespace();
        let hash = parts
            .next()
            .unwrap_or_default()
            .trim_start_matches("sha256:")
            .to_ascii_lowercase();
        let name = parts.next().unwrap_or_default().trim_start_matches('*');
        if parts.next().is_some() || !is_sha256_hex(&hash) || name != archive_name {
            continue;
        }
        if found.is_some_and(|existing| existing != hash) {
            return Err("Published checksum lists more than one hash for this archive".into());
        }
        found = Some(hash);
    }
    found.ok_or_else(|| "Published checksum does not match this archive".into())
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

pub fn verify_checksum(bytes: &[u8], expected: &str) -> Result<(), String> {
    let actual = sha256_hex(bytes);
    if actual.eq_ignore_ascii_case(expected.trim()) {
        Ok(())
    } else {
        Err("Downloaded archive checksum does not match the published SHA256".into())
    }
}

pub fn extract_binary(archive_name: &str, bytes: &[u8], binary: &str) -> Result<Vec<u8>, String> {
    if !archive_name.ends_with(".tar.gz") {
        return Err("Unexpected release archive type".into());
    }
    if bytes.len() > MAX_ARCHIVE {
        return Err("Update archive is larger than expected".into());
    }
    let decoder = GzDecoder::new(bytes);
    let mut archive = Archive::new(decoder);
    let mut found = None;
    let entries = archive.entries().map_err(|_| "Update archive is invalid")?;
    for entry in entries {
        let mut entry = entry.map_err(|_| "Update archive is invalid")?;
        let path = entry
            .path()
            .map_err(|_| "Update archive is invalid")?
            .into_owned();
        if !entry.header().entry_type().is_file() || !binary_path_ok(&path, binary) {
            if binary_path_ok(&path, binary) {
                return Err("Update archive contains an unexpected file type".into());
            }
            continue;
        }
        if entry.size() > MAX_BINARY as u64 {
            return Err("Update binary is larger than expected".into());
        }
        if found.is_some() {
            return Err("Update archive contains more than one OpenFelt executable".into());
        }
        let mut buf = Vec::new();
        entry
            .read_to_end(&mut buf)
            .map_err(|_| "Update archive is invalid")?;
        if buf.len() > MAX_BINARY || buf.is_empty() {
            return Err("Update binary has an unexpected size".into());
        }
        found = Some(buf);
    }
    found.ok_or_else(|| "Update archive does not contain the OpenFelt executable".into())
}

pub fn install_bytes(dest: &Path, bytes: &[u8]) -> Result<(), String> {
    if bytes.is_empty() || bytes.len() > MAX_BINARY {
        return Err("Update binary has an unexpected size".into());
    }
    let parent = dest
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .ok_or("Cannot locate the installed executable")?;
    let staged = parent.join(format!(".{}.new", file_name(dest)?));
    let write_result = (|| -> Result<(), String> {
        let mut file = fs::File::create(&staged)
            .map_err(|_| "Cannot write the update beside the current executable")?;
        std::io::Write::write_all(&mut file, bytes)
            .map_err(|_| "Cannot write the update beside the current executable")?;
        file.sync_all()
            .map_err(|_| "Cannot write the update beside the current executable")?;
        Ok(())
    })();
    if let Err(e) = write_result {
        let _ = fs::remove_file(&staged);
        return Err(e);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Err(e) = fs::set_permissions(&staged, fs::Permissions::from_mode(0o755))
            .map(|_| ())
            .map_err(|_| "Cannot mark the update executable".to_string())
        {
            let _ = fs::remove_file(&staged);
            return Err(e);
        }
    }
    let result = swap_in_place(dest, &staged);
    if result.is_err() {
        let _ = fs::remove_file(&staged);
    }
    result
}

pub fn host_allowed(host: &str) -> bool {
    let host = host.trim().trim_end_matches('.').to_ascii_lowercase();
    host == "github.com"
        || host.ends_with(".github.com")
        || host == "githubusercontent.com"
        || host.ends_with(".githubusercontent.com")
}

#[derive(Debug)]
pub enum UpdateMessage {
    Progress(String),
    Done(Result<UpdateDone, String>),
}

#[derive(Debug)]
pub enum UpdateDone {
    Checked(CheckResult),
    Installed(String),
}

pub struct UpdateTask {
    receiver: mpsc::Receiver<UpdateMessage>,
    cancel: Option<tokio::sync::oneshot::Sender<()>>,
}

impl UpdateTask {
    pub fn drain(&self) -> Vec<UpdateMessage> {
        let mut out = Vec::new();
        while let Ok(msg) = self.receiver.try_recv() {
            out.push(msg);
        }
        out
    }
}

impl Drop for UpdateTask {
    fn drop(&mut self) {
        if let Some(cancel) = self.cancel.take() {
            let _ = cancel.send(());
        }
    }
}

pub fn start_check() -> Result<UpdateTask, String> {
    let (sender, receiver) = mpsc::channel();
    let (cancel_tx, cancel_rx) = tokio::sync::oneshot::channel();
    std::thread::spawn(move || {
        let result = run_check(cancel_rx);
        let _ = sender.send(UpdateMessage::Done(result.map(UpdateDone::Checked)));
    });
    Ok(UpdateTask {
        receiver,
        cancel: Some(cancel_tx),
    })
}

pub fn start_install(update: &AvailableUpdate) -> Result<UpdateTask, String> {
    let origin = current_origin();
    if !origin.allows_replacement() {
        return Err(origin.guidance().into());
    }
    let update = update.clone();
    let (sender, receiver) = mpsc::channel();
    let (cancel_tx, cancel_rx) = tokio::sync::oneshot::channel();
    std::thread::spawn(move || {
        let _ = sender.send(UpdateMessage::Progress(
            "Downloading update… Esc cancels before replacement.".into(),
        ));
        let result = run_install(update, cancel_rx, &sender);
        let _ = sender.send(UpdateMessage::Done(result.map(UpdateDone::Installed)));
    });
    Ok(UpdateTask {
        receiver,
        cancel: Some(cancel_tx),
    })
}

fn run_check(cancel: tokio::sync::oneshot::Receiver<()>) -> Result<CheckResult, String> {
    let triple = target_triple();
    let current = current_version();
    block_on(cancel, async {
        let client = github_client()?;
        let bytes = get_limited(&client, RELEASES_URL, MAX_RELEASE_LIST).await?;
        select_release(current, triple, &bytes)
    })
}

fn run_install(
    update: AvailableUpdate,
    mut cancel: tokio::sync::oneshot::Receiver<()>,
    progress: &mpsc::Sender<UpdateMessage>,
) -> Result<String, String> {
    let dest = std::env::current_exe().map_err(|_| "Cannot locate the running executable")?;
    if !classify_install(&dest).allows_replacement() {
        return Err(current_origin().guidance().into());
    }
    let binary = block_on_with(async {
        let client = github_client()?;
        let archive = tokio::select! {
            _ = &mut cancel => return Err("Update cancelled".into()),
            result = get_limited(&client, &update.archive_url, MAX_ARCHIVE) => result?,
        };
        let checksum = tokio::select! {
            _ = &mut cancel => return Err("Update cancelled".into()),
            result = get_limited(&client, &update.checksum_url, 16_384) => result?,
        };
        let expected = parse_checksum(&String::from_utf8_lossy(&checksum), &update.archive_name)?;
        let _ = progress.send(UpdateMessage::Progress(
            "Verifying published checksum…".into(),
        ));
        verify_checksum(&archive, &expected)?;
        if cancel.try_recv().is_ok() {
            return Err("Update cancelled".into());
        }
        extract_binary(&update.archive_name, &archive, binary_name(target_triple()))
    })?;
    install_bytes(&dest, &binary)?;
    Ok(update.version)
}

fn block_on<T>(
    cancel: tokio::sync::oneshot::Receiver<()>,
    work: impl std::future::Future<Output = Result<T, String>>,
) -> Result<T, String> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| "Cannot start the update check")?;
    runtime.block_on(async {
        tokio::select! {
            _ = cancel => Err("Update cancelled".into()),
            result = work => result,
        }
    })
}

fn block_on_with<T>(
    work: impl std::future::Future<Output = Result<T, String>>,
) -> Result<T, String> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| "Cannot start the update")?;
    runtime.block_on(work)
}

fn github_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            let host = attempt.url().host_str().unwrap_or("");
            if attempt.previous().len() < 3 && host_allowed(host) {
                attempt.follow()
            } else {
                attempt.stop()
            }
        }))
        .timeout(Duration::from_secs(30))
        .user_agent(format!("openfelt/{}", current_version()))
        .build()
        .map_err(|_| "Cannot start the update connection".into())
}

async fn get_limited(client: &reqwest::Client, url: &str, limit: usize) -> Result<Vec<u8>, String> {
    https_github(url)?;
    let response = client
        .get(url)
        .header(
            "accept",
            if url.starts_with("https://api.github.com/") {
                "application/vnd.github+json"
            } else {
                "application/octet-stream"
            },
        )
        .send()
        .await
        .map_err(|_| "Could not reach GitHub. Check your connection and try again.".to_string())?;
    if !response.status().is_success() {
        return Err(http_error(response.status().as_u16()));
    }
    if response.content_length().is_some_and(|n| n > limit as u64) {
        return Err("Update download is larger than expected".into());
    }
    let bytes = response
        .bytes()
        .await
        .map_err(|_| "Update download was interrupted".to_string())?;
    if bytes.len() > limit {
        return Err("Update download is larger than expected".into());
    }
    Ok(bytes.to_vec())
}

fn http_error(status: u16) -> String {
    match status {
        403 | 429 => "GitHub rate limit or access denied. Try again later.".into(),
        404 => "No published GitHub release was found.".into(),
        _ => format!("GitHub returned HTTP {status}."),
    }
}

fn https_github(url: &str) -> Result<(), String> {
    let parsed = reqwest::Url::parse(url).map_err(|_| "Release URL is invalid")?;
    if parsed.scheme() == "https" && host_allowed(parsed.host_str().unwrap_or("")) {
        Ok(())
    } else {
        Err("Release URL is not a GitHub HTTPS address".into())
    }
}

fn swap_in_place(current: &Path, staged: &Path) -> Result<(), String> {
    let backup = backup_path(current)?;
    if backup.exists() {
        fs::remove_file(&backup).map_err(|_| "Cannot clear the previous update backup")?;
    }
    fs::rename(current, &backup).map_err(|_| "Cannot stage the current executable")?;
    if fs::rename(staged, current).is_err() {
        let restored = fs::rename(&backup, current).is_ok();
        return Err(if restored {
            "Update replacement failed; the existing executable was restored".into()
        } else {
            "Update replacement failed and the previous executable could not be restored".into()
        });
    }
    let _ = fs::remove_file(&backup);
    Ok(())
}

fn backup_path(current: &Path) -> Result<PathBuf, String> {
    let parent = current
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .ok_or("Cannot locate the installed executable")?;
    Ok(parent.join(format!(".{}.previous", file_name(current)?)))
}

fn file_name(path: &Path) -> Result<&str, String> {
    path.file_name()
        .and_then(|n| n.to_str())
        .filter(|n| !n.is_empty())
        .ok_or_else(|| "Cannot name the installed executable".into())
}

fn binary_path_ok(path: &Path, binary: &str) -> bool {
    let parts: Vec<_> = path
        .components()
        .filter(|c| !matches!(c, Component::CurDir))
        .collect();
    if parts.iter().any(|c| matches!(c, Component::ParentDir)) {
        return false;
    }
    match parts.as_slice() {
        [Component::Normal(name)] => name.to_str() == Some(binary),
        [Component::Normal(_dir), Component::Normal(name)] => name.to_str() == Some(binary),
        _ => false,
    }
}

fn parse_stable_version(tag: &str) -> Option<(u64, u64, u64)> {
    let tag = tag.trim().strip_prefix("openfelt-").unwrap_or(tag.trim());
    let tag = tag.strip_prefix('v').unwrap_or(tag);
    if tag.contains('-') || tag.contains('+') {
        return None;
    }
    let mut parts = tag.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some((major, minor, patch))
}

fn format_version((major, minor, patch): (u64, u64, u64)) -> String {
    format!("{major}.{minor}.{patch}")
}

fn is_sha256_hex(text: &str) -> bool {
    text.len() == 64 && text.chars().all(|c| c.is_ascii_hexdigit())
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    html_url: String,
    #[serde(default)]
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
}

#[derive(Deserialize)]
struct ApiError {
    message: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release(tag: &str, prerelease: bool, assets: &[(&str, &str)]) -> String {
        let assets = assets
            .iter()
            .map(|(name, url)| format!(r#"{{"name":"{name}","browser_download_url":"{url}"}}"#))
            .collect::<Vec<_>>()
            .join(",");
        format!(
            r#"{{"tag_name":"{tag}","draft":false,"prerelease":{prerelease},"html_url":"https://github.com/jdavies69/openfelt/releases/tag/{tag}","assets":[{assets}]}}"#
        )
    }

    fn asset_pair(triple: &str) -> (String, String, String, String) {
        let name = archive_name(triple);
        let checksum = format!("{name}.sha256");
        (
            name.clone(),
            format!("https://github.com/jdavies69/openfelt/releases/download/v9.0.0/{name}"),
            checksum.clone(),
            format!("https://github.com/jdavies69/openfelt/releases/download/v9.0.0/{checksum}"),
        )
    }

    #[test]
    fn stable_newer_release_is_selected_without_prereleases_or_downgrades() {
        let triple = "aarch64-apple-darwin";
        let (archive, archive_url, checksum, checksum_url) = asset_pair(triple);
        let body = format!(
            "[{},{},{},{}]",
            release(
                "v1.2.0-rc.1",
                true,
                &[(&archive, &archive_url), (&checksum, &checksum_url)]
            ),
            release(
                "v0.9.0",
                false,
                &[(&archive, &archive_url), (&checksum, &checksum_url)]
            ),
            release(
                "v1.1.0",
                false,
                &[(&archive, &archive_url), (&checksum, &checksum_url)]
            ),
            release(
                "v1.0.1",
                false,
                &[(&archive, &archive_url), (&checksum, &checksum_url)]
            ),
        );
        match select_release("1.0.1", triple, body.as_bytes()).unwrap() {
            CheckResult::Available(update) => {
                assert_eq!(update.version, "1.1.0");
                assert_eq!(update.archive_name, archive);
                assert!(update.notes_url.contains("v1.1.0"));
            }
            other => panic!("expected update, got {other:?}"),
        }
        assert!(matches!(
            select_release("1.1.0", triple, body.as_bytes()).unwrap(),
            CheckResult::UpToDate { version } if version == "1.1.0"
        ));
        assert!(matches!(
            select_release("2.0.0", triple, body.as_bytes()).unwrap(),
            CheckResult::UpToDate { .. }
        ));
    }

    #[test]
    fn missing_release_checksum_host_and_network_errors_are_recoverable() {
        let triple = "x86_64-unknown-linux-gnu";
        let name = archive_name(triple);
        assert!(matches!(
            select_release("1.0.1", triple, b"[]").unwrap(),
            CheckResult::NoRelease
        ));
        assert!(matches!(
            select_release(
                "1.0.1",
                triple,
                br#"[{"tag_name":"v9.0.0-beta","prerelease":true}]"#
            )
            .unwrap(),
            CheckResult::NoRelease
        ));
        let missing = release(
            "v9.0.0",
            false,
            &[(
                &name,
                "https://github.com/jdavies69/openfelt/releases/download/v9.0.0/x.tar.gz",
            )],
        );
        assert!(
            select_release("1.0.0", triple, format!("[{missing}]").as_bytes())
                .unwrap_err()
                .contains("missing the published checksum")
        );
        let evil = release(
            "v9.0.0",
            false,
            &[
                (&name, "https://example.invalid/openfelt.tar.gz"),
                (
                    &format!("{name}.sha256"),
                    "https://github.com/jdavies69/openfelt/releases/download/v9.0.0/x.sha256",
                ),
            ],
        );
        assert!(
            select_release("1.0.0", triple, format!("[{evil}]").as_bytes())
                .unwrap_err()
                .contains("GitHub HTTPS")
        );
        assert!(
            select_release("1.0.0", triple, br#"{"message":"API rate limit exceeded"}"#)
                .unwrap_err()
                .contains("rate limit")
        );
        assert_eq!(http_error(404), "No published GitHub release was found.");
        assert!(!host_allowed("example.invalid"));
        assert!(host_allowed("release-assets.githubusercontent.com"));
    }

    #[test]
    fn checksum_and_archive_must_match_before_any_replacement() {
        let name = "openfelt-aarch64-apple-darwin.tar.gz";
        let bytes = sample_archive(b"new-binary");
        let good = sha256_hex(&bytes);
        assert_eq!(
            parse_checksum(&format!("{good}  {name}\n"), name).unwrap(),
            good
        );
        assert_eq!(parse_checksum(&good, name).unwrap(), good);
        assert!(parse_checksum("abc", name).is_err());
        assert!(parse_checksum(&format!("{good}  other.tar.gz\n"), name).is_err());
        verify_checksum(&bytes, &good).unwrap();
        assert!(verify_checksum(&bytes, &"ab".repeat(32)).is_err());
        let dir = tempfile_dir("checksum-failure");
        let dest = dir.join("openfelt");
        fs::write(&dest, b"old-binary").unwrap();
        assert!(verify_checksum(&bytes, &"cd".repeat(32)).is_err());
        assert_eq!(fs::read(&dest).unwrap(), b"old-binary");
        assert_eq!(
            extract_binary(name, &bytes, "openfelt").unwrap(),
            b"new-binary"
        );
        let nested = sample_nested_archive();
        assert_eq!(
            extract_binary(name, &nested, "openfelt").unwrap(),
            b"nested"
        );
        assert!(extract_binary(name, &sample_escape_archive(), "openfelt").is_err());
        assert!(extract_binary(name, &sample_duplicate_archive(), "openfelt").is_err());
    }

    #[test]
    fn failed_replacement_preserves_the_existing_executable() {
        let dir = tempfile_dir("swap");
        let dest = dir.join("openfelt");
        fs::write(&dest, b"old-binary").unwrap();
        let missing = dir.join("missing");
        assert!(swap_in_place(&dest, &missing)
            .unwrap_err()
            .contains("restored"));
        assert_eq!(fs::read(&dest).unwrap(), b"old-binary");
        install_bytes(&dest, b"new-binary").unwrap();
        assert_eq!(fs::read(&dest).unwrap(), b"new-binary");
        assert!(!dir.join(".openfelt.previous").exists());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&dest).unwrap().permissions().mode();
            assert_eq!(mode & 0o111, 0o111);
        }
    }

    #[test]
    fn install_origin_refuses_source_cargo_and_package_paths() {
        assert_eq!(
            classify_install(Path::new("/Users/a/.local/bin/openfelt")),
            Origin::Managed
        );
        assert_eq!(
            classify_install(Path::new("/Users/a/.openfelt/bin/openfelt.exe")),
            Origin::Managed
        );
        assert_eq!(
            classify_install(Path::new("/Users/a/.cargo/bin/openfelt")),
            Origin::Cargo
        );
        assert_eq!(
            classify_install(Path::new("/src/openfelt/target/release/openfelt")),
            Origin::Source
        );
        assert_eq!(
            classify_install(Path::new("/opt/homebrew/bin/openfelt")),
            Origin::Package
        );
        assert_eq!(
            classify_install(Path::new("/usr/local/bin/openfelt")),
            Origin::Package
        );
        assert!(!Origin::Cargo.allows_replacement());
        assert!(Origin::Cargo.guidance().contains("cargo install"));
        assert!(Origin::Source.guidance().contains("source build"));
    }

    fn sample_archive(binary: &[u8]) -> Vec<u8> {
        archive_with(&[("openfelt", binary)])
    }

    fn sample_nested_archive() -> Vec<u8> {
        archive_with(&[("dist/openfelt", b"nested")])
    }

    fn sample_escape_archive() -> Vec<u8> {
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(tar::EntryType::Regular);
        header.set_size(3);
        header.set_mode(0o644);
        header.as_old_mut().name[..11].copy_from_slice(b"../openfelt");
        header.set_cksum();
        let mut raw = header.as_bytes().to_vec();
        raw.extend_from_slice(b"bad");
        raw.resize(512 + 512, 0);
        raw.extend_from_slice(&[0; 1024]);
        let mut gz = Vec::new();
        {
            let mut encoder = flate2::write::GzEncoder::new(&mut gz, flate2::Compression::fast());
            std::io::Write::write_all(&mut encoder, &raw).unwrap();
            encoder.finish().unwrap();
        }
        gz
    }

    fn sample_duplicate_archive() -> Vec<u8> {
        archive_with(&[("openfelt", b"one"), ("dir/openfelt", b"two")])
    }

    fn archive_with(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut raw = Vec::new();
        {
            let encoder = flate2::write::GzEncoder::new(&mut raw, flate2::Compression::fast());
            let mut builder = tar::Builder::new(encoder);
            for (name, bytes) in files {
                let mut header = tar::Header::new_gnu();
                header.set_size(bytes.len() as u64);
                header.set_mode(0o755);
                header.set_cksum();
                builder.append_data(&mut header, *name, *bytes).unwrap();
            }
            builder.finish().unwrap();
        }
        raw
    }

    fn tempfile_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "openfelt-update-{label}-{}-{:016x}",
            std::process::id(),
            rand::random::<u64>()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }
}
