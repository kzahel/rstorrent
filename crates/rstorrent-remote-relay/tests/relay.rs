use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use rstorrent_remote_relay::{
    PAIRED_CONTROL, PREAUTH_MESSAGE_BYTES, ProofRelay, ProofRelayServer, encode_client_select,
    encode_host_claim,
};
use tokio::time::timeout;
use tokio_tungstenite::{WebSocketStream, connect_async, tungstenite::Message};

type Client = WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

#[tokio::test]
async fn pairs_and_forwards_opaque_binary_messages() {
    let (relay, task, host_url, client_url) = start_relay().await;
    let mut host = connect(&host_url).await;
    host.send(Message::Binary(
        encode_host_claim("alice", &[7; 32]).unwrap().into(),
    ))
    .await
    .unwrap();
    let mut client = connect(&client_url).await;
    client
        .send(Message::Binary(
            encode_client_select("alice").unwrap().into(),
        ))
        .await
        .unwrap();
    paired(&mut host).await;
    paired(&mut client).await;

    client
        .send(Message::Binary(vec![1, 2, 3].into()))
        .await
        .unwrap();
    assert_eq!(binary(&mut host).await, vec![1, 2, 3]);
    host.send(Message::Binary(vec![4, 5].into())).await.unwrap();
    assert_eq!(binary(&mut client).await, vec![4, 5]);

    client.close(None).await.unwrap();
    wait_for(|| relay.metrics().active_circuits == 0).await;
    let metrics = relay.metrics();
    assert_eq!(metrics.registered_routes, 1);
    assert_eq!(metrics.waiting_hosts_high_water, 1);
    assert_eq!(metrics.active_circuits_high_water, 1);
    assert_eq!(metrics.active_pumps_high_water, 2);
    assert_eq!(metrics.forwarded_client_bytes, 3);
    assert_eq!(metrics.forwarded_host_bytes, 2);
    assert_eq!(metrics.completed_circuits, 1);
    shutdown(relay, task).await;
}

#[tokio::test]
async fn wrong_credential_cannot_replace_waiting_host() {
    let (relay, task, host_url, client_url) = start_relay().await;
    let mut original = connect(&host_url).await;
    original
        .send(Message::Binary(
            encode_host_claim("alice", &[7; 32]).unwrap().into(),
        ))
        .await
        .unwrap();
    let mut attacker = connect(&host_url).await;
    attacker
        .send(Message::Binary(
            encode_host_claim("alice", &[8; 32]).unwrap().into(),
        ))
        .await
        .unwrap();
    assert_generic_close(&mut attacker, 4_003, "claim rejected").await;

    let mut client = connect(&client_url).await;
    client
        .send(Message::Binary(
            encode_client_select("alice").unwrap().into(),
        ))
        .await
        .unwrap();
    paired(&mut original).await;
    paired(&mut client).await;
    assert_eq!(relay.metrics().rejected_host_claims, 1);
    client.close(None).await.unwrap();
    wait_for(|| relay.metrics().active_circuits == 0).await;
    shutdown(relay, task).await;
}

#[tokio::test]
async fn same_credential_replacement_is_generation_fenced() {
    let (relay, task, host_url, client_url) = start_relay().await;
    let mut old = connect(&host_url).await;
    old.send(Message::Binary(
        encode_host_claim("alice", &[7; 32]).unwrap().into(),
    ))
    .await
    .unwrap();
    let mut replacement = connect(&host_url).await;
    replacement
        .send(Message::Binary(
            encode_host_claim("alice", &[7; 32]).unwrap().into(),
        ))
        .await
        .unwrap();
    assert_generic_close(&mut old, 1_000, "waiting ended").await;

    let mut client = connect(&client_url).await;
    client
        .send(Message::Binary(
            encode_client_select("alice").unwrap().into(),
        ))
        .await
        .unwrap();
    paired(&mut replacement).await;
    paired(&mut client).await;
    assert_eq!(relay.metrics().host_replacements, 1);
    client.close(None).await.unwrap();
    wait_for(|| relay.metrics().active_circuits == 0).await;
    shutdown(relay, task).await;
}

#[tokio::test]
async fn unknown_offline_and_busy_clients_share_the_same_failure() {
    let (relay, task, host_url, client_url) = start_relay().await;
    let mut unknown = connect(&client_url).await;
    unknown
        .send(Message::Binary(
            encode_client_select("unknown").unwrap().into(),
        ))
        .await
        .unwrap();
    assert_generic_close(&mut unknown, 4_004, "unavailable").await;

    let mut host = connect(&host_url).await;
    host.send(Message::Binary(
        encode_host_claim("alice", &[7; 32]).unwrap().into(),
    ))
    .await
    .unwrap();
    wait_for(|| relay.metrics().accepted_host_claims == 1).await;
    host.close(None).await.unwrap();
    wait_for(|| relay.metrics().waiting_hosts == 0).await;
    let mut offline = connect(&client_url).await;
    offline
        .send(Message::Binary(
            encode_client_select("alice").unwrap().into(),
        ))
        .await
        .unwrap();
    assert_generic_close(&mut offline, 4_004, "unavailable").await;

    let mut host = connect(&host_url).await;
    host.send(Message::Binary(
        encode_host_claim("alice", &[7; 32]).unwrap().into(),
    ))
    .await
    .unwrap();
    let mut active = connect(&client_url).await;
    active
        .send(Message::Binary(
            encode_client_select("alice").unwrap().into(),
        ))
        .await
        .unwrap();
    paired(&mut host).await;
    paired(&mut active).await;
    let mut busy = connect(&client_url).await;
    busy.send(Message::Binary(
        encode_client_select("alice").unwrap().into(),
    ))
    .await
    .unwrap();
    assert_generic_close(&mut busy, 4_004, "unavailable").await;
    active.close(None).await.unwrap();
    wait_for(|| relay.metrics().active_circuits == 0).await;
    shutdown(relay, task).await;
}

#[tokio::test]
async fn oversized_preauth_and_plaintext_after_pair_fail_closed() {
    let (relay, task, host_url, client_url) = start_relay().await;
    let mut oversized = connect(&client_url).await;
    oversized
        .send(Message::Binary(vec![0; PREAUTH_MESSAGE_BYTES + 1].into()))
        .await
        .unwrap();
    assert_generic_close(&mut oversized, 4_004, "unavailable").await;

    let mut host = connect(&host_url).await;
    host.send(Message::Binary(
        encode_host_claim("alice", &[7; 32]).unwrap().into(),
    ))
    .await
    .unwrap();
    let mut client = connect(&client_url).await;
    client
        .send(Message::Binary(
            encode_client_select("alice").unwrap().into(),
        ))
        .await
        .unwrap();
    paired(&mut host).await;
    paired(&mut client).await;
    client
        .send(Message::Text("plaintext".into()))
        .await
        .unwrap();
    wait_for(|| relay.metrics().active_circuits == 0).await;
    assert_eq!(relay.metrics().forwarded_client_messages, 0);
    shutdown(relay, task).await;
}

#[tokio::test]
async fn shutdown_interrupts_idle_preauth_and_waiting_hosts() {
    let server = ProofRelayServer::bind_loopback([9; 32]).await.unwrap();
    let address = server.local_addr();
    let relay = server.relay();
    let task = tokio::spawn(async move { server.serve().await.unwrap() });
    let _idle_client = connect(&format!("ws://{address}/client")).await;
    let mut waiting_host = connect(&format!("ws://{address}/host")).await;
    waiting_host
        .send(Message::Binary(
            encode_host_claim("alice", &[7; 32]).unwrap().into(),
        ))
        .await
        .unwrap();
    wait_for(|| relay.metrics().waiting_hosts == 1).await;

    shutdown(relay, task).await;
}

async fn start_relay() -> (ProofRelay, tokio::task::JoinHandle<()>, String, String) {
    let server = ProofRelayServer::bind_loopback([9; 32]).await.unwrap();
    let address = server.local_addr();
    let relay = server.relay();
    let task = tokio::spawn(async move { server.serve().await.unwrap() });
    (
        relay,
        task,
        format!("ws://{address}/host"),
        format!("ws://{address}/client"),
    )
}

async fn connect(url: &str) -> Client {
    timeout(Duration::from_secs(2), connect_async(url))
        .await
        .unwrap()
        .unwrap()
        .0
}

async fn paired(socket: &mut Client) {
    assert_eq!(binary(socket).await, PAIRED_CONTROL);
}

async fn binary(socket: &mut Client) -> Vec<u8> {
    match timeout(Duration::from_secs(2), socket.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap()
    {
        Message::Binary(bytes) => bytes.to_vec(),
        other => panic!("expected binary message, got {other:?}"),
    }
}

async fn assert_generic_close(socket: &mut Client, code: u16, reason: &str) {
    match timeout(Duration::from_secs(2), socket.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap()
    {
        Message::Close(Some(frame)) => {
            assert_eq!(frame.code, code.into());
            assert_eq!(frame.reason, reason);
        }
        other => panic!("expected close, got {other:?}"),
    }
}

async fn wait_for(predicate: impl Fn() -> bool) {
    timeout(Duration::from_secs(2), async {
        while !predicate() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}

async fn shutdown(relay: ProofRelay, task: tokio::task::JoinHandle<()>) {
    relay.shutdown();
    timeout(Duration::from_secs(2), task)
        .await
        .unwrap()
        .unwrap();
    let metrics = relay.metrics();
    assert_eq!(metrics.active_circuits, 0);
    assert_eq!(metrics.waiting_hosts, 0);
    assert_eq!(metrics.active_pumps, 0);
}
