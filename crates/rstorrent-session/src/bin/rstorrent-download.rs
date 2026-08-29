#![forbid(unsafe_code)]

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let exit = rstorrent_session::foreground_download::run(std::env::args_os().skip(1)).await;
    std::process::exit(exit);
}
