use std::env;
use std::path::PathBuf;
use std::process::ExitCode;

use rstorrent_headless::runtime::{ErrorClass, InstalledLayout, run_installed_service};
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
    let config_path = parse_config_path(env::args().skip(1))?;
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

fn parse_config_path(
    mut arguments: impl Iterator<Item = String>,
) -> Result<PathBuf, rstorrent_headless::runtime::HeadlessError> {
    let default = default_config_path()?;
    let Some(argument) = arguments.next() else {
        return Ok(default);
    };
    if argument != "--config" {
        return Err(configuration_error(
            "usage: rstorrent-headless [--config ABSOLUTE_PATH]",
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
