use std::process::ExitCode;
use std::time::Duration;

use rstorrent_engine::NetworkPolicy;
use rstorrent_engine::dht::{DhtConfig, DhtService};

const DEFAULT_QUERY_COUNT: u64 = 1;
const MAX_QUERY_COUNT: u64 = 32;
const WAIT_TIMEOUT: Duration = Duration::from_secs(10);

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    let expected_queries = match parse_query_count() {
        Ok(count) => count,
        Err(error) => {
            eprintln!("argument error: {error}");
            return ExitCode::from(2);
        }
    };
    let mut config = DhtConfig::for_network(NetworkPolicy::LoopbackOnly);
    config.bootstrap_nodes.clear();
    let service = match DhtService::start(config).await {
        Ok(service) => service,
        Err(error) => {
            eprintln!("DHT start failed: {error}");
            return ExitCode::FAILURE;
        }
    };
    println!("address={}", service.local_address());
    let handle = service.handle();
    let observed = tokio::time::timeout(WAIT_TIMEOUT, async {
        loop {
            let stats = handle.stats().await?;
            if stats.queries_received >= expected_queries {
                return Ok::<_, rstorrent_engine::dht::DhtError>(stats.queries_received);
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await;
    let query_count = match observed {
        Ok(Ok(count)) => count,
        Ok(Err(error)) => {
            eprintln!("DHT stats failed: {error}");
            return ExitCode::FAILURE;
        }
        Err(_) => {
            eprintln!("DHT query wait timed out after {}s", WAIT_TIMEOUT.as_secs());
            return ExitCode::FAILURE;
        }
    };
    match service.shutdown().await {
        Ok(snapshot) => {
            println!(
                "queries_received={query_count} saved_nodes={}",
                snapshot.nodes_v4.len()
            );
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("DHT shutdown failed: {error}");
            ExitCode::FAILURE
        }
    }
}

fn parse_query_count() -> Result<u64, String> {
    let mut arguments = std::env::args().skip(1);
    let Some(flag) = arguments.next() else {
        return Ok(DEFAULT_QUERY_COUNT);
    };
    if flag != "--queries" {
        return Err(format!("unknown argument {flag}"));
    }
    let count = arguments
        .next()
        .ok_or_else(|| "--queries requires a value".to_owned())?
        .parse::<u64>()
        .map_err(|_| "--queries must be an integer".to_owned())?;
    if arguments.next().is_some() || !(1..=MAX_QUERY_COUNT).contains(&count) {
        return Err(format!(
            "--queries must be the only option and between 1 and {MAX_QUERY_COUNT}"
        ));
    }
    Ok(count)
}
