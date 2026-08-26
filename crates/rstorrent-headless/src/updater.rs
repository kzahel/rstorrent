use std::collections::BTreeSet;
use std::fmt;
use std::io::Cursor;
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use flate2::read::GzDecoder;
use futures_util::StreamExt;
use minisign_verify::{PublicKey, Signature};
use reqwest::redirect::Policy;
use sha2::{Digest, Sha256};

use crate::installer::{self, BundleLayout, InstallOutcome};

pub const MANIFEST_NAME: &str = "rstorrent-headless-release.manifest";
pub const SIGNATURE_NAME: &str = "rstorrent-headless-release.manifest.minisig";
pub const RELEASE_SCHEMA: &str = "rstorrent-headless-release-v1";
pub const REPOSITORY: &str = "kzahel/rstorrent";
pub const RUNTIME: &str = "linux-gnu-headless-package";
pub const INSTALL_PROTOCOL_VERSION: &str = "1";
pub const MINISIGN_PUBLIC_KEY: &str = "RWSWcDYxUXiKeJJ+KkHnWgOjCVOTZ6det0/BsM5QiFH+ohMb464FcQfL";
pub const STABLE_MANIFEST_URL: &str = "https://rstorrent.com/releases/headless/stable.manifest";
pub const STABLE_SIGNATURE_URL: &str =
    "https://rstorrent.com/releases/headless/stable.manifest.minisig";
pub const MAX_METADATA_BYTES: usize = 64 * 1024;
pub const MAX_ASSET_BYTES: usize = 128 * 1024 * 1024;
pub const MAX_EXPANDED_BYTES: u64 = 256 * 1024 * 1024;
pub const MAX_ARCHIVE_ENTRIES: usize = 4096;

const METADATA_TIMEOUT: Duration = Duration::from_secs(30);
const ASSET_TIMEOUT: Duration = Duration::from_secs(180);

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct ReleaseVersion {
    major: u64,
    minor: u64,
    patch: u64,
}

impl ReleaseVersion {
    pub fn parse(value: &str) -> Result<Self, UpdateError> {
        let parts = value.split('.').collect::<Vec<_>>();
        if parts.len() != 3 {
            return Err(UpdateError::Invalid(
                "release version must be numeric MAJOR.MINOR.PATCH".to_owned(),
            ));
        }
        let parse_part = |part: &str| {
            if part.is_empty()
                || (part.len() > 1 && part.starts_with('0'))
                || !part.bytes().all(|byte| byte.is_ascii_digit())
            {
                return Err(UpdateError::Invalid(
                    "release version must be numeric MAJOR.MINOR.PATCH".to_owned(),
                ));
            }
            part.parse::<u64>().map_err(|_| {
                UpdateError::Invalid("release version component is too large".to_owned())
            })
        };
        Ok(Self {
            major: parse_part(parts[0])?,
            minor: parse_part(parts[1])?,
            patch: parse_part(parts[2])?,
        })
    }
}

impl fmt::Display for ReleaseVersion {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReleaseAsset {
    pub name: String,
    pub sha256: String,
    pub size: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReleaseManifest {
    pub version: ReleaseVersion,
    pub tag: String,
    pub source_commit: String,
    pub x86_64: ReleaseAsset,
    pub aarch64: ReleaseAsset,
}

impl ReleaseManifest {
    pub fn parse(bytes: &[u8]) -> Result<Self, UpdateError> {
        if bytes.is_empty() || bytes.len() > MAX_METADATA_BYTES || bytes.contains(&b'\r') {
            return Err(UpdateError::Invalid(
                "release manifest has an invalid byte shape".to_owned(),
            ));
        }
        let source = std::str::from_utf8(bytes)
            .map_err(|_| UpdateError::Invalid("release manifest is not UTF-8".to_owned()))?;
        if !source.ends_with('\n') {
            return Err(UpdateError::Invalid(
                "release manifest must end with one newline".to_owned(),
            ));
        }
        let lines = source.lines().collect::<Vec<_>>();
        if lines.len() != 15 || lines[0] != RELEASE_SCHEMA {
            return Err(UpdateError::Invalid(
                "release manifest has an unsupported schema or field count".to_owned(),
            ));
        }
        let expected = [
            "version",
            "tag",
            "repository",
            "source_commit",
            "install_protocol",
            "runtime",
            "x86_64_asset",
            "x86_64_sha256",
            "x86_64_size",
            "aarch64_asset",
            "aarch64_sha256",
            "aarch64_size",
            "manifest_asset",
            "signature_asset",
        ];
        let fields = lines[1..]
            .iter()
            .map(|line| {
                let (key, value) = line.split_once('=').ok_or_else(|| {
                    UpdateError::Invalid("release manifest field has no value".to_owned())
                })?;
                if key.is_empty() || value.is_empty() || value.contains('=') {
                    return Err(UpdateError::Invalid(
                        "release manifest field is malformed".to_owned(),
                    ));
                }
                Ok((key, value))
            })
            .collect::<Result<Vec<_>, UpdateError>>()?;
        let actual_keys = fields.iter().map(|(key, _)| *key).collect::<Vec<_>>();
        if actual_keys != expected {
            return Err(UpdateError::Invalid(
                "release manifest fields are missing, reordered, or unknown".to_owned(),
            ));
        }
        let value = |index: usize| fields[index].1;
        let version = ReleaseVersion::parse(value(0))?;
        let tag = value(1);
        if tag != format!("headless-v{version}")
            || value(2) != REPOSITORY
            || !valid_commit(value(3))
            || value(4) != INSTALL_PROTOCOL_VERSION
            || value(5) != RUNTIME
            || value(12) != MANIFEST_NAME
            || value(13) != SIGNATURE_NAME
        {
            return Err(UpdateError::Invalid(
                "release manifest identity or compatibility is invalid".to_owned(),
            ));
        }
        Ok(Self {
            version,
            tag: tag.to_owned(),
            source_commit: value(3).to_owned(),
            x86_64: parse_asset(&version, "x86_64", &fields[6..9])?,
            aarch64: parse_asset(&version, "aarch64", &fields[9..12])?,
        })
    }

    pub fn asset_for_host(&self) -> Result<&ReleaseAsset, UpdateError> {
        match std::env::consts::ARCH {
            "x86_64" => Ok(&self.x86_64),
            "aarch64" => Ok(&self.aarch64),
            architecture => Err(UpdateError::Invalid(format!(
                "unsupported headless update architecture {architecture}"
            ))),
        }
    }

    pub fn release_url(&self) -> String {
        format!("https://github.com/{REPOSITORY}/releases/tag/{}", self.tag)
    }
}

#[derive(Clone, Debug)]
pub struct ReleaseCandidate {
    pub manifest: ReleaseManifest,
    pub asset: ReleaseAsset,
}

impl ReleaseCandidate {
    pub fn version(&self) -> String {
        self.manifest.version.to_string()
    }

    pub fn release_url(&self) -> String {
        self.manifest.release_url()
    }

    fn asset_url(&self) -> String {
        format!(
            "https://github.com/{REPOSITORY}/releases/download/{}/{}",
            self.manifest.tag, self.asset.name
        )
    }
}

#[derive(Clone)]
pub struct UpdateClient {
    http: reqwest::Client,
    manifest_url: String,
    signature_url: String,
    public_key: PublicKey,
}

impl UpdateClient {
    pub fn production() -> Result<Self, UpdateError> {
        Self::new(
            STABLE_MANIFEST_URL,
            STABLE_SIGNATURE_URL,
            MINISIGN_PUBLIC_KEY,
            false,
        )
    }

    fn new(
        manifest_url: &str,
        signature_url: &str,
        public_key: &str,
        allow_http: bool,
    ) -> Result<Self, UpdateError> {
        for url in [manifest_url, signature_url] {
            let parsed = url::Url::parse(url)
                .map_err(|_| UpdateError::Invalid("update URL is invalid".to_owned()))?;
            if parsed.scheme() != "https" && !(allow_http && parsed.scheme() == "http") {
                return Err(UpdateError::Invalid(
                    "production update URLs must use HTTPS".to_owned(),
                ));
            }
        }
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .redirect(Policy::custom(move |attempt| {
                if attempt.previous().len() >= 5 {
                    return attempt.error("too many update redirects");
                }
                if attempt.url().scheme() == "https"
                    || (allow_http && attempt.url().scheme() == "http")
                {
                    attempt.follow()
                } else {
                    attempt.error("update redirect changed to a non-HTTPS URL")
                }
            }))
            .build()
            .map_err(|error| UpdateError::Network(format!("build update client: {error}")))?;
        let public_key = PublicKey::from_base64(public_key)
            .map_err(|_| UpdateError::Invalid("embedded updater key is invalid".to_owned()))?;
        Ok(Self {
            http,
            manifest_url: manifest_url.to_owned(),
            signature_url: signature_url.to_owned(),
            public_key,
        })
    }

    pub async fn check(
        &self,
        current_version: &str,
    ) -> Result<Option<ReleaseCandidate>, UpdateError> {
        let current = ReleaseVersion::parse(current_version)?;
        let manifest_bytes = self
            .download_bounded(&self.manifest_url, MAX_METADATA_BYTES, METADATA_TIMEOUT)
            .await?;
        let signature_bytes = self
            .download_bounded(&self.signature_url, MAX_METADATA_BYTES, METADATA_TIMEOUT)
            .await?;
        verify_signature(&self.public_key, &manifest_bytes, &signature_bytes)?;
        let manifest = ReleaseManifest::parse(&manifest_bytes)?;
        if manifest.version < current {
            return Err(UpdateError::Invalid(format!(
                "stable channel attempts to downgrade {current} to {}",
                manifest.version
            )));
        }
        if manifest.version == current {
            return Ok(None);
        }
        let asset = manifest.asset_for_host()?.clone();
        Ok(Some(ReleaseCandidate { manifest, asset }))
    }

    pub async fn apply(&self, candidate: &ReleaseCandidate) -> Result<InstallOutcome, UpdateError> {
        let bytes = self
            .download_bounded(&candidate.asset_url(), candidate.asset.size, ASSET_TIMEOUT)
            .await?;
        if bytes.len() != candidate.asset.size
            || format!("{:x}", Sha256::digest(&bytes)) != candidate.asset.sha256
        {
            return Err(UpdateError::Invalid(
                "downloaded package failed its signed size or SHA-256 check".to_owned(),
            ));
        }
        let temporary = tempfile::tempdir()
            .map_err(|error| UpdateError::Io(format!("create update directory: {error}")))?;
        extract_archive(&bytes, temporary.path())?;
        let bundle = BundleLayout::validate(temporary.path())
            .map_err(|error| UpdateError::Invalid(error.to_string()))?;
        if bundle.version != candidate.version() {
            return Err(UpdateError::Invalid(
                "extracted package version differs from its signed manifest".to_owned(),
            ));
        }
        installer::install_bundle(temporary.path())
            .map_err(|error| UpdateError::Install(error.to_string()))
    }

    async fn download_bounded(
        &self,
        url: &str,
        maximum: usize,
        timeout: Duration,
    ) -> Result<Vec<u8>, UpdateError> {
        let request = async {
            let response = self
                .http
                .get(url)
                .send()
                .await
                .map_err(|error| UpdateError::Network(format!("download {url}: {error}")))?
                .error_for_status()
                .map_err(|error| UpdateError::Network(format!("download {url}: {error}")))?;
            if response
                .content_length()
                .is_some_and(|length| length > maximum as u64)
            {
                return Err(UpdateError::Invalid(format!(
                    "download from {url} exceeds {maximum} bytes"
                )));
            }
            let mut stream = response.bytes_stream();
            let mut bytes = Vec::with_capacity(maximum.min(64 * 1024));
            while let Some(chunk) = stream.next().await {
                let chunk = chunk.map_err(|error| {
                    UpdateError::Network(format!("read download from {url}: {error}"))
                })?;
                if bytes.len().saturating_add(chunk.len()) > maximum {
                    return Err(UpdateError::Invalid(format!(
                        "download from {url} exceeds {maximum} bytes"
                    )));
                }
                bytes.extend_from_slice(&chunk);
            }
            if bytes.is_empty() {
                return Err(UpdateError::Invalid(format!(
                    "download from {url} was empty"
                )));
            }
            Ok(bytes)
        };
        tokio::time::timeout(timeout, request)
            .await
            .map_err(|_| UpdateError::Network(format!("download from {url} timed out")))?
    }
}

#[derive(Debug)]
pub enum UpdateError {
    Invalid(String),
    Network(String),
    Io(String),
    Install(String),
}

impl fmt::Display for UpdateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(message)
            | Self::Network(message)
            | Self::Io(message)
            | Self::Install(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for UpdateError {}

fn parse_asset(
    version: &ReleaseVersion,
    architecture: &str,
    fields: &[(&str, &str)],
) -> Result<ReleaseAsset, UpdateError> {
    let expected_name = format!("rstorrent-headless-{version}-linux-{architecture}.tar.gz");
    let name = fields[0].1;
    let sha256 = fields[1].1;
    let size = fields[2]
        .1
        .parse::<usize>()
        .map_err(|_| UpdateError::Invalid("release asset size is invalid".to_owned()))?;
    if name != expected_name
        || sha256.len() != 64
        || !sha256
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        || size == 0
        || size > MAX_ASSET_BYTES
    {
        return Err(UpdateError::Invalid(format!(
            "release manifest has invalid {architecture} package metadata"
        )));
    }
    Ok(ReleaseAsset {
        name: name.to_owned(),
        sha256: sha256.to_owned(),
        size,
    })
}

fn valid_commit(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

fn verify_signature(
    public_key: &PublicKey,
    manifest: &[u8],
    signature: &[u8],
) -> Result<(), UpdateError> {
    let signature = std::str::from_utf8(signature)
        .map_err(|_| UpdateError::Invalid("release signature is not UTF-8".to_owned()))?;
    let signature = Signature::decode(signature)
        .map_err(|_| UpdateError::Invalid("release signature is malformed".to_owned()))?;
    public_key
        .verify(manifest, &signature, false)
        .map_err(|_| UpdateError::Invalid("release signature verification failed".to_owned()))
}

fn extract_archive(bytes: &[u8], destination: &Path) -> Result<(), UpdateError> {
    let decoder = GzDecoder::new(Cursor::new(bytes));
    let mut archive = tar::Archive::new(decoder);
    let entries = archive
        .entries()
        .map_err(|error| UpdateError::Invalid(format!("read package archive: {error}")))?;
    let mut count = 0usize;
    let mut expanded = 0u64;
    let mut destinations = BTreeSet::<PathBuf>::new();
    for entry in entries {
        let mut entry =
            entry.map_err(|error| UpdateError::Invalid(format!("read package entry: {error}")))?;
        count = count.saturating_add(1);
        if count > MAX_ARCHIVE_ENTRIES {
            return Err(UpdateError::Invalid(
                "package archive contains too many entries".to_owned(),
            ));
        }
        let entry_type = entry.header().entry_type();
        if !entry_type.is_file() && !entry_type.is_dir() {
            return Err(UpdateError::Invalid(
                "package archive contains a link or special entry".to_owned(),
            ));
        }
        expanded = expanded.saturating_add(entry.size());
        if expanded > MAX_EXPANDED_BYTES {
            return Err(UpdateError::Invalid(
                "package archive exceeds its expanded byte limit".to_owned(),
            ));
        }
        let raw_path = entry
            .path()
            .map_err(|error| UpdateError::Invalid(format!("read package path: {error}")))?;
        let normalized = normalize_archive_path(&raw_path)?;
        if normalized.as_os_str().is_empty() {
            if entry_type.is_dir() {
                continue;
            }
            return Err(UpdateError::Invalid(
                "package archive contains an empty file path".to_owned(),
            ));
        }
        if !destinations.insert(normalized.clone()) {
            return Err(UpdateError::Invalid(
                "package archive contains a duplicate destination".to_owned(),
            ));
        }
        let output = destination.join(&normalized);
        if entry_type.is_dir() {
            std::fs::create_dir_all(&output).map_err(|error| {
                UpdateError::Io(format!(
                    "create package directory {}: {error}",
                    output.display()
                ))
            })?;
        } else {
            let parent = output.parent().ok_or_else(|| {
                UpdateError::Invalid("package file has no parent directory".to_owned())
            })?;
            std::fs::create_dir_all(parent).map_err(|error| {
                UpdateError::Io(format!(
                    "create package directory {}: {error}",
                    parent.display()
                ))
            })?;
            let mut file = std::fs::File::create(&output).map_err(|error| {
                UpdateError::Io(format!("create package file {}: {error}", output.display()))
            })?;
            std::io::copy(&mut entry, &mut file).map_err(|error| {
                UpdateError::Io(format!(
                    "extract package file {}: {error}",
                    output.display()
                ))
            })?;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = entry.header().mode().map_err(|error| {
                UpdateError::Invalid(format!("read package mode {}: {error}", output.display()))
            })? & 0o777;
            std::fs::set_permissions(&output, std::fs::Permissions::from_mode(mode)).map_err(
                |error| UpdateError::Io(format!("set package mode {}: {error}", output.display())),
            )?;
        }
    }
    if count == 0 {
        return Err(UpdateError::Invalid(
            "package archive contains no entries".to_owned(),
        ));
    }
    Ok(())
}

fn normalize_archive_path(path: &Path) -> Result<PathBuf, UpdateError> {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir if normalized.as_os_str().is_empty() => {}
            Component::Normal(value) => {
                let value = value
                    .to_str()
                    .ok_or_else(|| UpdateError::Invalid("package path is not UTF-8".to_owned()))?;
                if value.is_empty() || value.contains('\\') {
                    return Err(UpdateError::Invalid(
                        "package path contains an unsafe component".to_owned(),
                    ));
                }
                normalized.push(value);
            }
            Component::CurDir
            | Component::ParentDir
            | Component::RootDir
            | Component::Prefix(_) => {
                return Err(UpdateError::Invalid(
                    "package path contains traversal or an absolute root".to_owned(),
                ));
            }
        }
    }
    Ok(normalized)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(version: &str) -> Vec<u8> {
        format!(
            "{RELEASE_SCHEMA}\n\
             version={version}\n\
             tag=headless-v{version}\n\
             repository={REPOSITORY}\n\
             source_commit=0123456789abcdef0123456789abcdef01234567\n\
             install_protocol={INSTALL_PROTOCOL_VERSION}\n\
             runtime={RUNTIME}\n\
             x86_64_asset=rstorrent-headless-{version}-linux-x86_64.tar.gz\n\
             x86_64_sha256={}\n\
             x86_64_size=123\n\
             aarch64_asset=rstorrent-headless-{version}-linux-aarch64.tar.gz\n\
             aarch64_sha256={}\n\
             aarch64_size=456\n\
             manifest_asset={MANIFEST_NAME}\n\
             signature_asset={SIGNATURE_NAME}\n",
            "a".repeat(64),
            "b".repeat(64),
        )
        .into_bytes()
    }

    #[test]
    fn parses_strict_manifest_and_numeric_versions() {
        let parsed = ReleaseManifest::parse(&manifest("1.2.3")).expect("manifest");
        assert_eq!(parsed.version.to_string(), "1.2.3");
        assert_eq!(parsed.x86_64.size, 123);
        assert!(ReleaseVersion::parse("1.2.4").unwrap() > parsed.version);
        for invalid in ["1.2", "1.2.3-dev", "01.2.3", "1.2.3.4"] {
            assert!(ReleaseVersion::parse(invalid).is_err(), "{invalid}");
        }
    }

    #[test]
    fn rejects_manifest_identity_order_and_asset_drift() {
        let valid = manifest("0.1.0");
        for (from, to) in [
            ("repository=kzahel/rstorrent", "repository=other/repository"),
            ("runtime=linux-gnu-headless-package", "runtime=other"),
            ("install_protocol=1", "install_protocol=2"),
            ("linux-x86_64.tar.gz", "linux-aarch64.tar.gz"),
        ] {
            let changed = String::from_utf8(valid.clone()).unwrap().replace(from, to);
            assert!(ReleaseManifest::parse(changed.as_bytes()).is_err(), "{to}");
        }
        let reordered = String::from_utf8(valid).unwrap().replace(
            "install_protocol=1\nruntime=linux-gnu-headless-package",
            "runtime=linux-gnu-headless-package\ninstall_protocol=1",
        );
        assert!(ReleaseManifest::parse(reordered.as_bytes()).is_err());
    }

    #[test]
    fn rejects_unsafe_archive_paths() {
        assert_eq!(
            normalize_archive_path(Path::new("./bin/rstorrent-headless")).unwrap(),
            PathBuf::from("bin/rstorrent-headless")
        );
        for path in ["../escape", "/absolute", "safe/../escape", "safe\\escape"] {
            assert!(normalize_archive_path(Path::new(path)).is_err(), "{path}");
        }
    }

    #[test]
    fn rejects_malformed_release_signature() {
        let public_key = PublicKey::from_base64(MINISIGN_PUBLIC_KEY).expect("embedded key");
        assert!(verify_signature(&public_key, b"manifest", b"not a signature").is_err());
    }
}
