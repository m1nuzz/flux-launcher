use std::fs::File;
use std::io::{self, Read, Write};
use std::path::PathBuf;
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
    let response = ureq::AgentBuilder::new()
        .timeout(CHECK_TIMEOUT)
        .build()
        .get(GITHUB_LATEST_RELEASE_URL)
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

pub fn launch_installer(update: &StableUpdate) -> Result<PathBuf, String> {
    let installer_path =
        std::env::temp_dir().join(format!("FluxLauncher-update-{}.exe", update.version));
    let response = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(60))
        .build()
        .get(&update.installer_url)
        .set("User-Agent", USER_AGENT)
        .call()
        .map_err(|error| format!("Update download failed: {error}"))?;
    let mut reader = response.into_reader();
    let mut file = File::create(&installer_path)
        .map_err(|error| format!("Could not create update file: {error}"))?;
    let mut hasher = Sha256::new();
    copy_and_hash(&mut reader, &mut file, &mut hasher)
        .map_err(|error| format!("Could not save update file: {error}"))?;
    file.flush()
        .map_err(|error| format!("Could not flush update file: {error}"))?;

    if let Some(expected) = &update.installer_sha256 {
        let actual = format_digest(&hasher.finalize());
        if !actual.eq_ignore_ascii_case(expected) {
            let _ = std::fs::remove_file(&installer_path);
            return Err(format!(
                "Update checksum mismatch: expected {expected}, received {actual}"
            ));
        }
    }

    Command::new(&installer_path)
        .args([
            "/SILENT",
            "/SUPPRESSMSGBOXES",
            "/NORESTART",
            "/CLOSEAPPLICATIONS",
            "/RESTARTAPPLICATIONS",
        ])
        .spawn()
        .map_err(|error| format!("Could not start update installer: {error}"))?;
    Ok(installer_path)
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

fn copy_and_hash<R: Read, W: Write>(
    reader: &mut R,
    writer: &mut W,
    hasher: &mut Sha256,
) -> io::Result<u64> {
    let mut buffer = [0_u8; 64 * 1024];
    let mut total = 0_u64;
    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        writer.write_all(&buffer[..count])?;
        hasher.update(&buffer[..count]);
        total = total.saturating_add(count as u64);
    }
    Ok(total)
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
}
