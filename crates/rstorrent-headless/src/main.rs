use std::env;
use std::path::Path;
use std::path::PathBuf;
use std::process::ExitCode;

use rstorrent_headless::runtime::{ErrorClass, InstalledLayout, run_installed_service};
use rstorrent_headless::updater::UpdateClient;
use rstorrent_headless::{SERVICE_NAME, installer};
use tokio_util::sync::CancellationToken;

const CONFIGURATION_EXIT: u8 = 78;
const RUNTIME_EXIT: u8 = 1;

#[tokio::main]
async fn main() -> ExitCode {
    match execute().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("rstorrent-headless: {error}");
            ExitCode::from(match error.class() {
                ErrorClass::Configuration => CONFIGURATION_EXIT,
                ErrorClass::Runtime => RUNTIME_EXIT,
            })
        }
    }
}

async fn execute() -> Result<(), rstorrent_headless::runtime::HeadlessError> {
    let arguments = env::args().skip(1).collect::<Vec<_>>();
    match arguments.as_slice() {
        [argument] if argument == "--version" => {
            println!("rstorrent-headless {}", env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
        [command, flag, bundle] if command == "install" && flag == "--bundle" => {
            return install(Path::new(bundle));
        }
        [command, flag, bundle] if command == "validate-package" && flag == "--bundle" => {
            let bundle = installer::BundleLayout::validate(Path::new(bundle))?;
            println!(
                "validated headless package version={} architecture={}",
                bundle.version, bundle.architecture
            );
            return Ok(());
        }
        [command] if command == "status" => return print_status(),
        [command, flag] if command == "update" && flag == "--check" => {
            return update(false).await;
        }
        [command, flag] if command == "update" && flag == "--apply" => {
            return update(true).await;
        }
        [command] if command == "uninstall" => return uninstall(),
        _ => {}
    }
    let config_path = parse_config_path(arguments.into_iter())?;
    let layout = InstalledLayout::discover()?;
    let shutdown = CancellationToken::new();
    let signal_shutdown = shutdown.clone();
    tokio::spawn(async move {
        wait_for_shutdown_signal().await;
        signal_shutdown.cancel();
    });
    let report = run_installed_service(&config_path, &layout, shutdown).await?;
    eprintln!(
        "headless stopped version={} listen={} shutdown_millis={}",
        report.version,
        report.listen,
        report.shutdown_elapsed.as_millis()
    );
    Ok(())
}

async fn update(apply: bool) -> Result<(), rstorrent_headless::runtime::HeadlessError> {
    let layout = InstalledLayout::discover()?;
    let client = UpdateClient::production()?;
    let Some(candidate) = client.check(&layout.version).await? else {
        println!("RSTorrent Headless {} is up to date.", layout.version);
        return Ok(());
    };
    if !apply {
        println!(
            "RSTorrent Headless {} is available (current {}).",
            candidate.version(),
            layout.version
        );
        println!("Release: {}", candidate.release_url());
        println!("Apply: $HOME/.local/bin/rstorrent-headless update --apply");
        return Ok(());
    }
    println!(
        "Downloading and verifying RSTorrent Headless {}...",
        candidate.version()
    );
    let outcome = client.apply(&candidate).await?;
    println!(
        "Installed RSTorrent Headless {} with health-checked service recovery.",
        outcome.version
    );
    Ok(())
}

fn install(bundle: &Path) -> Result<(), rstorrent_headless::runtime::HeadlessError> {
    if !bundle.is_absolute() {
        return Err(configuration_error(
            "install --bundle requires an absolute path",
        ));
    }
    let outcome = installer::install_bundle(bundle)?;
    let paths = installer::InstallPaths::system()?;
    println!("Installed RSTorrent Headless {}.", outcome.version);
    if outcome.config_example_created {
        println!(
            "Created protected configuration example: {}",
            paths.config_example.display()
        );
    }
    if outcome.restored_running {
        println!("The previously running user service passed authenticated health.");
    } else {
        println!("Installation did not start or enable the user service.");
        println!("1. Copy and edit the protected example:");
        println!(
            "   cp {} {}",
            paths.config_example.display(),
            paths.config.display()
        );
        println!("   chmod 600 {}", paths.config.display());
        println!("2. Enable and start explicitly:");
        println!("   systemctl --user enable --now {SERVICE_NAME}");
    }
    println!("Status: {} status", paths.command.display());
    println!("Logs: journalctl --user -u {SERVICE_NAME}");
    Ok(())
}

fn print_status() -> Result<(), rstorrent_headless::runtime::HeadlessError> {
    let report = installer::status()?;
    println!(
        "product=rstorrent-headless version={} access_mode={} enabled={} active={} healthy={}",
        report.version, report.access_mode, report.enabled, report.active, report.healthy
    );
    Ok(())
}

fn uninstall() -> Result<(), rstorrent_headless::runtime::HeadlessError> {
    let paths = installer::InstallPaths::system()?;
    installer::uninstall()?;
    println!("Removed RSTorrent Headless application files and user service.");
    println!("Preserved configuration: {}", paths.config.display());
    println!(
        "Preserved configuration example: {}",
        paths.config_example.display()
    );
    println!("Profiles and every configured payload root were preserved.");
    Ok(())
}

fn parse_config_path(
    mut arguments: impl Iterator<Item = String>,
) -> Result<PathBuf, rstorrent_headless::runtime::HeadlessError> {
    let default = default_config_path()?;
    let Some(argument) = arguments.next() else {
        return Ok(default);
    };
    if argument != "--config" {
        return Err(configuration_error(
            "usage: rstorrent-headless [--config ABSOLUTE_PATH] | status | update --check | update --apply | uninstall",
        ));
    }
    let path = arguments
        .next()
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| configuration_error("--config requires an absolute path"))?;
    if arguments.next().is_some() || !path.is_absolute() {
        return Err(configuration_error(
            "--config accepts exactly one absolute path",
        ));
    }
    Ok(path)
}

fn default_config_path() -> Result<PathBuf, rstorrent_headless::runtime::HeadlessError> {
    let base = match env::var_os("XDG_CONFIG_HOME") {
        Some(value) => PathBuf::from(value),
        None => PathBuf::from(
            env::var_os("HOME")
                .ok_or_else(|| configuration_error("HOME is required for the default config"))?,
        )
        .join(".config"),
    };
    if !base.is_absolute() {
        return Err(configuration_error(
            "XDG_CONFIG_HOME or HOME must resolve to an absolute path",
        ));
    }
    Ok(base.join("rstorrent/headless.toml"))
}

fn configuration_error(message: impl Into<String>) -> rstorrent_headless::runtime::HeadlessError {
    rstorrent_headless::runtime::HeadlessError::from(
        rstorrent_headless::config::ConfigError::Invalid(message.into()),
    )
}

async fn wait_for_shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};

        if let Ok(mut terminate) = signal(SignalKind::terminate()) {
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {}
                _ = terminate.recv() => {}
            }
            return;
        }
    }
    let _ = tokio::signal::ctrl_c().await;
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::parse_config_path;

    #[test]
    fn explicit_config_path_is_absolute_and_singular() {
        assert_eq!(
            parse_config_path(["--config".to_owned(), "/tmp/headless.toml".to_owned()].into_iter())
                .expect("explicit config"),
            PathBuf::from("/tmp/headless.toml")
        );
        assert!(
            parse_config_path(["--config".to_owned(), "relative".to_owned()].into_iter()).is_err()
        );
        assert!(parse_config_path(["--unknown".to_owned()].into_iter()).is_err());
    }
}
