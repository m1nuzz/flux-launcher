use std::fs::File;
use std::io::{self, Read, Write};
use std::path::Path;
use std::process::Command;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use semver::Version;
use serde::Deserialize;
use sha2::{Digest, Sha256};

const GITHUB_LATEST_RELEASE_URL: &str =
    "https://api.github.com/repos/m1nuzz/flux-launcher/releases/latest";
const INSTALLER_ASSET_NAME: &str = "FluxLauncher-Setup.exe";
const USER_AGENT: &str = "FluxLauncher-Updater";
const CHECK_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StableUpdate {
    pub version: Version,
    pub tag_name: String,
    pub release_url: String,
    pub installer_url: String,
    pub installer_sha256: Option<String>,
}

pub struct UpdateCheckResponse {
    pub checked_at: u64,
    pub result: Result<Option<StableUpdate>, String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RelaunchMode {
    Hidden,
    Visible,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DownloadProgress {
    pub received_bytes: u64,
    pub total_bytes: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct GitHubRelease {
    tag_name: String,
    html_url: String,
    draft: bool,
    prerelease: bool,
    assets: Vec<GitHubAsset>,
}

#[derive(Debug, Deserialize)]
struct GitHubAsset {
    name: String,
    browser_download_url: String,
    digest: Option<String>,
}

pub fn stable_update(current_version: &str, payload: &str) -> Result<Option<StableUpdate>, String> {
    let release: GitHubRelease = serde_json::from_str(payload)
        .map_err(|error| format!("GitHub release response is invalid: {error}"))?;
    if release.draft || release.prerelease {
        return Ok(None);
    }

    let version = parse_stable_version(&release.tag_name)?;
    let current = Version::parse(current_version)
        .map_err(|error| format!("Current Flux version is invalid: {error}"))?;
    if version <= current {
        return Ok(None);
    }

    let installer = release
        .assets
        .into_iter()
        .find(|asset| asset.name.eq_ignore_ascii_case(INSTALLER_ASSET_NAME))
        .ok_or_else(|| {
            format!(
                "Stable release {} has no {INSTALLER_ASSET_NAME} asset",
                release.tag_name
            )
        })?;

    Ok(Some(StableUpdate {
        version,
        tag_name: release.tag_name,
        release_url: release.html_url,
        installer_url: installer.browser_download_url,
        installer_sha256: normalize_digest(installer.digest),
    }))
}

pub fn check_stable(current_version: &str) -> Result<Option<StableUpdate>, String> {
    let release_url = std::env::var("FLUX_UPDATE_API_URL")
        .unwrap_or_else(|_| GITHUB_LATEST_RELEASE_URL.to_owned());
    let response = ureq::AgentBuilder::new()
        .timeout(CHECK_TIMEOUT)
        .build()
        .get(&release_url)
        .set("Accept", "application/vnd.github+json")
        .set("User-Agent", USER_AGENT)
        .call()
        .map_err(|error| format!("GitHub release check failed: {error}"))?;
    let payload = response
        .into_string()
        .map_err(|error| format!("GitHub release response could not be read: {error}"))?;
    stable_update(current_version, &payload)
}

pub fn should_check(now: u64, last_check: u64, interval_hours: u32) -> bool {
    if last_check == 0 || now < last_check {
        return true;
    }
    now.saturating_sub(last_check) >= u64::from(interval_hours).saturating_mul(60 * 60)
}

pub fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub fn download_installer_to_path<F>(
    update: &StableUpdate,
    installer_path: &Path,
    mut on_progress: F,
) -> Result<u64, String>
where
    F: FnMut(DownloadProgress),
{
    let result = (|| {
        let response = ureq::AgentBuilder::new()
            .timeout(Duration::from_secs(60))
            .build()
            .get(&update.installer_url)
            .set("User-Agent", USER_AGENT)
            .call()
            .map_err(|error| format!("Update download failed: {error}"))?;
        let total_bytes = response
            .header("Content-Length")
            .and_then(|value| value.parse::<u64>().ok());
        on_progress(DownloadProgress {
            received_bytes: 0,
            total_bytes,
        });

        let mut reader = response.into_reader();
        let mut file = File::create(installer_path)
            .map_err(|error| format!("Could not create update file: {error}"))?;
        let mut hasher = Sha256::new();
        let received_bytes = copy_and_hash_with_progress(
            &mut reader,
            &mut file,
            &mut hasher,
            total_bytes,
            &mut on_progress,
        )
        .map_err(|error| format!("Could not save update file: {error}"))?;
        close_download_file(file)
            .map_err(|error| format!("Could not finalize update file: {error}"))?;

        if let Some(expected) = &update.installer_sha256 {
            let actual = format_digest(&hasher.finalize());
            if !actual.eq_ignore_ascii_case(expected) {
                return Err(format!(
                    "Update checksum mismatch: expected {expected}, received {actual}"
                ));
            }
        }
        Ok(received_bytes)
    })();

    if result.is_err() {
        let _ = std::fs::remove_file(installer_path);
    }
    result
}

pub fn handoff_installer(installer_path: &Path, relaunch_mode: RelaunchMode) -> Result<(), String> {
    spawn_installer_handoff(installer_path, relaunch_mode)
        .map_err(|error| format!("Could not start update installer: {error}"))
}

fn close_download_file(mut file: File) -> io::Result<()> {
    file.flush()?;
    file.sync_all()?;
    drop(file);
    Ok(())
}

#[cfg(windows)]
fn spawn_installer_handoff(installer_path: &Path, relaunch_mode: RelaunchMode) -> io::Result<()> {
    use std::os::windows::process::CommandExt;

    let current_exe = std::env::current_exe()?;
    let parent_pid = std::process::id();
    let installer = powershell_literal(installer_path);
    let application = powershell_literal(&current_exe);
    let relaunch = relaunch_command(relaunch_mode);
    let script = format!(
        "$parent = Get-Process -Id {parent_pid} -ErrorAction SilentlyContinue; \
         if ($parent) {{ $parent.WaitForExit() }}; \
         $relaunchApplication = {application}; \
         $arguments = @('/VERYSILENT','/SUPPRESSMSGBOXES','/NORESTART','/NOCANCEL','/CLOSEAPPLICATIONS'); \
         if ($env:FLUX_UPDATE_INSTALL_DIR) {{ \
             $arguments += '/DIR=' + $env:FLUX_UPDATE_INSTALL_DIR; \
             $relaunchApplication = Join-Path $env:FLUX_UPDATE_INSTALL_DIR (Split-Path -Leaf $relaunchApplication) \
         }}; \
         $setup = Start-Process -FilePath {installer} -ArgumentList $arguments -Wait -PassThru; \
         if ($setup.ExitCode -eq 0) {{ {relaunch} }}; \
         Remove-Item -LiteralPath {installer} -Force -ErrorAction SilentlyContinue",
    );

    Command::new("powershell.exe")
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-WindowStyle",
            "Hidden",
            "-Command",
            &script,
        ])
        .creation_flags(0x0800_0000)
        .spawn()
        .map(|_| ())
}

#[cfg(not(windows))]
fn spawn_installer_handoff(installer_path: &Path, _relaunch_mode: RelaunchMode) -> io::Result<()> {
    Command::new(installer_path)
        .args([
            "/VERYSILENT",
            "/SUPPRESSMSGBOXES",
            "/NORESTART",
            "/NOCANCEL",
            "/CLOSEAPPLICATIONS",
            "/RESTARTAPPLICATIONS",
        ])
        .spawn()
        .map(|_| ())
}

#[cfg(windows)]
fn powershell_literal(path: &Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', "''"))
}

fn relaunch_command(relaunch_mode: RelaunchMode) -> &'static str {
    match relaunch_mode {
        RelaunchMode::Hidden => {
            "Start-Process -FilePath $relaunchApplication -ArgumentList @('--startup')"
        }
        RelaunchMode::Visible => "Start-Process -FilePath $relaunchApplication",
    }
}

fn parse_stable_version(tag_name: &str) -> Result<Version, String> {
    let version = Version::parse(tag_name.trim_start_matches('v'))
        .map_err(|error| format!("Release tag {tag_name} is not semantic version: {error}"))?;
    if !version.pre.is_empty() {
        return Err(format!("Release tag {tag_name} is a prerelease"));
    }
    Ok(version)
}

fn normalize_digest(digest: Option<String>) -> Option<String> {
    digest.and_then(|value| {
        let value = value.trim();
        value
            .strip_prefix("sha256:")
            .or_else(|| value.strip_prefix("SHA256:"))
            .map(str::to_owned)
    })
}

fn format_digest(digest: &[u8]) -> String {
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn copy_and_hash_with_progress<R: Read, W: Write, F: FnMut(DownloadProgress)>(
    reader: &mut R,
    writer: &mut W,
    hasher: &mut Sha256,
    total_bytes: Option<u64>,
    on_progress: &mut F,
) -> io::Result<u64> {
    let mut buffer = [0_u8; 64 * 1024];
    let mut received_bytes = 0_u64;
    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        writer.write_all(&buffer[..count])?;
        hasher.update(&buffer[..count]);
        received_bytes = received_bytes.saturating_add(count as u64);
        on_progress(DownloadProgress {
            received_bytes,
            total_bytes,
        });
    }
    Ok(received_bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release_payload(tag_name: &str, prerelease: bool) -> String {
        format!(
            r#"{{"tag_name":"{tag_name}","html_url":"https://github.com/m1nuzz/flux-launcher/releases/tag/{tag_name}","draft":false,"prerelease":{prerelease},"assets":[{{"name":"FluxLauncher-Setup.exe","browser_download_url":"https://example.test/FluxLauncher-Setup.exe","digest":"sha256:abc"}}]}}"#
        )
    }

    #[test]
    fn relaunch_command_keeps_automatic_updates_hidden() {
        let hidden = relaunch_command(RelaunchMode::Hidden);
        let visible = relaunch_command(RelaunchMode::Visible);
        assert!(hidden.contains("--startup"));
        assert!(!visible.contains("--startup"));
        assert!(hidden.contains("Start-Process"));
        assert!(hidden.contains("$relaunchApplication"));
        assert!(visible.contains("Start-Process"));
        assert!(visible.contains("$relaunchApplication"));
    }

    #[test]
    fn finalized_download_releases_file_for_rename_and_delete() {
        let source = std::env::temp_dir().join(format!(
            "flux-updater-test-{}-source.exe",
            std::process::id()
        ));
        let renamed = std::env::temp_dir().join(format!(
            "flux-updater-test-{}-renamed.exe",
            std::process::id()
        ));
        let file = File::create(&source).unwrap();
        close_download_file(file).unwrap();
        std::fs::rename(&source, &renamed).unwrap();
        std::fs::remove_file(&renamed).unwrap();
    }

    #[test]
    fn latest_stable_release_is_detected_and_beta_is_rejected() {
        let stable = stable_update("0.1.51", &release_payload("v0.1.52", false))
            .unwrap()
            .unwrap();
        assert_eq!(stable.version, Version::new(0, 1, 52));
        assert_eq!(stable.installer_sha256.as_deref(), Some("abc"));

        let beta = stable_update("0.1.51", &release_payload("v0.1.52-beta.1", true));
        assert_eq!(beta.unwrap(), None);
    }

    #[test]
    fn older_and_same_versions_are_not_updates() {
        assert_eq!(
            stable_update("0.1.52", &release_payload("v0.1.52", false)).unwrap(),
            None
        );
        assert_eq!(
            stable_update("0.1.52", &release_payload("v0.1.51", false)).unwrap(),
            None
        );
    }

    #[test]
    fn check_is_due_when_never_run_or_interval_elapsed() {
        assert!(should_check(100, 0, 24));
        assert!(!should_check(100, 100, 24));
        assert!(!should_check(100 + 23 * 60 * 60, 100, 24));
        assert!(should_check(100 + 24 * 60 * 60, 100, 24));
        assert!(should_check(50, 100, 24));
    }

    fn serve_payload(payload: Vec<u8>) -> (String, std::thread::JoinHandle<()>) {
        use std::io::{Read as _, Write as _};
        use std::net::TcpListener;
        use std::time::Duration;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0_u8; 2048];
            let _ = stream.read(&mut request);
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                payload.len()
            )
            .unwrap();
            for chunk in payload.chunks(7) {
                stream.write_all(chunk).unwrap();
                stream.flush().unwrap();
                std::thread::sleep(Duration::from_millis(2));
            }
        });
        (format!("http://{address}/FluxLauncher-Setup.exe"), handle)
    }

    fn test_update(installer_url: String, checksum: String) -> StableUpdate {
        StableUpdate {
            version: Version::new(0, 1, 64),
            tag_name: String::from("v0.1.64"),
            release_url: String::from("https://example.test/releases/v0.1.64"),
            installer_url,
            installer_sha256: Some(checksum),
        }
    }

    #[test]
    fn download_reports_monotonic_progress_and_verifies_checksum() {
        use sha2::{Digest as _, Sha256};
        use std::sync::{Arc, Mutex};

        let payload = b"Flux Launcher update payload with several chunks".repeat(8);
        let expected = format_digest(&Sha256::digest(&payload));
        let (url, server) = serve_payload(payload.clone());
        let path = std::env::temp_dir().join(format!(
            "flux-updater-progress-{}-{}.exe",
            std::process::id(),
            payload.len()
        ));
        let progress = Arc::new(Mutex::new(Vec::new()));
        let progress_for_callback = Arc::clone(&progress);

        let received =
            download_installer_to_path(&test_update(url, expected), &path, move |event| {
                progress_for_callback.lock().unwrap().push(event)
            })
            .unwrap();
        server.join().unwrap();

        assert_eq!(received, payload.len() as u64);
        assert_eq!(std::fs::read(&path).unwrap(), payload);
        let events = progress.lock().unwrap();
        assert!(
            events.len() >= 2,
            "download must report more than one progress event"
        );
        assert!(events.windows(2).all(|pair| {
            pair[0].received_bytes <= pair[1].received_bytes
                && pair[1].total_bytes == Some(payload.len() as u64)
        }));
        assert_eq!(events.last().unwrap().received_bytes, payload.len() as u64);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn checksum_failure_removes_partial_update_file() {
        let payload = b"bad checksum payload".repeat(16);
        let (url, server) = serve_payload(payload);
        let path = std::env::temp_dir().join(format!(
            "flux-updater-checksum-{}-{}.exe",
            std::process::id(),
            unix_now()
        ));

        let result = download_installer_to_path(
            &test_update(
                url,
                String::from("0000000000000000000000000000000000000000000000000000000000000000"),
            ),
            &path,
            |_| {},
        );
        server.join().unwrap();

        assert!(result.is_err());
        assert!(
            !path.exists(),
            "checksum failure must remove the staged installer"
        );
    }
}
