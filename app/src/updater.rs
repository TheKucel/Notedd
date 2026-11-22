use reqwest::blocking::Client;
use semver::Version;
use serde::Deserialize;
use std::io::Read;
use std::time::Duration;

pub const CURRENT: &str = env!("CARGO_PKG_VERSION");
const API: &str = "https://api.github.com/repos/TheKucel/Notedd/releases/latest";
const RELEASES: &str = "https://github.com/TheKucel/Notedd/releases";
#[cfg(target_os = "macos")]
const ASSET: Option<&str> = Some("notedd-setup-macos.zip");
#[cfg(all(windows, target_arch = "x86_64"))]
const ASSET: Option<&str> = Some("notedd-setup.exe");
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
const ASSET: Option<&str> = Some("notedd-linux-x86_64.tar.gz");
#[cfg(not(any(
    target_os = "macos",
    all(any(windows, target_os = "linux"), target_arch = "x86_64")
)))]
const ASSET: Option<&str> = None;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
const MAX_ASSET_SIZE: u64 = 256 * 1024 * 1024;

#[derive(Default)]
pub enum State {
    #[default]
    Idle,
    Checking,
    Available(Update),
    #[cfg(any(target_os = "macos", windows))]
    Downloading,
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    draft: bool,
    prerelease: bool,
    assets: Vec<Asset>,
}

#[derive(Clone, Deserialize)]
struct Asset {
    name: String,
    size: u64,
    digest: Option<String>,
}

#[derive(Clone)]
pub struct Update {
    pub version: Version,
    tag: String,
    asset: Option<Asset>,
}

impl Update {
    pub fn release_url(&self) -> String {
        format!("{RELEASES}/tag/{}", self.tag)
    }

    fn asset_url(&self, asset: &Asset) -> String {
        format!("{RELEASES}/download/{}/{}", self.tag, asset.name)
    }

    pub fn download_url(&self) -> Option<String> {
        self.asset.as_ref().map(|asset| self.asset_url(asset))
    }

    pub fn can_install(&self) -> bool {
        cfg!(any(target_os = "macos", windows)) && self.verified_asset().is_some()
    }

    fn verified_asset(&self) -> Option<(&Asset, &str)> {
        let asset = self.asset.as_ref()?;
        let digest = asset.digest.as_deref()?.strip_prefix("sha256:")?;
        ((1..=MAX_ASSET_SIZE).contains(&asset.size)
            && digest.len() == 64
            && digest.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .then_some((asset, digest))
    }
}

fn client(timeout: Duration) -> Result<Client> {
    Ok(Client::builder()
        .https_only(true)
        .user_agent(concat!("Notedd/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(10))
        .timeout(timeout)
        .build()?)
}

pub fn check() -> Result<Option<Update>> {
    let response = client(Duration::from_secs(20))?
        .get(API)
        .header("Accept", "application/vnd.github+json")
        .send()?
        .error_for_status()?;
    let mut json = Vec::new();
    let limit = 1024 * 1024;
    response.take(limit + 1).read_to_end(&mut json)?;
    if json.len() as u64 > limit {
        return Err("Release metadata is too large".into());
    }
    let release = serde_json::from_slice(&json)?;
    new_update(release, &Version::parse(CURRENT)?, ASSET)
}

fn new_update(
    release: Release,
    current: &Version,
    asset_name: Option<&str>,
) -> Result<Option<Update>> {
    if release.draft || release.prerelease {
        return Ok(None);
    }
    let tag = release.tag_name;
    let version = Version::parse(tag.strip_prefix('v').unwrap_or(&tag))?;
    if !version.pre.is_empty() || !version.cmp_precedence(current).is_gt() {
        return Ok(None);
    }
    let asset = release
        .assets
        .into_iter()
        .find(|asset| Some(asset.name.as_str()) == asset_name);
    Ok(Some(Update {
        tag,
        version,
        asset,
    }))
}

#[cfg(any(target_os = "macos", windows))]
pub fn download_installer(update: &Update) -> Result<(tempfile::TempDir, std::path::PathBuf)> {
    let (asset, digest) = update
        .verified_asset()
        .ok_or("Missing installer checksum")?;
    let dir = tempfile::Builder::new()
        .prefix("notedd-update-")
        .tempdir()?;
    let path = dir.path().join(&asset.name);
    let response = client(Duration::from_secs(180))?
        .get(update.asset_url(asset))
        .send()?
        .error_for_status()?;

    write_verified(response, &path, asset.size, digest)?;
    #[cfg(target_os = "macos")]
    let path = unpack_installer(&path, dir.path())?;
    Ok((dir, path))
}

#[cfg(any(target_os = "macos", windows))]
fn write_verified(
    source: impl Read,
    path: &std::path::Path,
    size: u64,
    digest: &str,
) -> Result<()> {
    use sha2::{Digest, Sha256};
    use std::{fs::File, io};

    let mut file = File::create(path)?;
    if io::copy(&mut source.take(size + 1), &mut file)? != size {
        return Err("Installer size mismatch".into());
    }
    drop(file);
    let mut checksum = Sha256::new();
    io::copy(&mut File::open(path)?, &mut checksum)?;
    if !format!("{:x}", checksum.finalize()).eq_ignore_ascii_case(digest) {
        return Err("Installer checksum mismatch".into());
    }

    Ok(())
}

#[cfg(target_os = "macos")]
fn unpack_installer(
    archive: &std::path::Path,
    dir: &std::path::Path,
) -> Result<std::path::PathBuf> {
    use std::process::Command;

    if !Command::new("/usr/bin/ditto")
        .args(["-x", "-k"])
        .arg(archive)
        .arg(dir)
        .status()?
        .success()
    {
        return Err("Could not unpack the installer".into());
    }
    let app = dir.join("Notedd Setup.app");
    if !app.join("Contents/MacOS/Notedd Setup").is_file()
        || !Command::new("/usr/bin/codesign")
            .args(["--verify", "--strict"])
            .arg(&app)
            .status()?
            .success()
    {
        return Err("Invalid installer bundle".into());
    }
    Ok(app)
}

#[cfg(any(target_os = "macos", windows))]
pub fn open_installer(path: &std::path::Path) -> std::io::Result<()> {
    use std::process::Command;
    #[cfg(windows)]
    Command::new(path).spawn()?;
    #[cfg(target_os = "macos")]
    if !Command::new("/usr/bin/open").arg(path).status()?.success() {
        return Err(std::io::Error::other("Could not open the installer app"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release(version: &str) -> Release {
        Release {
            tag_name: version.into(),
            draft: false,
            prerelease: false,
            assets: vec![Asset {
                name: "setup.zip".into(),
                size: 42,
                digest: Some(format!("sha256:{}", "ab".repeat(32))),
            }],
        }
    }

    #[test]
    fn only_offers_newer_stable_releases() {
        let current = Version::new(2, 9, 0);
        for tag in ["v2.8.0", "v2.9.0", "v2.9.0+build", "v2.10.0-rc.1"] {
            assert!(new_update(release(tag), &current, None).unwrap().is_none());
        }
        assert!(new_update(release("v2.10.0"), &current, None)
            .unwrap()
            .is_some());
    }

    #[test]
    fn picks_only_the_matching_asset_and_requires_valid_integrity_metadata() {
        let current = Version::new(2, 9, 0);
        let update = new_update(release("v2.10.0"), &current, Some("setup.zip"))
            .unwrap()
            .unwrap();
        assert_eq!(
            update.download_url().as_deref(),
            Some("https://github.com/TheKucel/Notedd/releases/download/v2.10.0/setup.zip")
        );
        assert!(update.verified_asset().is_some());
        let update = new_update(release("v2.10.0"), &current, Some("other.zip"))
            .unwrap()
            .unwrap();
        assert!(update.download_url().is_none());

        let mut missing_digest = release("v2.10.0");
        missing_digest.assets[0].digest = None;
        let update = new_update(missing_digest, &current, Some("setup.zip"))
            .unwrap()
            .unwrap();
        assert!(update.verified_asset().is_none());
    }

    #[cfg(any(target_os = "macos", windows))]
    #[test]
    fn rejects_truncated_oversized_and_corrupt_installers() {
        use sha2::{Digest, Sha256};

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("installer");
        let bytes = b"installer";
        let digest = format!("{:x}", Sha256::digest(bytes));
        let size = bytes.len() as u64;
        write_verified(bytes.as_slice(), &path, size, &digest).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        for wrong in [b"install".as_slice(), b"installer!", b"installar"] {
            assert!(write_verified(wrong, &path, size, &digest).is_err());
        }
    }
}
