use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use rstorrent_native_host::{HOST_NAME, LAUNCH_CONFIG_FILENAME, LaunchConfig, MAX_FRAME_BYTES};
use serde::Serialize;
use sha2::{Digest, Sha256};

const PRODUCTION_EXTENSION_ORIGIN: &str = "chrome-extension://dbokmlpefliilbjldladbimlcfgbolhk/";
const HOST_DESCRIPTION: &str = "RSTorrent desktop bootstrap";
const HOST_MANIFEST_FILENAME: &str = "com.jstorrent.rstorrent.native.json";
const HOST_DIRECTORY: &str = "native-host";
const MAX_HOST_BINARY_BYTES: u64 = 32 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Platform {
    MacOS,
    Linux,
    Windows,
}

#[derive(Debug, PartialEq, Eq)]
pub struct RegistrationReport {
    pub stable_host: PathBuf,
    pub browser_manifests: usize,
}

#[derive(Debug, Serialize)]
struct NativeHostManifest<'a> {
    name: &'static str,
    description: &'static str,
    path: &'a Path,
    #[serde(rename = "type")]
    transport: &'static str,
    allowed_origins: [&'static str; 1],
}

pub fn repair_native_host_registration(
    app_config_dir: &Path,
    home_dir: &Path,
    appimage: Option<&Path>,
) -> Result<RegistrationReport, String> {
    let desktop_executable =
        std::env::current_exe().map_err(|error| format!("resolve desktop executable: {error}"))?;
    let bundled_host = desktop_executable
        .parent()
        .ok_or_else(|| "desktop executable has no parent directory".to_owned())?
        .join(host_executable_filename());
    let desktop_launch_target = appimage.unwrap_or(&desktop_executable);
    install_for_platform(
        runtime_platform(),
        app_config_dir,
        home_dir,
        desktop_launch_target,
        &bundled_host,
    )
}

fn install_for_platform(
    platform: Platform,
    app_config_dir: &Path,
    home_dir: &Path,
    desktop_executable: &Path,
    bundled_host: &Path,
) -> Result<RegistrationReport, String> {
    if !desktop_executable.is_absolute() || !bundled_host.is_absolute() {
        return Err("desktop and native host paths must be absolute".to_owned());
    }
    let stable_directory = app_config_dir.join(HOST_DIRECTORY);
    fs::create_dir_all(&stable_directory)
        .map_err(|error| format!("create native host directory: {error}"))?;
    let stable_host = install_versioned_host(bundled_host, &stable_directory)?;

    let launch_config = launch_config(platform, desktop_executable)?;
    let launch_bytes = serde_json::to_vec_pretty(&launch_config)
        .map_err(|error| format!("encode native host launch config: {error}"))?;
    atomic_write(
        &stable_directory.join(LAUNCH_CONFIG_FILENAME),
        &launch_bytes,
    )?;

    let manifest_bytes = manifest_bytes(&stable_host)?;
    let stable_manifest = stable_directory.join(HOST_MANIFEST_FILENAME);
    atomic_write(&stable_manifest, &manifest_bytes)?;

    let browser_manifests = match platform {
        Platform::MacOS | Platform::Linux => {
            install_browser_manifests(platform, home_dir, &manifest_bytes)?
        }
        Platform::Windows => {
            register_windows_manifest(&stable_manifest)?;
            1
        }
    };
    remove_stale_hosts(&stable_directory, &stable_host);

    Ok(RegistrationReport {
        stable_host,
        browser_manifests,
    })
}

fn host_executable_filename() -> &'static str {
    if cfg!(target_os = "windows") {
        "rstorrent-native-host.exe"
    } else {
        "rstorrent-native-host"
    }
}

fn runtime_platform() -> Platform {
    if cfg!(target_os = "macos") {
        Platform::MacOS
    } else if cfg!(target_os = "windows") {
        Platform::Windows
    } else {
        Platform::Linux
    }
}

fn manifest_bytes(host_path: &Path) -> Result<Vec<u8>, String> {
    if !host_path.is_absolute() {
        return Err("native host manifest path must be absolute".to_owned());
    }
    let manifest = NativeHostManifest {
        name: HOST_NAME,
        description: HOST_DESCRIPTION,
        path: host_path,
        transport: "stdio",
        allowed_origins: [PRODUCTION_EXTENSION_ORIGIN],
    };
    let bytes = serde_json::to_vec_pretty(&manifest)
        .map_err(|error| format!("encode native host manifest: {error}"))?;
    if bytes.len() > MAX_FRAME_BYTES {
        return Err("native host manifest exceeds 64 KiB".to_owned());
    }
    Ok(bytes)
}

fn launch_config(platform: Platform, desktop_executable: &Path) -> Result<LaunchConfig, String> {
    if platform != Platform::MacOS {
        return Ok(LaunchConfig::executable(desktop_executable.to_owned()));
    }
    let application = desktop_executable
        .ancestors()
        .find(|candidate| {
            candidate
                .extension()
                .is_some_and(|extension| extension == "app")
        })
        .ok_or_else(|| {
            "packaged macOS desktop executable is not inside an app bundle".to_owned()
        })?;
    Ok(LaunchConfig::mac_app(application.to_owned()))
}

fn install_versioned_host(source: &Path, directory: &Path) -> Result<PathBuf, String> {
    let metadata = fs::metadata(source)
        .map_err(|error| format!("read packaged native host metadata: {error}"))?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_HOST_BINARY_BYTES {
        return Err(
            "packaged native host must be a nonempty file no larger than 32 MiB".to_owned(),
        );
    }
    let digest = sha256_file(source)?;
    let suffix = if cfg!(target_os = "windows") {
        ".exe"
    } else {
        ""
    };
    let destination = directory.join(format!(
        "rstorrent-native-host-v{}-{}{suffix}",
        env!("CARGO_PKG_VERSION"),
        &digest[..16]
    ));
    if destination.is_file() {
        return Ok(destination);
    }

    let mut input = File::open(source)
        .map_err(|error| format!("open packaged native host for copy: {error}"))?;
    let mut temporary = tempfile::NamedTempFile::new_in(directory)
        .map_err(|error| format!("create temporary native host: {error}"))?;
    std::io::copy(&mut input, &mut temporary)
        .map_err(|error| format!("copy native host: {error}"))?;
    temporary
        .as_file()
        .set_permissions(metadata.permissions())
        .map_err(|error| format!("set native host permissions: {error}"))?;
    temporary
        .as_file()
        .sync_all()
        .map_err(|error| format!("sync native host: {error}"))?;
    match temporary.persist_noclobber(&destination) {
        Ok(_) => Ok(destination),
        Err(_error) if destination.is_file() => Ok(destination),
        Err(error) => Err(format!("install native host: {}", error.error)),
    }
}

fn sha256_file(path: &Path) -> Result<String, String> {
    let mut file = File::open(path).map_err(|error| format!("open native host: {error}"))?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| format!("hash native host: {error}"))?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "native host file has no parent directory".to_owned())?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("create native host manifest directory: {error}"))?;
    if fs::read(path).ok().as_deref() == Some(bytes) {
        return Ok(());
    }
    let mut temporary = tempfile::NamedTempFile::new_in(parent)
        .map_err(|error| format!("create temporary native host file: {error}"))?;
    temporary
        .write_all(bytes)
        .map_err(|error| format!("write temporary native host file: {error}"))?;
    temporary
        .as_file()
        .sync_all()
        .map_err(|error| format!("sync temporary native host file: {error}"))?;
    temporary
        .persist(path)
        .map_err(|error| format!("replace native host file atomically: {}", error.error))?;
    Ok(())
}

fn install_browser_manifests(
    platform: Platform,
    home_dir: &Path,
    manifest: &[u8],
) -> Result<usize, String> {
    let mut installed = 0;
    for profile_root in browser_profile_roots(platform, home_dir) {
        if !profile_root.is_dir() {
            continue;
        }
        atomic_write(
            &profile_root
                .join("NativeMessagingHosts")
                .join(HOST_MANIFEST_FILENAME),
            manifest,
        )?;
        installed += 1;
    }
    Ok(installed)
}

fn browser_profile_roots(platform: Platform, home_dir: &Path) -> Vec<PathBuf> {
    match platform {
        Platform::MacOS => {
            let application_support = home_dir.join("Library/Application Support");
            vec![
                application_support.join("Google/Chrome"),
                application_support.join("Google/ChromeForTesting"),
                application_support.join("Chromium"),
            ]
        }
        Platform::Linux => {
            let config = home_dir.join(".config");
            vec![
                config.join("google-chrome"),
                config.join("google-chrome-for-testing"),
                config.join("chromium"),
            ]
        }
        Platform::Windows => Vec::new(),
    }
}

fn remove_stale_hosts(directory: &Path, current: &Path) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path == current {
            continue;
        }
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if name.starts_with("rstorrent-native-host-v")
            && entry.file_type().is_ok_and(|kind| kind.is_file())
        {
            let _ = fs::remove_file(path);
        }
    }
}

#[cfg(target_os = "windows")]
fn register_windows_manifest(manifest: &Path) -> Result<(), String> {
    use winreg::RegKey;
    use winreg::enums::HKEY_CURRENT_USER;

    let current_user = RegKey::predef(HKEY_CURRENT_USER);
    for key_path in [
        format!(r"Software\Google\Chrome\NativeMessagingHosts\{HOST_NAME}"),
        format!(r"Software\Chromium\NativeMessagingHosts\{HOST_NAME}"),
    ] {
        let (key, _) = current_user
            .create_subkey(&key_path)
            .map_err(|error| format!("create native host registry key: {error}"))?;
        key.set_value("", &manifest.as_os_str())
            .map_err(|error| format!("set native host registry manifest: {error}"))?;
    }
    Ok(())
}

#[cfg(not(target_os = "windows"))]
fn register_windows_manifest(_manifest: &Path) -> Result<(), String> {
    Err("Windows native host registration is unavailable on this platform".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn absolute(root: &Path, relative: &str) -> PathBuf {
        root.join(relative)
    }

    #[test]
    fn manifest_is_exact_and_never_takes_over_the_legacy_host() {
        let directory = tempfile::tempdir().unwrap();
        let host = absolute(directory.path(), "rstorrent-native-host");
        let value: serde_json::Value =
            serde_json::from_slice(&manifest_bytes(&host).unwrap()).unwrap();

        assert_eq!(value["name"], "com.jstorrent.rstorrent.native");
        assert_ne!(value["name"], "com.jstorrent.native");
        assert_eq!(value["type"], "stdio");
        assert_eq!(
            value["allowed_origins"],
            serde_json::json!([PRODUCTION_EXTENSION_ORIGIN])
        );
        assert_eq!(value["path"], host.to_string_lossy().as_ref());
    }

    #[test]
    fn browser_roots_are_platform_specific_and_do_not_include_edge() {
        let home = Path::new("/home/tester");
        let mac = browser_profile_roots(Platform::MacOS, home);
        assert!(mac.contains(&home.join("Library/Application Support/Google/Chrome")));
        assert!(mac.contains(&home.join("Library/Application Support/Google/ChromeForTesting")));
        assert!(
            !mac.iter()
                .any(|path| path.to_string_lossy().contains("Edge"))
        );

        let linux = browser_profile_roots(Platform::Linux, home);
        assert!(linux.contains(&home.join(".config/google-chrome")));
        assert!(linux.contains(&home.join(".config/google-chrome-for-testing")));
        assert!(
            !linux
                .iter()
                .any(|path| path.to_string_lossy().contains("edge"))
        );
    }

    #[test]
    fn registration_only_writes_browsers_with_existing_profile_roots() {
        let directory = tempfile::tempdir().unwrap();
        let home = directory.path().join("home");
        let config = directory.path().join("config");
        let desktop = absolute(directory.path(), "RSTorrent");
        let bundled_host = absolute(directory.path(), "rstorrent-native-host");
        fs::create_dir_all(home.join(".config/google-chrome")).unwrap();
        fs::write(&desktop, b"desktop").unwrap();
        fs::write(&bundled_host, b"native-host").unwrap();

        let report =
            install_for_platform(Platform::Linux, &config, &home, &desktop, &bundled_host).unwrap();

        assert_eq!(report.browser_manifests, 1);
        assert!(report.stable_host.is_file());
        assert!(
            home.join(".config/google-chrome/NativeMessagingHosts")
                .join(HOST_MANIFEST_FILENAME)
                .is_file()
        );
        assert!(!home.join(".config/chromium/NativeMessagingHosts").exists());
        let launch: LaunchConfig = serde_json::from_slice(
            &fs::read(config.join(HOST_DIRECTORY).join(LAUNCH_CONFIG_FILENAME)).unwrap(),
        )
        .unwrap();
        assert_eq!(launch, LaunchConfig::executable(desktop));
    }

    #[test]
    fn mac_launch_targets_the_app_bundle_not_its_inner_executable() {
        let desktop = Path::new("/Applications/RSTorrent.app/Contents/MacOS/rstorrent-desktop");
        assert_eq!(
            launch_config(Platform::MacOS, desktop).unwrap(),
            LaunchConfig::mac_app(PathBuf::from("/Applications/RSTorrent.app"))
        );
    }

    #[test]
    fn stable_host_is_content_versioned_and_repairs_manifest_path() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("source-host");
        let stable = directory.path().join("stable");
        fs::create_dir(&stable).unwrap();
        fs::write(&source, b"first host").unwrap();
        let first = install_versioned_host(&source, &stable).unwrap();
        assert_eq!(fs::read(&first).unwrap(), b"first host");

        fs::write(&source, b"second host").unwrap();
        let second = install_versioned_host(&source, &stable).unwrap();
        assert_ne!(first, second);
        assert_eq!(fs::read(&second).unwrap(), b"second host");
    }
}
