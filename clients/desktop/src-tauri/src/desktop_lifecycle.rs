use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU8, Ordering};

use serde::{Deserialize, Serialize};

const SETTINGS_FILE_NAME: &str = "desktop-shell.json";
const SETTINGS_VERSION: u32 = 2;
const LEGACY_SETTINGS_VERSION: u32 = 1;
const MAX_SETTINGS_BYTES: usize = 4 * 1024;

const SHUTDOWN_RUNNING: u8 = 0;
const SHUTDOWN_STOPPING: u8 = 1;
const SHUTDOWN_FINAL_EXIT: u8 = 2;
const SHUTDOWN_FAILED: u8 = 3;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DesktopShellSettings {
    version: u32,
    pub(crate) run_in_background: bool,
    pub(crate) notifications: DesktopNotificationSettings,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DesktopNotificationSettings {
    pub(crate) notify_download_complete: bool,
    pub(crate) notify_needs_attention: bool,
    pub(crate) notify_while_focused: bool,
}

impl Default for DesktopNotificationSettings {
    fn default() -> Self {
        Self {
            notify_download_complete: true,
            notify_needs_attention: true,
            notify_while_focused: true,
        }
    }
}

impl Default for DesktopShellSettings {
    fn default() -> Self {
        Self {
            version: SETTINGS_VERSION,
            run_in_background: true,
            notifications: DesktopNotificationSettings::default(),
        }
    }
}

#[derive(Deserialize)]
struct SettingsVersion {
    version: u32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyDesktopShellSettings {
    #[serde(rename = "version")]
    _version: u32,
    run_in_background: bool,
}

enum ReadDesktopShellSettings {
    Current(DesktopShellSettings),
    Migrated(DesktopShellSettings),
}

pub(crate) struct LoadedDesktopShellSettings {
    pub(crate) path: PathBuf,
    pub(crate) settings: DesktopShellSettings,
    pub(crate) diagnostic: Option<String>,
}

pub(crate) fn load_desktop_shell_settings(config_dir: &Path) -> LoadedDesktopShellSettings {
    let path = config_dir.join(SETTINGS_FILE_NAME);
    match read_desktop_shell_settings(&path) {
        Ok(Some(ReadDesktopShellSettings::Current(settings))) => LoadedDesktopShellSettings {
            path,
            settings,
            diagnostic: None,
        },
        Ok(Some(ReadDesktopShellSettings::Migrated(settings))) => {
            let diagnostic = match write_desktop_shell_settings(&path, settings) {
                Ok(()) => "desktop shell version 1 settings were migrated to version 2".to_owned(),
                Err(error) => format!(
                    "desktop shell version 1 settings are active but could not be migrated: {error}"
                ),
            };
            LoadedDesktopShellSettings {
                path,
                settings,
                diagnostic: Some(diagnostic),
            }
        }
        Ok(None) => repair_with_defaults(path, "settings were missing"),
        Err(error) => repair_with_defaults(path, &error),
    }
}

pub(crate) fn persist_run_in_background(
    path: &Path,
    current: DesktopShellSettings,
    run_in_background: bool,
) -> Result<DesktopShellSettings, String> {
    let next = DesktopShellSettings {
        run_in_background,
        ..current
    };
    write_desktop_shell_settings(path, next)?;
    Ok(next)
}

pub(crate) fn persist_notification_settings(
    path: &Path,
    current: DesktopShellSettings,
    notifications: DesktopNotificationSettings,
) -> Result<DesktopShellSettings, String> {
    let next = DesktopShellSettings {
        notifications,
        ..current
    };
    write_desktop_shell_settings(path, next)?;
    Ok(next)
}

fn repair_with_defaults(path: PathBuf, reason: &str) -> LoadedDesktopShellSettings {
    let settings = DesktopShellSettings::default();
    let diagnostic = match write_desktop_shell_settings(&path, settings) {
        Ok(()) => format!("desktop shell {reason}; defaults were restored"),
        Err(error) => format!(
            "desktop shell {reason}; defaults are active but could not be persisted: {error}"
        ),
    };
    LoadedDesktopShellSettings {
        path,
        settings,
        diagnostic: Some(diagnostic),
    }
}

fn read_desktop_shell_settings(path: &Path) -> Result<Option<ReadDesktopShellSettings>, String> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("settings could not be opened: {error}")),
    };
    let mut bytes = Vec::new();
    file.take((MAX_SETTINGS_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("settings could not be read: {error}"))?;
    if bytes.len() > MAX_SETTINGS_BYTES {
        return Err("settings exceeded the size limit".to_owned());
    }
    let version: SettingsVersion =
        serde_json::from_slice(&bytes).map_err(|_| "settings were malformed".to_owned())?;
    match version.version {
        SETTINGS_VERSION => {
            let settings: DesktopShellSettings =
                serde_json::from_slice(&bytes).map_err(|_| "settings were malformed".to_owned())?;
            Ok(Some(ReadDesktopShellSettings::Current(settings)))
        }
        LEGACY_SETTINGS_VERSION => {
            let legacy: LegacyDesktopShellSettings =
                serde_json::from_slice(&bytes).map_err(|_| "settings were malformed".to_owned())?;
            Ok(Some(ReadDesktopShellSettings::Migrated(
                DesktopShellSettings {
                    version: SETTINGS_VERSION,
                    run_in_background: legacy.run_in_background,
                    notifications: DesktopNotificationSettings::default(),
                },
            )))
        }
        _ => Err("settings used an unsupported version".to_owned()),
    }
}

fn write_desktop_shell_settings(path: &Path, settings: DesktopShellSettings) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "settings path had no parent directory".to_owned())?;
    std::fs::create_dir_all(parent)
        .map_err(|error| format!("create settings directory: {error}"))?;
    let bytes = serde_json::to_vec_pretty(&settings)
        .map_err(|error| format!("serialize settings: {error}"))?;
    if bytes.len() + 1 > MAX_SETTINGS_BYTES {
        return Err("serialized settings exceeded the size limit".to_owned());
    }
    let mut temporary = tempfile::NamedTempFile::new_in(parent)
        .map_err(|error| format!("create temporary settings: {error}"))?;
    temporary
        .write_all(&bytes)
        .and_then(|()| temporary.write_all(b"\n"))
        .and_then(|()| temporary.as_file().sync_all())
        .map_err(|error| format!("write temporary settings: {error}"))?;
    temporary
        .persist(path)
        .map_err(|error| format!("replace settings atomically: {}", error.error))?;
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ShutdownPhase {
    Running,
    Stopping,
    FinalExit,
    Failed,
}

pub(crate) struct ShutdownGate {
    phase: AtomicU8,
}

impl ShutdownGate {
    pub(crate) const fn new() -> Self {
        Self {
            phase: AtomicU8::new(SHUTDOWN_RUNNING),
        }
    }

    pub(crate) fn phase(&self) -> ShutdownPhase {
        match self.phase.load(Ordering::Acquire) {
            SHUTDOWN_RUNNING => ShutdownPhase::Running,
            SHUTDOWN_STOPPING => ShutdownPhase::Stopping,
            SHUTDOWN_FINAL_EXIT => ShutdownPhase::FinalExit,
            SHUTDOWN_FAILED => ShutdownPhase::Failed,
            _ => unreachable!("shutdown state is internal"),
        }
    }

    pub(crate) fn try_start(&self) -> bool {
        self.phase
            .compare_exchange(
                SHUTDOWN_RUNNING,
                SHUTDOWN_STOPPING,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_ok()
    }

    pub(crate) fn complete(&self) {
        self.phase.store(SHUTDOWN_FINAL_EXIT, Ordering::Release);
    }

    pub(crate) fn fail(&self) {
        self.phase.store(SHUTDOWN_FAILED, Ordering::Release);
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CloseAction {
    Allow,
    Hide,
    StartShutdown,
    Prevent,
}

pub(crate) fn close_action(shutdown_phase: ShutdownPhase, run_in_background: bool) -> CloseAction {
    match shutdown_phase {
        ShutdownPhase::FinalExit => CloseAction::Allow,
        ShutdownPhase::Running if run_in_background => CloseAction::Hide,
        ShutdownPhase::Running => CloseAction::StartShutdown,
        ShutdownPhase::Stopping | ShutdownPhase::Failed => CloseAction::Prevent,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CloseAction, DesktopNotificationSettings, DesktopShellSettings, ShutdownGate,
        ShutdownPhase, close_action, load_desktop_shell_settings, persist_notification_settings,
        persist_run_in_background,
    };

    #[test]
    fn shell_settings_default_on_persist_and_reopen() {
        let temporary = tempfile::tempdir().expect("temporary settings directory");
        let loaded = load_desktop_shell_settings(temporary.path());
        assert!(loaded.settings.run_in_background);
        assert_eq!(
            loaded.settings.notifications,
            DesktopNotificationSettings::default()
        );
        assert!(loaded.path.is_file());
        assert!(loaded.diagnostic.is_some());

        let disabled = persist_run_in_background(&loaded.path, loaded.settings, false)
            .expect("persist disabled background setting");
        assert!(!disabled.run_in_background);
        let reopened = load_desktop_shell_settings(temporary.path());
        assert_eq!(reopened.settings, disabled);
        assert!(reopened.diagnostic.is_none());
    }

    #[test]
    fn version_one_migrates_background_choice_and_notification_defaults() {
        let temporary = tempfile::tempdir().expect("temporary settings directory");
        let path = temporary.path().join("desktop-shell.json");
        std::fs::write(&path, br#"{"version":1,"run_in_background":false}"#)
            .expect("write version one settings");

        let migrated = load_desktop_shell_settings(temporary.path());
        assert!(!migrated.settings.run_in_background);
        assert_eq!(
            migrated.settings.notifications,
            DesktopNotificationSettings::default()
        );
        assert!(migrated.diagnostic.is_some());

        let reopened = load_desktop_shell_settings(temporary.path());
        assert_eq!(reopened.settings, migrated.settings);
        assert!(reopened.diagnostic.is_none());
    }

    #[test]
    fn notification_settings_persist_as_one_atomic_shell_record() {
        let temporary = tempfile::tempdir().expect("temporary settings directory");
        let loaded = load_desktop_shell_settings(temporary.path());
        let notifications = DesktopNotificationSettings {
            notify_download_complete: false,
            notify_needs_attention: true,
            notify_while_focused: false,
        };
        let changed = persist_notification_settings(&loaded.path, loaded.settings, notifications)
            .expect("persist notification settings");
        assert!(changed.run_in_background);
        assert_eq!(changed.notifications, notifications);
        assert_eq!(
            load_desktop_shell_settings(temporary.path()).settings,
            changed
        );
    }

    #[test]
    fn malformed_oversized_and_unknown_settings_repair_to_default() {
        for (index, contents) in [
            b"{".to_vec(),
            br#"{"version":3,"run_in_background":false}"#.to_vec(),
            br#"{"version":1,"run_in_background":false,"extra":1}"#.to_vec(),
            br#"{"version":2,"run_in_background":false,"notifications":{"notify_download_complete":true,"notify_needs_attention":true}}"#.to_vec(),
            br#"{"version":2,"run_in_background":false,"notifications":{"notify_download_complete":true,"notify_needs_attention":true,"notify_while_focused":true},"extra":1}"#.to_vec(),
            vec![b'x'; 4 * 1024 + 1],
        ]
        .into_iter()
        .enumerate()
        {
            let temporary = tempfile::tempdir().expect("temporary settings directory");
            let path = temporary.path().join("desktop-shell.json");
            std::fs::write(&path, contents).expect("write invalid settings");
            let repaired = load_desktop_shell_settings(temporary.path());
            assert!(repaired.settings.run_in_background, "case {index}");
            assert!(repaired.diagnostic.is_some(), "case {index}");
            let reopened = load_desktop_shell_settings(temporary.path());
            assert_eq!(reopened.settings, DesktopShellSettings::default());
            assert!(reopened.diagnostic.is_none());
        }
    }

    #[test]
    fn persistence_failure_does_not_return_changed_live_state() {
        let temporary = tempfile::tempdir().expect("temporary settings directory");
        let path = temporary.path().join("desktop-shell.json");
        std::fs::create_dir(&path).expect("create directory at settings path");
        let current = DesktopShellSettings::default();
        assert!(persist_run_in_background(&path, current, false).is_err());
        assert!(
            persist_notification_settings(
                &path,
                current,
                DesktopNotificationSettings {
                    notify_download_complete: false,
                    ..DesktopNotificationSettings::default()
                },
            )
            .is_err()
        );
        assert!(current.run_in_background);
        assert_eq!(
            current.notifications,
            DesktopNotificationSettings::default()
        );
        let loaded = load_desktop_shell_settings(temporary.path());
        assert!(loaded.settings.run_in_background);
        assert!(loaded.diagnostic.is_some());
    }

    #[test]
    fn close_policy_and_shutdown_admission_are_closed() {
        assert_eq!(
            close_action(ShutdownPhase::Running, true),
            CloseAction::Hide
        );
        assert_eq!(
            close_action(ShutdownPhase::Running, false),
            CloseAction::StartShutdown
        );
        assert_eq!(
            close_action(ShutdownPhase::Stopping, true),
            CloseAction::Prevent
        );
        assert_eq!(
            close_action(ShutdownPhase::Failed, false),
            CloseAction::Prevent
        );
        assert_eq!(
            close_action(ShutdownPhase::FinalExit, true),
            CloseAction::Allow
        );

        let gate = ShutdownGate::new();
        assert!(gate.try_start());
        assert!(!gate.try_start());
        assert_eq!(gate.phase(), ShutdownPhase::Stopping);
        gate.complete();
        assert_eq!(gate.phase(), ShutdownPhase::FinalExit);
        assert!(!gate.try_start());

        let failed = ShutdownGate::new();
        assert!(failed.try_start());
        failed.fail();
        assert_eq!(failed.phase(), ShutdownPhase::Failed);
        assert!(!failed.try_start());
    }
}
