use std::collections::BTreeSet;
use std::fs;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{Duration, Instant};

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use serde::Deserialize;

use crate::config::{AuthenticationConfig, load, load_basic_credentials};
use crate::runtime::{
    HeadlessError, InstalledLayout, read_identity, valid_version, validate_directory,
    validate_regular_file, validate_web_tree,
};
use crate::{PACKAGE_ID, PRODUCT_ID, SERVICE_NAME};

const SERVICE_TEMPLATE: &str =
    include_str!("../resources/com.jstorrent.rstorrent.headless.service.in");
const CONFIG_EXAMPLE: &str = include_str!("../resources/headless.toml.example");
const OWNERSHIP_HEADER: &str = "rstorrent-headless-ownership-v1";
#[cfg(not(test))]
const HEALTH_READY_ATTEMPTS: usize = 101;
#[cfg(test)]
const HEALTH_READY_ATTEMPTS: usize = 3;
#[cfg(not(test))]
const HEALTH_READY_INTERVAL: Duration = Duration::from_millis(50);
#[cfg(test)]
const HEALTH_READY_INTERVAL: Duration = Duration::ZERO;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InstallPaths {
    pub application_root: PathBuf,
    pub versions: PathBuf,
    pub current: PathBuf,
    pub command: PathBuf,
    pub unit: PathBuf,
    pub config: PathBuf,
    pub config_example: PathBuf,
    pub ownership: PathBuf,
}

impl InstallPaths {
    pub fn system() -> Result<Self, HeadlessError> {
        let home = std::env::var_os("HOME")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .ok_or_else(|| HeadlessError::configuration("HOME is unavailable"))?;
        let data = std::env::var_os("XDG_DATA_HOME")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".local/share"));
        let config = std::env::var_os("XDG_CONFIG_HOME")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".config"));
        Self::for_roots(&home, &data, &config)
    }

    pub fn for_roots(home: &Path, data: &Path, config: &Path) -> Result<Self, HeadlessError> {
        for (path, label) in [(home, "home"), (data, "data"), (config, "config")] {
            validate_base_path(path, label)?;
        }
        let application_root = data.join("rstorrent-headless");
        let config_path = config.join("rstorrent/headless.toml");
        Ok(Self {
            versions: application_root.join("versions"),
            current: application_root.join("current"),
            command: home.join(".local/bin/rstorrent-headless"),
            unit: config.join("systemd/user").join(SERVICE_NAME),
            config_example: config_path.with_extension("toml.example"),
            config: config_path,
            ownership: application_root.join("ownership-v1"),
            application_root,
        })
    }

    fn profile_default(&self) -> PathBuf {
        self.application_root.join("profile")
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BundleLayout {
    pub root: PathBuf,
    pub version: String,
    pub architecture: String,
}

impl BundleLayout {
    pub fn validate(root: &Path) -> Result<Self, HeadlessError> {
        validate_directory(root, "package root")?;
        require_exact_entries(
            root,
            &[
                "ARCH",
                "PACKAGE_ID",
                "VERSION",
                "bin",
                "install.sh",
                "resources",
                "web",
            ],
            "package root",
        )?;
        require_exact_entries(
            &root.join("bin"),
            &["rstorrent-gateway", "rstorrent-headless"],
            "package bin",
        )?;
        require_exact_entries(
            &root.join("resources"),
            &[
                "com.jstorrent.rstorrent.headless.service.in",
                "headless.toml.example",
            ],
            "package resources",
        )?;
        let version = read_identity(&root.join("VERSION"), "package VERSION")?;
        if !valid_version(&version) {
            return Err(HeadlessError::configuration("package VERSION is invalid"));
        }
        #[cfg(not(test))]
        if version != env!("CARGO_PKG_VERSION") {
            return Err(HeadlessError::configuration(format!(
                "package VERSION {version} does not match adapter {}",
                env!("CARGO_PKG_VERSION")
            )));
        }
        if read_identity(&root.join("PACKAGE_ID"), "package identity")? != PACKAGE_ID {
            return Err(HeadlessError::configuration(format!(
                "package identity must be {PACKAGE_ID}"
            )));
        }
        let architecture = read_identity(&root.join("ARCH"), "package architecture")?;
        if !matches!(architecture.as_str(), "x86_64" | "aarch64") {
            return Err(HeadlessError::configuration(
                "package architecture must be x86_64 or aarch64",
            ));
        }
        if architecture != std::env::consts::ARCH {
            return Err(HeadlessError::configuration(format!(
                "package architecture {architecture} does not match host {}",
                std::env::consts::ARCH
            )));
        }
        for binary in ["rstorrent-headless", "rstorrent-gateway"] {
            let path = root.join("bin").join(binary);
            validate_regular_file(&path, true, "package executable")?;
            validate_elf_architecture(&path, &architecture)?;
        }
        #[cfg(not(test))]
        validate_binary_versions(root, &version)?;
        validate_regular_file(&root.join("install.sh"), true, "package installer")?;
        validate_regular_file(
            &root.join("resources/com.jstorrent.rstorrent.headless.service.in"),
            false,
            "package service template",
        )?;
        validate_regular_file(
            &root.join("resources/headless.toml.example"),
            false,
            "package config example",
        )?;
        if fs::read_to_string(root.join("resources/com.jstorrent.rstorrent.headless.service.in"))
            .map_err(|error| {
                HeadlessError::configuration(format!("read service template: {error}"))
            })?
            != SERVICE_TEMPLATE
            || fs::read_to_string(root.join("resources/headless.toml.example")).map_err(
                |error| HeadlessError::configuration(format!("read config example: {error}")),
            )? != CONFIG_EXAMPLE
        {
            return Err(HeadlessError::configuration(
                "package templates do not match the adapter build",
            ));
        }
        validate_web_tree(&root.join("web"))?;
        Ok(Self {
            root: fs::canonicalize(root).map_err(|error| {
                HeadlessError::configuration(format!("resolve package root: {error}"))
            })?,
            version,
            architecture,
        })
    }
}

pub trait ServiceManager {
    fn is_enabled(&mut self, unit: &str) -> Result<bool, HeadlessError>;
    fn is_active(&mut self, unit: &str) -> Result<bool, HeadlessError>;
    fn stop(&mut self, unit: &str) -> Result<(), HeadlessError>;
    fn start(&mut self, unit: &str) -> Result<(), HeadlessError>;
    fn enable(&mut self, unit: &str) -> Result<(), HeadlessError>;
    fn disable(&mut self, unit: &str) -> Result<(), HeadlessError>;
    fn daemon_reload(&mut self) -> Result<(), HeadlessError>;
}

pub struct SystemdUser;

impl ServiceManager for SystemdUser {
    fn is_enabled(&mut self, unit: &str) -> Result<bool, HeadlessError> {
        query_systemctl("is-enabled", unit)
    }

    fn is_active(&mut self, unit: &str) -> Result<bool, HeadlessError> {
        query_systemctl("is-active", unit)
    }

    fn stop(&mut self, unit: &str) -> Result<(), HeadlessError> {
        run_systemctl(&["stop", unit], "stop user service")
    }

    fn start(&mut self, unit: &str) -> Result<(), HeadlessError> {
        run_systemctl(&["start", unit], "start user service")
    }

    fn enable(&mut self, unit: &str) -> Result<(), HeadlessError> {
        run_systemctl(&["enable", unit], "enable user service")
    }

    fn disable(&mut self, unit: &str) -> Result<(), HeadlessError> {
        run_systemctl(&["disable", unit], "disable user service")
    }

    fn daemon_reload(&mut self) -> Result<(), HeadlessError> {
        run_systemctl(&["daemon-reload"], "reload user service manager")
    }
}

pub trait HealthVerifier {
    fn verify(&mut self, paths: &InstallPaths, version: &str) -> Result<(), HeadlessError>;
}

pub struct LocalHealthVerifier;

impl HealthVerifier for LocalHealthVerifier {
    fn verify(&mut self, paths: &InstallPaths, version: &str) -> Result<(), HeadlessError> {
        probe_health(&paths.config, version).map(|_| ())
    }
}

fn verify_health_ready(
    verifier: &mut dyn HealthVerifier,
    paths: &InstallPaths,
    version: &str,
) -> Result<(), HeadlessError> {
    let started = Instant::now();
    let mut last_error = None;
    for attempt in 0..HEALTH_READY_ATTEMPTS {
        match verifier.verify(paths, version) {
            Ok(()) => return Ok(()),
            Err(error) => last_error = Some(error),
        }
        if attempt + 1 < HEALTH_READY_ATTEMPTS {
            std::thread::sleep(HEALTH_READY_INTERVAL);
        }
    }
    let error = last_error.unwrap_or_else(|| HeadlessError::runtime("health probe did not run"));
    Err(HeadlessError::runtime(format!(
        "service did not become healthy within {}ms: {error}",
        started.elapsed().as_millis()
    )))
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InstallOutcome {
    pub version: String,
    pub restored_enabled: bool,
    pub restored_running: bool,
    pub config_example_created: bool,
}

pub fn install_bundle(bundle: &Path) -> Result<InstallOutcome, HeadlessError> {
    let paths = InstallPaths::system()?;
    let mut manager = SystemdUser;
    let mut verifier = LocalHealthVerifier;
    install_bundle_at(bundle, &paths, &mut manager, &mut verifier)
}

pub fn install_bundle_at(
    bundle: &Path,
    paths: &InstallPaths,
    manager: &mut dyn ServiceManager,
    verifier: &mut dyn HealthVerifier,
) -> Result<InstallOutcome, HeadlessError> {
    let bundle = BundleLayout::validate(bundle)?;
    let existing = fs::symlink_metadata(&paths.application_root).is_ok();
    if existing {
        verify_ownership(paths)?;
    } else {
        refuse_unowned_collision(&paths.command, "command link")?;
        refuse_unowned_collision(&paths.unit, "systemd unit")?;
    }
    let was_enabled = manager.is_enabled(SERVICE_NAME)?;
    let was_active = manager.is_active(SERVICE_NAME)?;

    ensure_install_directories(paths)?;
    let stage = paths
        .versions
        .join(format!(".stage-{}-{}", bundle.version, std::process::id()));
    remove_owned_stage_if_present(&stage, &paths.versions)?;
    copy_release(&bundle.root, &stage)?;
    validate_staged_release(&stage, &bundle)?;
    let snapshot = InstallSnapshot::capture(paths)?;
    let version_directory = paths.versions.join(&bundle.version);
    let replaced_same_version = fs::symlink_metadata(&version_directory).is_ok();
    if replaced_same_version {
        validate_directory(&version_directory, "installed version")?;
    }

    if was_active {
        manager.stop(SERVICE_NAME)?;
    }
    let backup = paths.versions.join(format!(
        ".rollback-{}-{}",
        bundle.version,
        std::process::id()
    ));
    remove_owned_stage_if_present(&backup, &paths.versions)?;
    if replaced_same_version && let Err(error) = fs::rename(&version_directory, &backup) {
        if was_active {
            let _ = manager.start(SERVICE_NAME);
        }
        return Err(HeadlessError::configuration(format!(
            "stage same-version rollback: {error}"
        )));
    }
    if let Err(error) = fs::rename(&stage, &version_directory) {
        if replaced_same_version {
            let _ = fs::rename(&backup, &version_directory);
        }
        if was_active {
            let _ = manager.start(SERVICE_NAME);
        }
        return Err(HeadlessError::configuration(format!(
            "install staged version: {error}"
        )));
    }

    let config_example_created = !paths.config_example.exists();
    let installation: Result<(), HeadlessError> = (|| {
        atomic_symlink(
            &PathBuf::from("versions").join(&bundle.version),
            &paths.current,
        )?;
        atomic_symlink(
            &paths.current.join("bin/rstorrent-headless"),
            &paths.command,
        )?;
        atomic_write(&paths.unit, render_service(paths)?.as_bytes(), 0o644)?;
        if config_example_created {
            atomic_write(
                &paths.config_example,
                render_config_example(paths)?.as_bytes(),
                0o600,
            )?;
        }
        atomic_write(
            &paths.ownership,
            ownership_manifest(paths)?.as_bytes(),
            0o600,
        )?;
        InstalledLayout::discover_from_executable(
            &version_directory.join("bin/rstorrent-headless"),
        )?;
        manager.daemon_reload()?;
        if was_enabled {
            manager.enable(SERVICE_NAME)?;
        }
        if was_active {
            manager.start(SERVICE_NAME)?;
            verify_health_ready(verifier, paths, &bundle.version)?;
        }
        Ok(())
    })();

    if let Err(error) = installation {
        let rollback = rollback_install(
            paths,
            manager,
            &snapshot,
            &version_directory,
            &backup,
            replaced_same_version,
            was_enabled,
            was_active,
            config_example_created,
        );
        return match rollback {
            Ok(()) => Err(HeadlessError::runtime(format!(
                "install failed and previous version was restored: {error}"
            ))),
            Err(rollback_error) => Err(HeadlessError::runtime(format!(
                "install failed: {error}; rollback also failed: {rollback_error}"
            ))),
        };
    }
    if replaced_same_version {
        remove_owned_directory(&backup, &paths.versions, ".rollback-")?;
    }
    Ok(InstallOutcome {
        version: bundle.version,
        restored_enabled: was_enabled,
        restored_running: was_active,
        config_example_created,
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StatusReport {
    pub version: String,
    pub enabled: bool,
    pub active: bool,
    pub healthy: bool,
}

pub fn status() -> Result<StatusReport, HeadlessError> {
    let paths = InstallPaths::system()?;
    let mut manager = SystemdUser;
    status_at(&paths, &mut manager)
}

pub fn status_at(
    paths: &InstallPaths,
    manager: &mut dyn ServiceManager,
) -> Result<StatusReport, HeadlessError> {
    verify_ownership(paths)?;
    let executable = fs::canonicalize(&paths.command).map_err(|error| {
        HeadlessError::configuration(format!("resolve installed command: {error}"))
    })?;
    let layout = InstalledLayout::discover_from_executable(&executable)?;
    let enabled = manager.is_enabled(SERVICE_NAME)?;
    let active = manager.is_active(SERVICE_NAME)?;
    let healthy = if active {
        probe_health(&paths.config, &layout.version)?;
        true
    } else {
        false
    };
    Ok(StatusReport {
        version: layout.version,
        enabled,
        active,
        healthy,
    })
}

pub fn uninstall() -> Result<(), HeadlessError> {
    let paths = InstallPaths::system()?;
    let mut manager = SystemdUser;
    uninstall_at(&paths, &mut manager)
}

pub fn uninstall_at(
    paths: &InstallPaths,
    manager: &mut dyn ServiceManager,
) -> Result<(), HeadlessError> {
    verify_ownership(paths)?;
    verify_owned_install_shape(paths)?;
    if manager.is_active(SERVICE_NAME)? {
        manager.stop(SERVICE_NAME)?;
    }
    if manager.is_enabled(SERVICE_NAME)? {
        manager.disable(SERVICE_NAME)?;
    }
    remove_exact_link(
        &paths.command,
        &paths.current.join("bin/rstorrent-headless"),
        "command link",
    )?;
    remove_managed_unit(&paths.unit)?;
    remove_exact_relative_current(&paths.current)?;
    if paths.versions.exists() {
        validate_directory(&paths.versions, "owned versions")?;
        fs::remove_dir_all(&paths.versions).map_err(|error| {
            HeadlessError::runtime(format!("remove installed versions: {error}"))
        })?;
    }
    remove_regular_file(&paths.ownership, "ownership manifest")?;
    if fs::read_dir(&paths.application_root)
        .map_err(|error| HeadlessError::runtime(format!("inspect application root: {error}")))?
        .next()
        .is_none()
    {
        fs::remove_dir(&paths.application_root).map_err(|error| {
            HeadlessError::runtime(format!("remove empty application root: {error}"))
        })?;
    }
    manager.daemon_reload()?;
    Ok(())
}

#[derive(Deserialize)]
struct HealthResponse {
    status: String,
    build_id: String,
    product: String,
}

fn probe_health(config_path: &Path, version: &str) -> Result<HealthResponse, HeadlessError> {
    let config = load(config_path)?;
    let _origin = url::Url::parse(&config.public_origin)
        .map_err(|_| HeadlessError::configuration("configured public origin is invalid"))?;
    let host = config
        .public_origin
        .split_once("://")
        .map(|(_, authority)| authority)
        .ok_or_else(|| HeadlessError::configuration("configured public origin has no authority"))?;
    let authorization = match &config.authentication {
        AuthenticationConfig::Basic { .. } => {
            let credentials = load_basic_credentials(&config)?;
            Some(format!(
                "Basic {}",
                BASE64_STANDARD.encode(format!(
                    "{}:{}",
                    credentials.username(),
                    credentials.password()
                ))
            ))
        }
        AuthenticationConfig::LocalBrowser => None,
    };
    let mut stream = TcpStream::connect_timeout(&config.listen, Duration::from_secs(2))
        .map_err(|error| HeadlessError::runtime(format!("connect service health: {error}")))?;
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .map_err(|error| HeadlessError::runtime(format!("set service health timeout: {error}")))?;
    let mut request = format!("GET /healthz HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n");
    if let Some(authorization) = authorization {
        request.push_str("Authorization: ");
        request.push_str(&authorization);
        request.push_str("\r\n");
    }
    request.push_str("\r\n");
    stream.write_all(request.as_bytes()).map_err(|error| {
        HeadlessError::runtime(format!("write service health request: {error}"))
    })?;
    let mut response = Vec::with_capacity(1024);
    stream
        .take(64 * 1024)
        .read_to_end(&mut response)
        .map_err(|error| {
            HeadlessError::runtime(format!("read service health response: {error}"))
        })?;
    let split = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .ok_or_else(|| HeadlessError::runtime("service health response has no header boundary"))?;
    let headers = std::str::from_utf8(&response[..split])
        .map_err(|_| HeadlessError::runtime("service health headers are not UTF-8"))?;
    if !headers.starts_with("HTTP/1.1 200 ") {
        return Err(HeadlessError::runtime(
            "service health did not return HTTP 200",
        ));
    }
    let health: HealthResponse = serde_json::from_slice(&response[split + 4..])
        .map_err(|error| HeadlessError::runtime(format!("decode service health: {error}")))?;
    if health.status != "ok" || health.product != PRODUCT_ID || health.build_id != version {
        return Err(HeadlessError::runtime(
            "service health returned the wrong product or build identity",
        ));
    }
    Ok(health)
}

#[allow(clippy::too_many_arguments)]
fn rollback_install(
    paths: &InstallPaths,
    manager: &mut dyn ServiceManager,
    snapshot: &InstallSnapshot,
    version_directory: &Path,
    backup: &Path,
    replaced_same_version: bool,
    was_enabled: bool,
    was_active: bool,
    config_example_created: bool,
) -> Result<(), HeadlessError> {
    let _ = manager.stop(SERVICE_NAME);
    if version_directory.exists() {
        remove_owned_directory(version_directory, &paths.versions, "")?;
    }
    if replaced_same_version {
        fs::rename(backup, version_directory).map_err(|error| {
            HeadlessError::runtime(format!("restore same-version directory: {error}"))
        })?;
    }
    snapshot.restore(paths)?;
    if config_example_created {
        remove_regular_file_if_present(&paths.config_example)?;
    }
    manager.daemon_reload()?;
    if was_enabled {
        manager.enable(SERVICE_NAME)?;
    }
    if was_active {
        manager.start(SERVICE_NAME)?;
    }
    Ok(())
}

struct InstallSnapshot {
    current: Option<PathBuf>,
    command: Option<PathBuf>,
    unit: Option<Vec<u8>>,
    ownership: Option<Vec<u8>>,
}

impl InstallSnapshot {
    fn capture(paths: &InstallPaths) -> Result<Self, HeadlessError> {
        Ok(Self {
            current: read_optional_link(&paths.current)?,
            command: read_optional_link(&paths.command)?,
            unit: read_optional_regular(&paths.unit)?,
            ownership: read_optional_regular(&paths.ownership)?,
        })
    }

    fn restore(&self, paths: &InstallPaths) -> Result<(), HeadlessError> {
        restore_link(&paths.current, self.current.as_deref())?;
        restore_link(&paths.command, self.command.as_deref())?;
        restore_file(&paths.unit, self.unit.as_deref(), 0o644)?;
        restore_file(&paths.ownership, self.ownership.as_deref(), 0o600)
    }
}

fn ensure_install_directories(paths: &InstallPaths) -> Result<(), HeadlessError> {
    ensure_real_directory(&paths.application_root, 0o700, "application root")?;
    ensure_real_directory(&paths.versions, 0o700, "versions root")?;
    ensure_real_directory(
        paths
            .command
            .parent()
            .ok_or_else(|| HeadlessError::configuration("command has no parent"))?,
        0o755,
        "command directory",
    )?;
    ensure_real_directory(
        paths
            .unit
            .parent()
            .ok_or_else(|| HeadlessError::configuration("unit has no parent"))?,
        0o755,
        "systemd user directory",
    )?;
    ensure_real_directory(
        paths
            .config_example
            .parent()
            .ok_or_else(|| HeadlessError::configuration("config example has no parent"))?,
        0o700,
        "headless config directory",
    )
}

fn copy_release(source: &Path, destination: &Path) -> Result<(), HeadlessError> {
    ensure_real_directory(destination, 0o755, "release stage")?;
    let mut pending = vec![(source.to_path_buf(), destination.to_path_buf())];
    while let Some((source_dir, destination_dir)) = pending.pop() {
        for entry in fs::read_dir(&source_dir).map_err(|error| {
            HeadlessError::configuration(format!("read package directory: {error}"))
        })? {
            let entry = entry.map_err(|error| {
                HeadlessError::configuration(format!("read package entry: {error}"))
            })?;
            let source_path = entry.path();
            let destination_path = destination_dir.join(entry.file_name());
            let metadata = fs::symlink_metadata(&source_path).map_err(|error| {
                HeadlessError::configuration(format!("inspect package entry: {error}"))
            })?;
            if metadata.file_type().is_symlink() {
                return Err(HeadlessError::configuration(
                    "package copy encountered a symbolic link",
                ));
            }
            if metadata.is_dir() {
                ensure_real_directory(&destination_path, 0o755, "release directory")?;
                pending.push((source_path, destination_path));
            } else if metadata.is_file() {
                let relative = source_path.strip_prefix(source).map_err(|_| {
                    HeadlessError::configuration("package entry escaped package root")
                })?;
                let executable =
                    relative == Path::new("install.sh") || relative.starts_with("bin/");
                copy_regular_file(
                    &source_path,
                    &destination_path,
                    if executable { 0o755 } else { 0o644 },
                )?;
            } else {
                return Err(HeadlessError::configuration(
                    "package copy encountered an unsupported file type",
                ));
            }
        }
    }
    Ok(())
}

fn validate_staged_release(stage: &Path, bundle: &BundleLayout) -> Result<(), HeadlessError> {
    if read_identity(&stage.join("VERSION"), "staged VERSION")? != bundle.version
        || read_identity(&stage.join("ARCH"), "staged architecture")? != bundle.architecture
        || read_identity(&stage.join("PACKAGE_ID"), "staged identity")? != PACKAGE_ID
    {
        return Err(HeadlessError::configuration(
            "staged release identity does not match the package",
        ));
    }
    validate_regular_file(
        &stage.join("bin/rstorrent-headless"),
        true,
        "staged headless executable",
    )?;
    validate_regular_file(
        &stage.join("bin/rstorrent-gateway"),
        true,
        "staged gateway executable",
    )?;
    validate_web_tree(&stage.join("web"))
}

fn render_service(paths: &InstallPaths) -> Result<String, HeadlessError> {
    Ok(SERVICE_TEMPLATE
        .replace(
            "@RSTORRENT_HEADLESS_COMMAND@",
            &systemd_escape_path(&paths.command)?,
        )
        .replace(
            "@RSTORRENT_HEADLESS_CONFIG@",
            &systemd_escape_path(&paths.config)?,
        ))
}

fn render_config_example(paths: &InstallPaths) -> Result<String, HeadlessError> {
    let profile_path = paths.profile_default();
    let profile = profile_path
        .to_str()
        .ok_or_else(|| HeadlessError::configuration("profile path is not valid UTF-8"))?;
    Ok(CONFIG_EXAMPLE.replace("@RSTORRENT_HEADLESS_PROFILE_ROOT@", &toml_escape(profile)))
}

fn ownership_manifest(paths: &InstallPaths) -> Result<String, HeadlessError> {
    let values = [
        ("application_root", &paths.application_root),
        ("command", &paths.command),
        ("unit", &paths.unit),
        ("config_preserved", &paths.config),
        ("config_example_preserved", &paths.config_example),
    ];
    let mut manifest = format!("{OWNERSHIP_HEADER}\npackage_id={PACKAGE_ID}\n");
    for (key, path) in values {
        let value = path
            .to_str()
            .ok_or_else(|| HeadlessError::configuration(format!("{key} is not valid UTF-8")))?;
        if value.contains(['\0', '\r', '\n']) {
            return Err(HeadlessError::configuration(format!(
                "{key} contains a line ending"
            )));
        }
        manifest.push_str(key);
        manifest.push('=');
        manifest.push_str(value);
        manifest.push('\n');
    }
    Ok(manifest)
}

fn verify_ownership(paths: &InstallPaths) -> Result<(), HeadlessError> {
    let actual = fs::read(&paths.ownership).map_err(|error| {
        HeadlessError::configuration(format!("read ownership manifest: {error}"))
    })?;
    if actual != ownership_manifest(paths)?.as_bytes() {
        return Err(HeadlessError::configuration(
            "ownership manifest mismatch; refusing mutation",
        ));
    }
    Ok(())
}

fn verify_owned_install_shape(paths: &InstallPaths) -> Result<(), HeadlessError> {
    validate_directory(&paths.application_root, "owned application root")?;
    let allowed = BTreeSet::from(["current", "ownership-v1", "profile", "versions"]);
    for entry in fs::read_dir(&paths.application_root).map_err(|error| {
        HeadlessError::configuration(format!("read owned application root: {error}"))
    })? {
        let entry = entry.map_err(|error| {
            HeadlessError::configuration(format!("read owned application entry: {error}"))
        })?;
        let name = entry
            .file_name()
            .to_str()
            .ok_or_else(|| HeadlessError::configuration("owned entry name is not UTF-8"))?
            .to_owned();
        if !allowed.contains(name.as_str()) {
            return Err(HeadlessError::configuration(format!(
                "unexpected application-root entry {name}; refusing removal"
            )));
        }
    }
    Ok(())
}

fn validate_base_path(path: &Path, label: &str) -> Result<(), HeadlessError> {
    let value = path
        .to_str()
        .ok_or_else(|| HeadlessError::configuration(format!("{label} path is not UTF-8")))?;
    if !path.is_absolute()
        || path == Path::new("/")
        || value.contains(['\0', '\r', '\n'])
        || path
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err(HeadlessError::configuration(format!(
            "unsafe {label} path {}",
            path.display()
        )));
    }
    Ok(())
}

fn require_exact_entries(root: &Path, expected: &[&str], label: &str) -> Result<(), HeadlessError> {
    let actual = fs::read_dir(root)
        .map_err(|error| HeadlessError::configuration(format!("read {label}: {error}")))?
        .map(|entry| {
            entry
                .map_err(|error| HeadlessError::configuration(format!("read {label}: {error}")))?
                .file_name()
                .into_string()
                .map_err(|_| HeadlessError::configuration(format!("{label} name is not UTF-8")))
        })
        .collect::<Result<BTreeSet<_>, _>>()?;
    let expected = expected
        .iter()
        .map(|value| (*value).to_owned())
        .collect::<BTreeSet<_>>();
    if actual != expected {
        return Err(HeadlessError::configuration(format!(
            "{label} entries do not match the package allowlist"
        )));
    }
    Ok(())
}

fn validate_elf_architecture(path: &Path, architecture: &str) -> Result<(), HeadlessError> {
    let bytes = fs::read(path)
        .map_err(|error| HeadlessError::configuration(format!("read ELF executable: {error}")))?;
    let expected_machine = match architecture {
        "x86_64" => 62u16,
        "aarch64" => 183u16,
        _ => return Err(HeadlessError::configuration("unsupported ELF architecture")),
    };
    if bytes.len() < 20
        || &bytes[..4] != b"\x7fELF"
        || bytes[4] != 2
        || bytes[5] != 1
        || u16::from_le_bytes([bytes[18], bytes[19]]) != expected_machine
    {
        return Err(HeadlessError::configuration(format!(
            "package executable {} is not a matching 64-bit little-endian ELF",
            path.display()
        )));
    }
    Ok(())
}

#[cfg(not(test))]
fn validate_binary_versions(root: &Path, version: &str) -> Result<(), HeadlessError> {
    for (binary, product) in [
        ("rstorrent-headless", "rstorrent-headless"),
        ("rstorrent-gateway", "rstorrent-gateway"),
    ] {
        let output = Command::new(root.join("bin").join(binary))
            .arg("--version")
            .output()
            .map_err(|error| {
                HeadlessError::configuration(format!("run package {binary} identity: {error}"))
            })?;
        if !output.status.success()
            || output.stdout != format!("{product} {version}\n").as_bytes()
            || !output.stderr.is_empty()
        {
            return Err(HeadlessError::configuration(format!(
                "package {binary} identity does not match VERSION"
            )));
        }
    }
    Ok(())
}

#[cfg(unix)]
fn ensure_real_directory(path: &Path, mode: u32, label: &str) -> Result<(), HeadlessError> {
    use std::os::unix::fs::PermissionsExt;

    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {}
        Ok(_) => {
            return Err(HeadlessError::configuration(format!(
                "{label} {} is not a real directory",
                path.display()
            )));
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir_all(path).map_err(|error| {
                HeadlessError::configuration(format!("create {label}: {error}"))
            })?;
        }
        Err(error) => {
            return Err(HeadlessError::configuration(format!(
                "inspect {label}: {error}"
            )));
        }
    }
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
        .map_err(|error| HeadlessError::configuration(format!("set {label} mode: {error}")))
}

#[cfg(not(unix))]
fn ensure_real_directory(_path: &Path, _mode: u32, _label: &str) -> Result<(), HeadlessError> {
    Err(HeadlessError::configuration(
        "the headless installer is supported only on Unix",
    ))
}

#[cfg(unix)]
fn copy_regular_file(source: &Path, destination: &Path, mode: u32) -> Result<(), HeadlessError> {
    use std::os::unix::fs::PermissionsExt;

    validate_regular_file(source, mode & 0o100 != 0, "package file")?;
    if let Some(parent) = destination.parent() {
        ensure_real_directory(parent, 0o755, "release parent")?;
    }
    fs::copy(source, destination)
        .map_err(|error| HeadlessError::configuration(format!("copy package file: {error}")))?;
    fs::set_permissions(destination, fs::Permissions::from_mode(mode))
        .map_err(|error| HeadlessError::configuration(format!("set package file mode: {error}")))
}

#[cfg(unix)]
fn atomic_write(path: &Path, contents: &[u8], mode: u32) -> Result<(), HeadlessError> {
    use std::os::unix::fs::PermissionsExt;

    let parent = path
        .parent()
        .ok_or_else(|| HeadlessError::configuration("owned file has no parent"))?;
    ensure_real_directory(parent, 0o755, "owned file parent")?;
    let temporary = parent.join(format!(".rstorrent-write-{}.tmp", std::process::id()));
    remove_regular_file_if_present(&temporary)?;
    fs::write(&temporary, contents)
        .map_err(|error| HeadlessError::runtime(format!("write owned file: {error}")))?;
    fs::set_permissions(&temporary, fs::Permissions::from_mode(mode))
        .map_err(|error| HeadlessError::runtime(format!("set owned file mode: {error}")))?;
    fs::rename(&temporary, path)
        .map_err(|error| HeadlessError::runtime(format!("replace owned file: {error}")))
}

#[cfg(unix)]
fn atomic_symlink(target: &Path, destination: &Path) -> Result<(), HeadlessError> {
    use std::os::unix::fs::symlink;

    let parent = destination
        .parent()
        .ok_or_else(|| HeadlessError::configuration("owned link has no parent"))?;
    ensure_real_directory(parent, 0o755, "owned link parent")?;
    let temporary = parent.join(format!(".rstorrent-link-{}.tmp", std::process::id()));
    remove_regular_file_if_present(&temporary)?;
    symlink(target, &temporary)
        .map_err(|error| HeadlessError::runtime(format!("create owned link: {error}")))?;
    fs::rename(&temporary, destination)
        .map_err(|error| HeadlessError::runtime(format!("replace owned link: {error}")))
}

#[cfg(not(unix))]
fn atomic_write(_path: &Path, _contents: &[u8], _mode: u32) -> Result<(), HeadlessError> {
    Err(HeadlessError::configuration(
        "headless install requires Unix",
    ))
}

#[cfg(not(unix))]
fn atomic_symlink(_target: &Path, _destination: &Path) -> Result<(), HeadlessError> {
    Err(HeadlessError::configuration(
        "headless install requires Unix",
    ))
}

fn systemd_escape_path(path: &Path) -> Result<String, HeadlessError> {
    let value = path
        .to_str()
        .ok_or_else(|| HeadlessError::configuration("systemd path is not UTF-8"))?;
    if !path.is_absolute() || value.contains(['\0', '\r', '\n']) {
        return Err(HeadlessError::configuration("unsafe systemd path"));
    }
    let mut escaped = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'.' | b'_' | b'-') {
            escaped.push(char::from(byte));
        } else {
            escaped.push_str(&format!("\\x{byte:02x}"));
        }
    }
    Ok(escaped)
}

fn toml_escape(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\t', "\\t")
}

fn query_systemctl(action: &str, unit: &str) -> Result<bool, HeadlessError> {
    let output = Command::new("systemctl")
        .args(["--user", action, unit])
        .output()
        .map_err(|error| HeadlessError::runtime(format!("run systemctl {action}: {error}")))?;
    if output.status.success() {
        return Ok(true);
    }
    if matches!(output.status.code(), Some(1 | 3 | 4)) {
        let state = String::from_utf8_lossy(&output.stdout);
        if matches!(
            state.trim(),
            "disabled" | "failed" | "inactive" | "indirect" | "not-found" | "static" | "unknown"
        ) {
            return Ok(false);
        }
    }
    Err(command_error(output, &format!("query service {action}")))
}

fn run_systemctl(arguments: &[&str], action: &str) -> Result<(), HeadlessError> {
    let output = Command::new("systemctl")
        .arg("--user")
        .args(arguments)
        .output()
        .map_err(|error| HeadlessError::runtime(format!("{action}: {error}")))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(command_error(output, action))
    }
}

fn command_error(output: Output, action: &str) -> HeadlessError {
    let detail = String::from_utf8_lossy(&output.stderr);
    let detail = detail.trim();
    if detail.is_empty() {
        HeadlessError::runtime(format!("{action} failed with {}", output.status))
    } else {
        HeadlessError::runtime(format!("{action} failed: {detail}"))
    }
}

fn refuse_unowned_collision(path: &Path, label: &str) -> Result<(), HeadlessError> {
    match fs::symlink_metadata(path) {
        Ok(_) => Err(HeadlessError::configuration(format!(
            "unowned {label} already exists at {}",
            path.display()
        ))),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(HeadlessError::configuration(format!(
            "inspect {label}: {error}"
        ))),
    }
}

fn remove_owned_stage_if_present(path: &Path, versions: &Path) -> Result<(), HeadlessError> {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("");
    if path.parent() != Some(versions)
        || !(name.starts_with(".stage-") || name.starts_with(".rollback-"))
    {
        return Err(HeadlessError::configuration("unsafe package staging path"));
    }
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {
            fs::remove_dir_all(path)
                .map_err(|error| HeadlessError::runtime(format!("clear package staging: {error}")))
        }
        Ok(_) => Err(HeadlessError::configuration(
            "package staging collision is not a real directory",
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(HeadlessError::configuration(format!(
            "inspect package staging: {error}"
        ))),
    }
}

fn remove_owned_directory(path: &Path, parent: &Path, prefix: &str) -> Result<(), HeadlessError> {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("");
    if path.parent() != Some(parent) || (!prefix.is_empty() && !name.starts_with(prefix)) {
        return Err(HeadlessError::configuration(
            "unsafe owned directory removal",
        ));
    }
    validate_directory(path, "owned removal directory")?;
    fs::remove_dir_all(path)
        .map_err(|error| HeadlessError::runtime(format!("remove owned directory: {error}")))
}

fn read_optional_link(path: &Path) -> Result<Option<PathBuf>, HeadlessError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => fs::read_link(path)
            .map(Some)
            .map_err(|error| HeadlessError::configuration(format!("read owned link: {error}"))),
        Ok(_) => Err(HeadlessError::configuration(
            "owned link path is not a symbolic link",
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(HeadlessError::configuration(format!(
            "inspect owned link: {error}"
        ))),
    }
}

fn read_optional_regular(path: &Path) -> Result<Option<Vec<u8>>, HeadlessError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => fs::read(path)
            .map(Some)
            .map_err(|error| HeadlessError::configuration(format!("read owned file: {error}"))),
        Ok(_) => Err(HeadlessError::configuration(
            "owned file path is not regular",
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(HeadlessError::configuration(format!(
            "inspect owned file: {error}"
        ))),
    }
}

fn restore_link(path: &Path, target: Option<&Path>) -> Result<(), HeadlessError> {
    match target {
        Some(target) => atomic_symlink(target, path),
        None => remove_regular_file_if_present(path),
    }
}

fn restore_file(path: &Path, contents: Option<&[u8]>, mode: u32) -> Result<(), HeadlessError> {
    match contents {
        Some(contents) => atomic_write(path, contents, mode),
        None => remove_regular_file_if_present(path),
    }
}

fn remove_regular_file_if_present(path: &Path) -> Result<(), HeadlessError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() || metadata.file_type().is_symlink() => {
            fs::remove_file(path)
                .map_err(|error| HeadlessError::runtime(format!("remove owned file: {error}")))
        }
        Ok(_) => Err(HeadlessError::configuration(
            "refusing to remove non-file owned path",
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(HeadlessError::configuration(format!(
            "inspect owned file: {error}"
        ))),
    }
}

fn remove_regular_file(path: &Path, label: &str) -> Result<(), HeadlessError> {
    validate_regular_file(path, false, label)?;
    fs::remove_file(path)
        .map_err(|error| HeadlessError::runtime(format!("remove {label}: {error}")))
}

fn remove_exact_link(path: &Path, expected: &Path, label: &str) -> Result<(), HeadlessError> {
    let target = read_optional_link(path)?
        .ok_or_else(|| HeadlessError::configuration(format!("{label} is missing")))?;
    if target != expected {
        return Err(HeadlessError::configuration(format!(
            "{label} target mismatch; refusing removal"
        )));
    }
    fs::remove_file(path)
        .map_err(|error| HeadlessError::runtime(format!("remove {label}: {error}")))
}

fn remove_exact_relative_current(path: &Path) -> Result<(), HeadlessError> {
    let target = read_optional_link(path)?
        .ok_or_else(|| HeadlessError::configuration("current link is missing"))?;
    if target.is_absolute()
        || target.components().count() != 2
        || target
            .components()
            .next()
            .and_then(|component| match component {
                std::path::Component::Normal(value) => value.to_str(),
                _ => None,
            })
            != Some("versions")
    {
        return Err(HeadlessError::configuration(
            "current link is not an owned relative version target",
        ));
    }
    fs::remove_file(path)
        .map_err(|error| HeadlessError::runtime(format!("remove current link: {error}")))
}

fn remove_managed_unit(path: &Path) -> Result<(), HeadlessError> {
    let contents = fs::read_to_string(path)
        .map_err(|error| HeadlessError::configuration(format!("read managed unit: {error}")))?;
    if !contents.starts_with("# Managed by rstorrent-headless.") {
        return Err(HeadlessError::configuration(
            "systemd unit is not marked as headless-owned",
        ));
    }
    fs::remove_file(path)
        .map_err(|error| HeadlessError::runtime(format!("remove managed unit: {error}")))
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::{HealthVerifier, InstallPaths, ServiceManager, install_bundle_at, uninstall_at};
    use crate::runtime::HeadlessError;
    use crate::{PACKAGE_ID, SERVICE_NAME};

    static NEXT_TEST: AtomicU64 = AtomicU64::new(0);

    #[derive(Default)]
    struct FakeManager {
        enabled: bool,
        active: bool,
        calls: Vec<String>,
    }

    impl ServiceManager for FakeManager {
        fn is_enabled(&mut self, unit: &str) -> Result<bool, HeadlessError> {
            assert_eq!(unit, SERVICE_NAME);
            self.calls.push("is-enabled".to_owned());
            Ok(self.enabled)
        }

        fn is_active(&mut self, unit: &str) -> Result<bool, HeadlessError> {
            assert_eq!(unit, SERVICE_NAME);
            self.calls.push("is-active".to_owned());
            Ok(self.active)
        }

        fn stop(&mut self, unit: &str) -> Result<(), HeadlessError> {
            assert_eq!(unit, SERVICE_NAME);
            self.calls.push("stop".to_owned());
            self.active = false;
            Ok(())
        }

        fn start(&mut self, unit: &str) -> Result<(), HeadlessError> {
            assert_eq!(unit, SERVICE_NAME);
            self.calls.push("start".to_owned());
            self.active = true;
            Ok(())
        }

        fn enable(&mut self, unit: &str) -> Result<(), HeadlessError> {
            assert_eq!(unit, SERVICE_NAME);
            self.calls.push("enable".to_owned());
            self.enabled = true;
            Ok(())
        }

        fn disable(&mut self, unit: &str) -> Result<(), HeadlessError> {
            assert_eq!(unit, SERVICE_NAME);
            self.calls.push("disable".to_owned());
            self.enabled = false;
            Ok(())
        }

        fn daemon_reload(&mut self) -> Result<(), HeadlessError> {
            self.calls.push("daemon-reload".to_owned());
            Ok(())
        }
    }

    struct FakeHealth {
        fail_attempts: usize,
        versions: Vec<String>,
    }

    impl HealthVerifier for FakeHealth {
        fn verify(&mut self, _paths: &InstallPaths, version: &str) -> Result<(), HeadlessError> {
            self.versions.push(version.to_owned());
            if self.fail_attempts > 0 {
                if self.fail_attempts != usize::MAX {
                    self.fail_attempts -= 1;
                }
                Err(HeadlessError::runtime("injected health failure"))
            } else {
                Ok(())
            }
        }
    }

    fn test_root(label: &str) -> PathBuf {
        let sequence = NEXT_TEST.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "rstorrent-headless-installer-{label}-{}-{sequence}",
            std::process::id()
        ))
    }

    #[cfg(unix)]
    fn write_file(path: &Path, bytes: &[u8], mode: u32) {
        use std::os::unix::fs::PermissionsExt;

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create fixture parent");
        }
        fs::write(path, bytes).expect("write fixture file");
        fs::set_permissions(path, fs::Permissions::from_mode(mode)).expect("set fixture mode");
    }

    #[cfg(unix)]
    fn bundle_fixture(root: &Path, version: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;

        let bundle = root.join(format!("bundle-{version}"));
        let mut elf = vec![0u8; 64];
        elf[..6].copy_from_slice(b"\x7fELF\x02\x01");
        let machine = if std::env::consts::ARCH == "x86_64" {
            62u16
        } else {
            183u16
        };
        elf[18..20].copy_from_slice(&machine.to_le_bytes());
        write_file(&bundle.join("bin/rstorrent-headless"), &elf, 0o755);
        write_file(&bundle.join("bin/rstorrent-gateway"), &elf, 0o755);
        write_file(&bundle.join("web/index.html"), b"index", 0o644);
        write_file(&bundle.join("web/assets/app.js"), b"app", 0o644);
        write_file(
            &bundle.join("resources/com.jstorrent.rstorrent.headless.service.in"),
            super::SERVICE_TEMPLATE.as_bytes(),
            0o644,
        );
        write_file(
            &bundle.join("resources/headless.toml.example"),
            super::CONFIG_EXAMPLE.as_bytes(),
            0o644,
        );
        write_file(&bundle.join("install.sh"), b"#!/bin/sh\n", 0o755);
        write_file(&bundle.join("VERSION"), version.as_bytes(), 0o644);
        write_file(&bundle.join("PACKAGE_ID"), PACKAGE_ID.as_bytes(), 0o644);
        write_file(
            &bundle.join("ARCH"),
            std::env::consts::ARCH.as_bytes(),
            0o644,
        );
        for directory in [
            bundle.clone(),
            bundle.join("bin"),
            bundle.join("web"),
            bundle.join("web/assets"),
            bundle.join("resources"),
        ] {
            fs::set_permissions(&directory, fs::Permissions::from_mode(0o755))
                .expect("set fixture directory mode");
        }
        bundle
    }

    fn install_paths(root: &Path) -> InstallPaths {
        let home = root.join("home");
        fs::create_dir_all(&home).expect("create home");
        InstallPaths::for_roots(&home, &home.join(".local/share"), &home.join(".config"))
            .expect("install paths")
    }

    #[cfg(unix)]
    #[test]
    fn fresh_install_is_disabled_and_writes_owned_templates() {
        let root = test_root("fresh");
        let paths = install_paths(&root);
        let bundle = bundle_fixture(&root, "1.0.0");
        let mut manager = FakeManager::default();
        let mut health = FakeHealth {
            fail_attempts: 0,
            versions: Vec::new(),
        };
        let outcome =
            install_bundle_at(&bundle, &paths, &mut manager, &mut health).expect("fresh install");

        assert_eq!(outcome.version, "1.0.0");
        assert!(!outcome.restored_enabled);
        assert!(!outcome.restored_running);
        assert!(outcome.config_example_created);
        assert_eq!(
            fs::read_link(&paths.current).expect("current link"),
            Path::new("versions/1.0.0")
        );
        assert!(!paths.config.exists());
        assert!(paths.config_example.is_file());
        let unit = fs::read_to_string(&paths.unit).expect("read unit");
        assert!(unit.contains("[Install]\nWantedBy=default.target"));
        assert!(unit.contains("RestartPreventExitStatus=78"));
        assert!(unit.contains("TimeoutStopSec=45s"));
        assert!(
            !manager
                .calls
                .iter()
                .any(|call| call == "enable" || call == "start")
        );
        assert!(health.versions.is_empty());

        fs::remove_dir_all(root).expect("remove fixture");
    }

    #[cfg(unix)]
    #[test]
    fn running_update_rolls_back_identity_and_service_after_failed_health() {
        let root = test_root("rollback");
        let paths = install_paths(&root);
        let first = bundle_fixture(&root, "1.0.0");
        let second = bundle_fixture(&root, "2.0.0");
        let mut manager = FakeManager::default();
        let mut health = FakeHealth {
            fail_attempts: 0,
            versions: Vec::new(),
        };
        install_bundle_at(&first, &paths, &mut manager, &mut health).expect("first install");
        manager.enabled = true;
        manager.active = true;
        health.fail_attempts = usize::MAX;

        let error = install_bundle_at(&second, &paths, &mut manager, &mut health)
            .expect_err("health failure must roll back");
        assert!(error.to_string().contains("previous version was restored"));
        assert_eq!(
            fs::read_link(&paths.current).expect("restored current"),
            Path::new("versions/1.0.0")
        );
        assert!(paths.versions.join("1.0.0").is_dir());
        assert!(!paths.versions.join("2.0.0").exists());
        assert!(manager.enabled);
        assert!(manager.active);
        assert_eq!(health.versions, ["2.0.0", "2.0.0", "2.0.0"]);

        fs::remove_dir_all(root).expect("remove fixture");
    }

    #[cfg(unix)]
    #[test]
    fn repair_restores_running_state_and_uninstall_preserves_operator_data() {
        let root = test_root("repair-uninstall");
        let paths = install_paths(&root);
        let bundle = bundle_fixture(&root, "1.0.0");
        let mut manager = FakeManager::default();
        let mut health = FakeHealth {
            fail_attempts: 0,
            versions: Vec::new(),
        };
        install_bundle_at(&bundle, &paths, &mut manager, &mut health).expect("first install");
        fs::create_dir_all(paths.profile_default()).expect("create profile");
        fs::write(paths.profile_default().join("state"), b"profile").expect("write profile");
        fs::write(&paths.config, b"operator config").expect("write config");
        let payload = root.join("payload");
        fs::create_dir_all(&payload).expect("create payload");
        fs::write(payload.join("keep"), b"payload").expect("write payload");
        manager.enabled = true;
        manager.active = true;
        health.fail_attempts = 1;

        let outcome = install_bundle_at(&bundle, &paths, &mut manager, &mut health)
            .expect("same-version repair");
        assert!(outcome.restored_enabled);
        assert!(outcome.restored_running);
        assert!(manager.active);
        assert_eq!(health.versions, ["1.0.0", "1.0.0"]);

        uninstall_at(&paths, &mut manager).expect("preserving uninstall");
        assert!(!paths.command.exists());
        assert!(!paths.unit.exists());
        assert!(!paths.versions.exists());
        assert_eq!(
            fs::read(paths.profile_default().join("state")).expect("preserved profile"),
            b"profile"
        );
        assert_eq!(
            fs::read(&paths.config).expect("preserved config"),
            b"operator config"
        );
        assert_eq!(
            fs::read(payload.join("keep")).expect("preserved payload"),
            b"payload"
        );
        assert!(paths.config_example.exists());
        assert!(!manager.enabled);
        assert!(!manager.active);

        fs::remove_dir_all(root).expect("remove fixture");
    }
}
