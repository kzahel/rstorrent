use std::fs;
use std::path::{Path, PathBuf};

const ALLOWED_DIRECT_DEPENDENCIES: &[&str] = &["crypto-bigint", "sha1", "sha2"];
const FORBIDDEN_SOURCE_FRAGMENTS: &[&str] = &[
    "rstorrent_engine",
    "std::fs",
    "std::net",
    "std::path",
    "std::process",
    "std::sync",
    "std::task",
    "std::sync::mpsc",
    "std::thread",
    "std::time",
    "tokio",
];

#[test]
fn protocol_crate_keeps_runtime_dependencies_outward() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    check_manifest_dependencies(&manifest_dir.join("Cargo.toml"));
    check_source_tree(&manifest_dir.join("src"));
}

fn check_manifest_dependencies(manifest_path: &Path) {
    let manifest = fs::read_to_string(manifest_path).expect("read protocol Cargo.toml");
    let mut dependency_section = false;

    for raw_line in manifest.lines() {
        let line = raw_line.split('#').next().unwrap_or_default().trim();
        if line.starts_with('[') {
            dependency_section = line == "[dependencies]"
                || line == "[dev-dependencies]"
                || line.starts_with("[target.") && line.ends_with(".dependencies]");
            continue;
        }
        if !dependency_section || line.is_empty() {
            continue;
        }

        let dependency = line
            .split_once('=')
            .map(|(name, _)| name.trim())
            .expect("dependency declarations must contain '='");
        assert!(
            ALLOWED_DIRECT_DEPENDENCIES.contains(&dependency),
            "protocol crate has disallowed direct dependency {dependency:?}"
        );
    }
}

fn check_source_tree(source_dir: &Path) {
    let mut pending = vec![source_dir.to_path_buf()];
    while let Some(path) = pending.pop() {
        for entry in fs::read_dir(&path).expect("read protocol source directory") {
            let entry = entry.expect("read protocol source entry");
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
                continue;
            }
            if path.extension().and_then(|extension| extension.to_str()) != Some("rs") {
                continue;
            }

            let source = fs::read_to_string(&path).expect("read protocol source");
            for forbidden in FORBIDDEN_SOURCE_FRAGMENTS {
                assert!(
                    !source.contains(forbidden),
                    "{} contains forbidden runtime fragment {forbidden:?}",
                    path.display()
                );
            }
        }
    }
}
