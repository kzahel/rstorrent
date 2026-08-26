use std::collections::BTreeSet;
use std::fmt;
use std::fs::File;
use std::io::Read;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::path::{Component, Path, PathBuf};

use rstorrent_session::{
    ConfiguredStorageRoot, MAX_ROOT_ID_LENGTH, MAX_ROOT_LABEL_LENGTH,
    MAX_STORAGE_ROOT_LOCATOR_LENGTH, MAX_STORAGE_ROOTS, StorageRootLocation,
};
use serde::Deserialize;
use url::Url;

pub const CONFIG_VERSION: u32 = 1;
pub const MAX_CONFIG_BYTES: usize = 64 * 1024;
pub const MAX_USERNAME_BYTES: usize = 64;
pub const MAX_PASSWORD_BYTES: usize = 128;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HeadlessConfig {
    pub profile_root: PathBuf,
    pub listen: SocketAddr,
    pub public_origin: String,
    pub storage_roots: Vec<ConfiguredStorageRoot>,
    pub authentication: AuthenticationConfig,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AuthenticationConfig {
    LocalBrowser,
    LanNone,
    Basic {
        username: String,
        password_file: PathBuf,
    },
}

impl AuthenticationConfig {
    pub fn mode_name(&self) -> &'static str {
        match self {
            Self::LocalBrowser => "local-browser",
            Self::LanNone => "lan-none",
            Self::Basic { .. } => "basic",
        }
    }
}

impl HeadlessConfig {
    pub fn redacted_summary(&self) -> String {
        let roots = self
            .storage_roots
            .iter()
            .map(|root| match &root.location {
                StorageRootLocation::Path(path) => format!("{}={}", root.id, path.display()),
                StorageRootLocation::PlatformCapability => format!("{}=[platform]", root.id),
            })
            .collect::<Vec<_>>()
            .join(",");
        format!(
            "config_version={CONFIG_VERSION} profile_root={} listen={} public_origin={} authentication={} storage_roots=[{roots}]",
            self.profile_root.display(),
            self.listen,
            self.public_origin,
            self.authentication.mode_name(),
        )
    }
}

pub struct BasicCredentials {
    username: String,
    password: String,
}

impl BasicCredentials {
    pub fn username(&self) -> &str {
        &self.username
    }

    pub fn password(&self) -> &str {
        &self.password
    }
}

impl fmt::Debug for BasicCredentials {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("BasicCredentials")
            .field("username", &"[redacted]")
            .field("password", &"[redacted]")
            .finish()
    }
}

#[derive(Debug)]
pub enum ConfigError {
    Io {
        operation: &'static str,
        path: PathBuf,
        source: std::io::Error,
    },
    Invalid(String),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io {
                operation,
                path,
                source,
            } => write!(formatter, "{operation} {}: {source}", path.display()),
            Self::Invalid(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for ConfigError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Invalid(_) => None,
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawConfig {
    version: u32,
    profile_root: String,
    listen: String,
    public_origin: String,
    storage_roots: Vec<RawStorageRoot>,
    authentication: RawAuthentication,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawStorageRoot {
    id: String,
    label: String,
    path: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawAuthentication {
    mode: String,
    username: Option<String>,
    password_file: Option<String>,
}

pub fn parse(bytes: &[u8]) -> Result<HeadlessConfig, ConfigError> {
    if bytes.len() > MAX_CONFIG_BYTES {
        return Err(invalid(format!(
            "configuration exceeds {MAX_CONFIG_BYTES} bytes"
        )));
    }
    let source =
        std::str::from_utf8(bytes).map_err(|_| invalid("configuration is not valid UTF-8"))?;
    let raw: RawConfig = toml::from_str(source)
        .map_err(|error| invalid(format!("invalid configuration: {error}")))?;
    if raw.version != CONFIG_VERSION {
        return Err(invalid(format!(
            "configuration version must be {CONFIG_VERSION}"
        )));
    }

    let profile_root = validate_path(&raw.profile_root, "profile_root")?;
    let listen = raw
        .listen
        .parse::<SocketAddr>()
        .map_err(|_| invalid("listen must be one concrete IP socket address"))?;
    validate_listener_basics(listen)?;
    let public_origin = validate_origin(&raw.public_origin)?;
    let authentication = validate_authentication(raw.authentication, listen, &public_origin)?;

    if raw.storage_roots.is_empty() || raw.storage_roots.len() > MAX_STORAGE_ROOTS {
        return Err(invalid(format!(
            "storage_roots must contain 1..={MAX_STORAGE_ROOTS} entries"
        )));
    }
    let mut ids = BTreeSet::new();
    let mut paths = BTreeSet::new();
    let mut storage_roots = Vec::with_capacity(raw.storage_roots.len());
    for root in raw.storage_roots {
        validate_identifier(&root.id)?;
        if root.label.is_empty()
            || root.label.len() > MAX_ROOT_LABEL_LENGTH
            || root.label.chars().any(char::is_control)
        {
            return Err(invalid(format!(
                "storage root label must be 1..={MAX_ROOT_LABEL_LENGTH} bytes without control characters"
            )));
        }
        let path = validate_path(&root.path, "storage root path")?;
        if !ids.insert(root.id.clone()) {
            return Err(invalid(format!(
                "storage root ID {} is duplicated",
                root.id
            )));
        }
        if !paths.insert(path.clone()) {
            return Err(invalid(format!(
                "storage root path {} is duplicated",
                path.display()
            )));
        }
        if paths_overlap(&profile_root, &path) {
            return Err(invalid(format!(
                "storage root {} overlaps profile_root",
                root.id
            )));
        }
        storage_roots.push(ConfiguredStorageRoot::path(root.id, path).with_label(root.label));
    }

    Ok(HeadlessConfig {
        profile_root,
        listen,
        public_origin,
        storage_roots,
        authentication,
    })
}

pub fn load(path: &Path) -> Result<HeadlessConfig, ConfigError> {
    validate_absolute_path(path, "configuration path")?;
    let mut file = open_checked(path, CheckedFileKind::Configuration)?;
    let mut bytes = Vec::with_capacity(4096);
    file.by_ref()
        .take((MAX_CONFIG_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|source| ConfigError::Io {
            operation: "read configuration",
            path: path.to_path_buf(),
            source,
        })?;
    let config = parse(&bytes)?;
    validate_runtime_paths(&config, path, &[])?;
    Ok(config)
}

pub fn validate_runtime_paths(
    config: &HeadlessConfig,
    config_path: &Path,
    release_paths: &[&Path],
) -> Result<(), ConfigError> {
    let config_parent = config_path
        .parent()
        .ok_or_else(|| invalid("configuration path has no parent directory"))?;
    validate_existing_path(&config.profile_root)?;
    for root in &config.storage_roots {
        let StorageRootLocation::Path(path) = &root.location else {
            return Err(invalid("headless storage roots must be path-backed"));
        };
        validate_existing_path(path)?;
        if paths_overlap(path, config_parent)
            || release_paths
                .iter()
                .any(|release_path| paths_overlap(path, release_path))
        {
            return Err(invalid(format!(
                "storage root {} overlaps protected configuration or release state",
                root.id
            )));
        }
    }
    Ok(())
}

pub fn load_basic_credentials(config: &HeadlessConfig) -> Result<BasicCredentials, ConfigError> {
    let AuthenticationConfig::Basic {
        username,
        password_file,
    } = &config.authentication
    else {
        return Err(invalid(
            "Basic credentials requested for non-Basic authentication",
        ));
    };
    let mut file = open_checked(password_file, CheckedFileKind::Secret)?;
    let mut bytes = Vec::with_capacity(MAX_PASSWORD_BYTES + 2);
    file.by_ref()
        .take((MAX_PASSWORD_BYTES + 3) as u64)
        .read_to_end(&mut bytes)
        .map_err(|source| ConfigError::Io {
            operation: "read Basic password file",
            path: password_file.clone(),
            source,
        })?;
    if bytes.ends_with(b"\r\n") {
        bytes.truncate(bytes.len() - 2);
    } else if bytes.ends_with(b"\n") {
        bytes.truncate(bytes.len() - 1);
    }
    if bytes.is_empty()
        || bytes.len() > MAX_PASSWORD_BYTES
        || bytes.contains(&b'\r')
        || bytes.contains(&b'\n')
    {
        return Err(invalid(format!(
            "Basic password must be 1..={MAX_PASSWORD_BYTES} bytes with at most one final line ending"
        )));
    }
    let password =
        String::from_utf8(bytes).map_err(|_| invalid("Basic password must be valid UTF-8"))?;
    Ok(BasicCredentials {
        username: username.clone(),
        password,
    })
}

fn validate_authentication(
    raw: RawAuthentication,
    listen: SocketAddr,
    public_origin: &str,
) -> Result<AuthenticationConfig, ConfigError> {
    match raw.mode.as_str() {
        "local-browser" => {
            if raw.username.is_some() || raw.password_file.is_some() {
                return Err(invalid(
                    "local-browser authentication does not accept username or password_file",
                ));
            }
            if !listen.ip().is_loopback() {
                return Err(invalid(
                    "local-browser authentication requires a loopback listener",
                ));
            }
            let expected = format!("http://{listen}");
            if public_origin != expected {
                return Err(invalid(format!(
                    "local-browser public_origin must be exactly {expected}"
                )));
            }
            Ok(AuthenticationConfig::LocalBrowser)
        }
        "basic" => {
            if !is_private_unicast(listen.ip()) {
                return Err(invalid(
                    "Basic authentication requires a loopback or private unicast listener",
                ));
            }
            if !public_origin.starts_with("https://") {
                return Err(invalid(
                    "Basic authentication requires an exact HTTPS public_origin",
                ));
            }
            let username = raw
                .username
                .ok_or_else(|| invalid("Basic authentication requires username"))?;
            if username.is_empty()
                || username.len() > MAX_USERNAME_BYTES
                || username.contains(':')
                || username.chars().any(char::is_control)
            {
                return Err(invalid(format!(
                    "Basic username must be 1..={MAX_USERNAME_BYTES} bytes without colon or control characters"
                )));
            }
            let password_file = validate_path(
                &raw.password_file
                    .ok_or_else(|| invalid("Basic authentication requires password_file"))?,
                "password_file",
            )?;
            Ok(AuthenticationConfig::Basic {
                username,
                password_file,
            })
        }
        "lan-none" => {
            if raw.username.is_some() || raw.password_file.is_some() {
                return Err(invalid(
                    "lan-none authentication does not accept username or password_file",
                ));
            }
            let IpAddr::V4(address) = listen.ip() else {
                return Err(invalid(
                    "lan-none requires one exact non-loopback RFC 1918 IPv4 listener",
                ));
            };
            if address.is_loopback() || !address.is_private() {
                return Err(invalid(
                    "lan-none requires one exact non-loopback RFC 1918 IPv4 listener",
                ));
            }
            let expected = format!("http://{listen}");
            if public_origin != expected {
                return Err(invalid(format!(
                    "lan-none public_origin must be exactly {expected}"
                )));
            }
            Ok(AuthenticationConfig::LanNone)
        }
        _ => Err(invalid(
            "authentication mode must be local-browser, lan-none, or basic",
        )),
    }
}

fn validate_listener_basics(listen: SocketAddr) -> Result<(), ConfigError> {
    if listen.port() == 0 {
        return Err(invalid("listen port must be nonzero"));
    }
    if listen.ip().is_unspecified() || listen.ip().is_multicast() {
        return Err(invalid("listen address must not be wildcard or multicast"));
    }
    Ok(())
}

fn validate_origin(raw: &str) -> Result<String, ConfigError> {
    let origin = Url::parse(raw).map_err(|_| invalid("public_origin is not a valid URL origin"))?;
    if origin.scheme() != "http" && origin.scheme() != "https" {
        return Err(invalid("public_origin must use HTTP or HTTPS"));
    }
    if origin.host_str().is_none()
        || !origin.username().is_empty()
        || origin.password().is_some()
        || origin.path() != "/"
        || origin.query().is_some()
        || origin.fragment().is_some()
    {
        return Err(invalid(
            "public_origin must contain only scheme and authority",
        ));
    }
    let canonical = origin.origin().ascii_serialization();
    if raw != canonical {
        return Err(invalid(format!(
            "public_origin must use its exact canonical origin {canonical}"
        )));
    }
    Ok(canonical)
}

fn validate_identifier(id: &str) -> Result<(), ConfigError> {
    if id.is_empty()
        || id.len() > MAX_ROOT_ID_LENGTH
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
    {
        return Err(invalid(format!(
            "storage root ID must be 1..={MAX_ROOT_ID_LENGTH} ASCII letters, digits, dot, dash, or underscore"
        )));
    }
    Ok(())
}

fn validate_path(raw: &str, field: &str) -> Result<PathBuf, ConfigError> {
    if raw.is_empty()
        || raw.len() > MAX_STORAGE_ROOT_LOCATOR_LENGTH
        || raw.contains('\0')
        || raw.contains('\r')
        || raw.contains('\n')
    {
        return Err(invalid(format!(
            "{field} must be 1..={MAX_STORAGE_ROOT_LOCATOR_LENGTH} bytes without NUL or line endings"
        )));
    }
    let path = PathBuf::from(raw);
    validate_absolute_path(&path, field)?;
    if path
        .components()
        .any(|component| matches!(component, Component::CurDir | Component::ParentDir))
    {
        return Err(invalid(format!(
            "{field} must not contain dot path components"
        )));
    }
    Ok(path)
}

fn validate_absolute_path(path: &Path, field: &str) -> Result<(), ConfigError> {
    if !path.is_absolute() || path.to_str().is_none() {
        return Err(invalid(format!(
            "{field} must be an absolute UTF-8 Linux path"
        )));
    }
    Ok(())
}

fn validate_existing_path(path: &Path) -> Result<(), ConfigError> {
    let mut current = PathBuf::new();
    for component in path.components() {
        current.push(component);
        match std::fs::symlink_metadata(&current) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() {
                    return Err(invalid(format!(
                        "path {} contains a symbolic link",
                        path.display()
                    )));
                }
                if current == path && !metadata.is_dir() {
                    return Err(invalid(format!(
                        "path {} must be a directory when present",
                        path.display()
                    )));
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(source) => {
                return Err(ConfigError::Io {
                    operation: "inspect configured path",
                    path: current,
                    source,
                });
            }
        }
    }
    Ok(())
}

fn paths_overlap(left: &Path, right: &Path) -> bool {
    left.starts_with(right) || right.starts_with(left)
}

fn is_private_unicast(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => ipv4_is_private_unicast(ip),
        IpAddr::V6(ip) => ipv6_is_private_unicast(ip),
    }
}

fn ipv4_is_private_unicast(ip: Ipv4Addr) -> bool {
    ip.is_loopback() || ip.is_private()
}

fn ipv6_is_private_unicast(ip: Ipv6Addr) -> bool {
    ip.is_loopback() || (ip.segments()[0] & 0xfe00) == 0xfc00
}

#[derive(Clone, Copy)]
enum CheckedFileKind {
    Configuration,
    Secret,
}

#[cfg(unix)]
fn open_checked(path: &Path, kind: CheckedFileKind) -> Result<File, ConfigError> {
    use std::os::unix::fs::MetadataExt;

    use rustix::fs::{Mode, OFlags, open};

    validate_absolute_path(path, "file path")?;
    let descriptor = open(
        path,
        OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::empty(),
    )
    .map_err(|error| ConfigError::Io {
        operation: "open protected file",
        path: path.to_path_buf(),
        source: std::io::Error::from_raw_os_error(error.raw_os_error()),
    })?;
    let file = File::from(descriptor);
    let metadata = file.metadata().map_err(|source| ConfigError::Io {
        operation: "inspect protected file",
        path: path.to_path_buf(),
        source,
    })?;
    if !metadata.is_file() {
        return Err(invalid(format!(
            "protected file {} must be regular",
            path.display()
        )));
    }
    if metadata.uid() != rustix::process::getuid().as_raw() {
        return Err(invalid(format!(
            "protected file {} is not owned by the current user",
            path.display()
        )));
    }
    let mode = metadata.mode() & 0o777;
    let forbidden = match kind {
        CheckedFileKind::Configuration => 0o022,
        CheckedFileKind::Secret => 0o077,
    };
    if mode & forbidden != 0 {
        return Err(invalid(format!(
            "protected file {} has unsafe permissions {:03o}",
            path.display(),
            mode
        )));
    }
    Ok(file)
}

#[cfg(not(unix))]
fn open_checked(path: &Path, _kind: CheckedFileKind) -> Result<File, ConfigError> {
    Err(invalid(format!(
        "protected file validation is supported only on Unix: {}",
        path.display()
    )))
}

fn invalid(message: impl Into<String>) -> ConfigError {
    ConfigError::Invalid(message.into())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::{AuthenticationConfig, MAX_CONFIG_BYTES, load, load_basic_credentials, parse};

    static NEXT_TEST: AtomicU64 = AtomicU64::new(0);

    fn test_root(label: &str) -> PathBuf {
        let sequence = NEXT_TEST.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "rstorrent-headless-config-{label}-{}-{sequence}",
            std::process::id()
        ))
    }

    fn basic_config() -> String {
        r#"version = 1
profile_root = "/var/lib/rstorrent/profile"
listen = "127.0.0.1:3030"
public_origin = "https://torrent.example.test"

[[storage_roots]]
id = "downloads"
label = "Downloads"
path = "/srv/media/torrents"

[authentication]
mode = "basic"
username = "owner"
password_file = "/var/lib/rstorrent/password"
"#
        .to_owned()
    }

    fn local_config() -> String {
        basic_config()
            .replace(
                "https://torrent.example.test",
                "http://127.0.0.1:3030",
            )
            .replace(
                "mode = \"basic\"\nusername = \"owner\"\npassword_file = \"/var/lib/rstorrent/password\"",
                "mode = \"local-browser\"",
            )
    }

    fn lan_none_config() -> String {
        basic_config()
            .replace("127.0.0.1:3030", "192.168.1.20:3030")
            .replace(
                "https://torrent.example.test",
                "http://192.168.1.20:3030",
            )
            .replace(
                "mode = \"basic\"\nusername = \"owner\"\npassword_file = \"/var/lib/rstorrent/password\"",
                "mode = \"lan-none\"",
            )
    }

    #[test]
    fn parses_valid_authentication_configurations() {
        let basic = parse(basic_config().as_bytes()).expect("valid Basic configuration");
        assert!(matches!(
            basic.authentication,
            AuthenticationConfig::Basic { .. }
        ));
        let local = parse(local_config().as_bytes()).expect("valid local configuration");
        assert_eq!(local.authentication, AuthenticationConfig::LocalBrowser);
        let lan = parse(lan_none_config().as_bytes()).expect("valid private LAN configuration");
        assert_eq!(lan.authentication, AuthenticationConfig::LanNone);
    }

    #[test]
    fn rejects_unknown_duplicate_wrong_version_and_oversize_input() {
        let unknown = basic_config().replace("version = 1", "version = 1\nextra = true");
        assert!(parse(unknown.as_bytes()).is_err());
        let duplicate = basic_config().replace("version = 1", "version = 1\nversion = 1");
        assert!(parse(duplicate.as_bytes()).is_err());
        let version = basic_config().replace("version = 1", "version = 2");
        assert!(parse(version.as_bytes()).is_err());
        assert!(parse(&vec![b'x'; MAX_CONFIG_BYTES + 1]).is_err());
    }

    #[test]
    fn rejects_invalid_listener_authentication_and_origin_combinations() {
        for invalid in [
            basic_config().replace("127.0.0.1:3030", "0.0.0.0:3030"),
            basic_config().replace("127.0.0.1:3030", "8.8.8.8:3030"),
            basic_config().replace("https://", "http://"),
            basic_config().replace(
                "https://torrent.example.test",
                "https://torrent.example.test/path",
            ),
            local_config().replace("127.0.0.1:3030", "192.168.1.2:3030"),
            local_config().replace("http://127.0.0.1:3030", "http://127.0.0.1:3031"),
        ] {
            assert!(parse(invalid.as_bytes()).is_err(), "accepted:\n{invalid}");
        }
        for invalid in [
            lan_none_config().replace("192.168.1.20:3030", "127.0.0.1:3030"),
            lan_none_config().replace("192.168.1.20:3030", "8.8.8.8:3030"),
            lan_none_config().replace("192.168.1.20:3030", "[fd00::20]:3030"),
            lan_none_config().replace("http://", "https://"),
            lan_none_config().replace(
                "mode = \"lan-none\"",
                "mode = \"lan-none\"\nusername = \"owner\"",
            ),
        ] {
            assert!(parse(invalid.as_bytes()).is_err(), "accepted:\n{invalid}");
        }
    }

    #[test]
    fn rejects_root_identity_path_and_profile_overlaps() {
        let duplicate_id = basic_config().replace(
            "[authentication]",
            "[[storage_roots]]\nid = \"downloads\"\nlabel = \"Other\"\npath = \"/srv/other\"\n\n[authentication]",
        );
        assert!(parse(duplicate_id.as_bytes()).is_err());
        let duplicate_path = basic_config().replace(
            "[authentication]",
            "[[storage_roots]]\nid = \"other\"\nlabel = \"Other\"\npath = \"/srv/media/torrents\"\n\n[authentication]",
        );
        assert!(parse(duplicate_path.as_bytes()).is_err());
        assert!(
            parse(
                basic_config()
                    .replace("/srv/media/torrents", "/var/lib/rstorrent/profile/data")
                    .as_bytes()
            )
            .is_err()
        );
        assert!(
            parse(
                basic_config()
                    .replace("id = \"downloads\"", "id = \"bad/id\"")
                    .as_bytes()
            )
            .is_err()
        );
    }

    #[test]
    fn redacted_summary_never_contains_basic_identity_or_secret_path() {
        let config = parse(basic_config().as_bytes()).expect("valid configuration");
        let summary = config.redacted_summary();
        assert!(!summary.contains("owner"));
        assert!(!summary.contains("password"));
        assert!(summary.contains("authentication=basic"));
    }

    #[cfg(unix)]
    #[test]
    fn protected_files_enforce_permissions_and_no_symlinks() {
        use std::os::unix::fs::{PermissionsExt, symlink};

        let root = test_root("protected-files");
        let config_parent = root.join("config");
        let profile = root.join("profile");
        let payload = root.join("payload");
        fs::create_dir_all(&config_parent).expect("create config directory");
        fs::create_dir_all(&profile).expect("create profile directory");
        fs::create_dir_all(&payload).expect("create payload directory");
        let secret = config_parent.join("secret");
        fs::write(&secret, "correct horse battery staple\n").expect("write secret");
        fs::set_permissions(&secret, fs::Permissions::from_mode(0o600)).expect("chmod secret");
        let config_path = config_parent.join("headless.toml");
        let source = basic_config()
            .replace(
                "/var/lib/rstorrent/profile",
                profile.to_str().expect("profile path"),
            )
            .replace(
                "/srv/media/torrents",
                payload.to_str().expect("payload path"),
            )
            .replace(
                "/var/lib/rstorrent/password",
                secret.to_str().expect("secret path"),
            );
        fs::write(&config_path, source).expect("write configuration");
        fs::set_permissions(&config_path, fs::Permissions::from_mode(0o600))
            .expect("chmod configuration");

        let config = load(&config_path).expect("load protected configuration");
        let credentials = load_basic_credentials(&config).expect("load protected secret");
        assert_eq!(credentials.username(), "owner");
        assert_eq!(credentials.password(), "correct horse battery staple");
        assert!(!format!("{credentials:?}").contains("correct horse"));

        fs::set_permissions(&secret, fs::Permissions::from_mode(0o640)).expect("weaken secret");
        assert!(load_basic_credentials(&config).is_err());
        fs::set_permissions(&secret, fs::Permissions::from_mode(0o600)).expect("restore secret");
        fs::set_permissions(&config_path, fs::Permissions::from_mode(0o622))
            .expect("weaken configuration");
        assert!(load(&config_path).is_err());
        fs::set_permissions(&config_path, fs::Permissions::from_mode(0o600))
            .expect("restore configuration");
        let link = config_parent.join("linked.toml");
        symlink(&config_path, &link).expect("create configuration symlink");
        assert!(load(&link).is_err());

        fs::remove_dir_all(root).expect("remove test root");
    }

    #[cfg(unix)]
    #[test]
    fn runtime_paths_reject_config_tree_and_symlink_components() {
        use std::os::unix::fs::{PermissionsExt, symlink};

        let root = test_root("runtime-paths");
        let config_parent = root.join("config");
        let profile = root.join("profile");
        fs::create_dir_all(&config_parent).expect("create config directory");
        fs::create_dir_all(&profile).expect("create profile directory");
        let secret = config_parent.join("secret");
        fs::write(&secret, "secret").expect("write secret");
        fs::set_permissions(&secret, fs::Permissions::from_mode(0o600)).expect("chmod secret");
        let config_path = config_parent.join("headless.toml");
        let nested_payload = config_parent.join("payload");
        let source = basic_config()
            .replace(
                "/var/lib/rstorrent/profile",
                profile.to_str().expect("profile path"),
            )
            .replace(
                "/srv/media/torrents",
                nested_payload.to_str().expect("payload path"),
            )
            .replace(
                "/var/lib/rstorrent/password",
                secret.to_str().expect("secret path"),
            );
        fs::write(&config_path, source).expect("write configuration");
        fs::set_permissions(&config_path, fs::Permissions::from_mode(0o600))
            .expect("chmod configuration");
        assert!(load(&config_path).is_err());

        let real = root.join("real");
        let linked = root.join("linked");
        fs::create_dir_all(&real).expect("create real root");
        symlink(&real, &linked).expect("create root symlink");
        let source = fs::read_to_string(&config_path)
            .expect("read configuration")
            .replace(
                nested_payload.to_str().expect("nested payload"),
                linked.to_str().expect("linked payload"),
            );
        fs::write(&config_path, source).expect("replace configuration");
        assert!(load(&config_path).is_err());

        fs::remove_dir_all(root).expect("remove test root");
    }
}
