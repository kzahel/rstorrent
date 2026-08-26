use std::io::Write;
use std::process::{Command, Stdio};

use rstorrent_native_host::decode_frames;

const ORIGIN: &str = "chrome-extension://dbokmlpefliilbjldladbimlcfgbolhk/";

fn framed(json: &str) -> Vec<u8> {
    let mut bytes = (json.len() as u32).to_ne_bytes().to_vec();
    bytes.extend_from_slice(json.as_bytes());
    bytes
}

fn spawn_host() -> std::process::Child {
    Command::new(env!("CARGO_BIN_EXE_rstorrent-native-host"))
        .arg(ORIGIN)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn native host")
}

#[test]
fn process_writes_one_protocol_frame_and_exits_cleanly_on_eof() {
    let mut child = spawn_host();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(&framed(
            r#"{"id":"process-hello","protocolVersion":1,"op":"hello"}"#,
        ))
        .unwrap();

    let output = child.wait_with_output().expect("wait for native host");
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let frames = decode_frames(&output.stdout).expect("decode native host stdout");
    assert_eq!(frames.len(), 1);
    assert_eq!(frames[0]["id"], "process-hello");
    assert_eq!(frames[0]["result"]["kind"], "hello");
}

#[test]
fn malformed_process_input_never_contaminates_stdout() {
    let mut child = spawn_host();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(&framed("not-json"))
        .unwrap();

    let output = child.wait_with_output().expect("wait for native host");
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(output.stderr.starts_with(b"RSTorrent native host:"));
    assert!(output.stderr.len() < 1024);
}
