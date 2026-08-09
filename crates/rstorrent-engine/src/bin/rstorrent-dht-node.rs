use std::process::ExitCode;
use std::time::Duration;

use rstorrent_engine::dht::{DhtConfig, DhtService};
use rstorrent_engine::{AddressFamily, NetworkPolicy, SessionUdpService};
use tokio::net::UdpSocket;

const DEFAULT_QUERY_COUNT: u64 = 1;
const MAX_QUERY_COUNT: u64 = 32;
const WAIT_TIMEOUT: Duration = Duration::from_secs(10);

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    let arguments = match Arguments::parse() {
        Ok(arguments) => arguments,
        Err(error) => {
            eprintln!("argument error: {error}");
            return ExitCode::from(2);
        }
    };
    let mut config = DhtConfig::for_network(NetworkPolicy::LoopbackOnly);
    config.bootstrap_nodes.clear();
    let mut udp_owner = None;
    let mut query_address = None;
    let service = match arguments.family {
        FamilyMode::Ipv4 => DhtService::start(config).await,
        FamilyMode::Ipv6 | FamilyMode::Dual => {
            let initial_address = match arguments.family {
                FamilyMode::Ipv6 => "[::1]:0",
                FamilyMode::Dual => "127.0.0.1:0",
                FamilyMode::Ipv4 => unreachable!(),
            };
            let socket = match UdpSocket::bind(initial_address).await {
                Ok(socket) => socket,
                Err(error) => {
                    eprintln!("initial UDP bind failed: {error}");
                    return ExitCode::FAILURE;
                }
            };
            let (mut udp, transport) = match SessionUdpService::start(socket) {
                Ok(owner) => owner,
                Err(error) => {
                    eprintln!("UDP start failed: {error}");
                    return ExitCode::FAILURE;
                }
            };
            if arguments.family == FamilyMode::Dual {
                let ipv6 = match UdpSocket::bind("[::1]:0").await {
                    Ok(socket) => socket,
                    Err(error) => {
                        eprintln!("IPv6 UDP bind failed: {error}");
                        let _ = udp.shutdown().await;
                        return ExitCode::FAILURE;
                    }
                };
                if let Err(error) = udp.replace_socket(ipv6).await {
                    eprintln!("IPv6 UDP start failed: {error}");
                    let _ = udp.shutdown().await;
                    return ExitCode::FAILURE;
                }
            }
            query_address = udp.local_address_for(AddressFamily::Ipv6);
            match DhtService::start_with_transport(config, transport).await {
                Ok(service) => {
                    udp_owner = Some(udp);
                    Ok(service)
                }
                Err(error) => {
                    let _ = udp.shutdown().await;
                    Err(error)
                }
            }
        }
    };
    let service = match service {
        Ok(service) => service,
        Err(error) => {
            eprintln!("DHT start failed: {error}");
            return ExitCode::FAILURE;
        }
    };
    let query_address = query_address.unwrap_or_else(|| service.local_address());
    let ipv4_address = udp_owner
        .as_ref()
        .and_then(|udp| udp.local_address_for(AddressFamily::Ipv4))
        .or_else(|| (arguments.family == FamilyMode::Ipv4).then_some(service.local_address()));
    let ipv6_address = udp_owner
        .as_ref()
        .and_then(|udp| udp.local_address_for(AddressFamily::Ipv6));
    println!(
        "address={query_address} family={} address_ipv4={} address_ipv6={}",
        arguments.family,
        ipv4_address.map_or_else(|| "none".to_owned(), |address| address.to_string()),
        ipv6_address.map_or_else(|| "none".to_owned(), |address| address.to_string())
    );
    let handle = service.handle();
    let observed = tokio::time::timeout(WAIT_TIMEOUT, async {
        loop {
            let stats = handle.stats().await?;
            if stats.queries_received >= arguments.expected_queries {
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
    let result = match service.shutdown().await {
        Ok(snapshot) => {
            println!(
                "queries_received={query_count} saved_nodes_v4={} saved_nodes_v6={}",
                snapshot.nodes_v4.len(),
                snapshot.nodes_v6.len()
            );
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("DHT shutdown failed: {error}");
            ExitCode::FAILURE
        }
    };
    if let Some(udp) = udp_owner
        && let Err(error) = udp.shutdown().await
    {
        eprintln!("UDP shutdown failed: {error}");
        return ExitCode::FAILURE;
    }
    result
}

#[derive(Clone, Copy, Debug)]
struct Arguments {
    expected_queries: u64,
    family: FamilyMode,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FamilyMode {
    Ipv4,
    Ipv6,
    Dual,
}

impl std::fmt::Display for FamilyMode {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Ipv4 => "ipv4",
            Self::Ipv6 => "ipv6",
            Self::Dual => "dual",
        })
    }
}

impl Arguments {
    fn parse() -> Result<Self, String> {
        let mut expected_queries = DEFAULT_QUERY_COUNT;
        let mut family = FamilyMode::Ipv4;
        let mut query_count_seen = false;
        let mut family_seen = false;
        let mut arguments = std::env::args().skip(1);
        while let Some(flag) = arguments.next() {
            let value = arguments
                .next()
                .ok_or_else(|| format!("{flag} requires a value"))?;
            match flag.as_str() {
                "--queries" if !query_count_seen => {
                    expected_queries = value
                        .parse::<u64>()
                        .map_err(|_| "--queries must be an integer".to_owned())?;
                    if !(1..=MAX_QUERY_COUNT).contains(&expected_queries) {
                        return Err(format!("--queries must be between 1 and {MAX_QUERY_COUNT}"));
                    }
                    query_count_seen = true;
                }
                "--family" if !family_seen => {
                    family = match value.as_str() {
                        "ipv4" => FamilyMode::Ipv4,
                        "ipv6" => FamilyMode::Ipv6,
                        "dual" => FamilyMode::Dual,
                        _ => return Err("--family must be ipv4, ipv6, or dual".to_owned()),
                    };
                    family_seen = true;
                }
                "--queries" | "--family" => {
                    return Err(format!("{flag} may appear only once"));
                }
                _ => return Err(format!("unknown argument {flag}")),
            }
        }
        Ok(Self {
            expected_queries,
            family,
        })
    }
}
