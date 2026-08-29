use super::*;
use crate::tracker::{TrackerConfig, TrackerEndpoint, TrackerSource};
use crate::{PeerBudget, ResumeValidationIntent, TrackerConnectionFamily};
use std::collections::BTreeSet;
use std::net::{IpAddr, Ipv6Addr};

fn pure_v2_info(payload: &[u8]) -> Vec<u8> {
    let root = <[u8; 32]>::from(Sha256::digest(payload));
    let mut info = format!(
        "d9:file treed1:ad0:d6:lengthi{}e11:pieces root32:",
        payload.len()
    )
    .into_bytes();
    info.extend_from_slice(&root);
    info.extend_from_slice(b"eee12:meta versioni2e4:name4:root12:piece lengthi16384ee");
    info
}

fn two_piece_v2_info(first: &[u8], second: &[u8]) -> (Vec<u8>, [[u8; 32]; 2]) {
    assert_eq!(first.len(), 16 * 1024);
    assert_eq!(second.len(), 16 * 1024);
    let piece_roots = [
        rstorrent_protocol::merkle::hash_block(first).unwrap(),
        rstorrent_protocol::merkle::hash_block(second).unwrap(),
    ];
    let file_root = rstorrent_protocol::merkle::hash_pair(&piece_roots[0], &piece_roots[1]);
    let mut info = b"d9:file treed1:ad0:d6:lengthi32768e11:pieces root32:".to_vec();
    info.extend_from_slice(&file_root);
    info.extend_from_slice(b"eee12:meta versioni2e4:name4:root12:piece lengthi16384ee");
    (info, piece_roots)
}

fn four_block_v2_info(blocks: &[Vec<u8>; 4]) -> (Vec<u8>, [[u8; 32]; 4], [[u8; 32]; 2]) {
    assert!(blocks.iter().all(|block| block.len() == 16 * 1024));
    let leaves = blocks
        .each_ref()
        .map(|block| rstorrent_protocol::merkle::hash_block(block).unwrap());
    let piece_roots = [
        rstorrent_protocol::merkle::hash_pair(&leaves[0], &leaves[1]),
        rstorrent_protocol::merkle::hash_pair(&leaves[2], &leaves[3]),
    ];
    let file_root = rstorrent_protocol::merkle::hash_pair(&piece_roots[0], &piece_roots[1]);
    let mut info = b"d9:file treed1:ad0:d6:lengthi65536e11:pieces root32:".to_vec();
    info.extend_from_slice(&file_root);
    info.extend_from_slice(b"eee12:meta versioni2e4:name4:root12:piece lengthi32768ee");
    (info, leaves, piece_roots)
}

async fn begin_v2_scripted_peer(
    listener: &TcpListener,
    wire_hash: [u8; 20],
    peer_id: [u8; 20],
) -> PeerConnection {
    let (mut stream, _) = listener.accept().await.expect("accept v2 scripted peer");
    let mut handshake_bytes = [0; HANDSHAKE_LENGTH];
    stream
        .read_exact(&mut handshake_bytes)
        .await
        .expect("read v2 scripted handshake");
    decode_handshake(&handshake_bytes, wire_hash).expect("v2 scripted wire identity");
    let mut reserved = [0; 8];
    reserved[EXTENSION_PROTOCOL_RESERVED_INDEX] = EXTENSION_PROTOCOL_RESERVED_BIT;
    stream
        .write_all(&encode_handshake_with_reserved(
            wire_hash, peer_id, reserved,
        ))
        .await
        .expect("send v2 scripted handshake");
    let mut peer = PeerConnection::for_test(test_dial_attempt(), stream, Duration::from_secs(5));
    peer.set_protocol(rstorrent_protocol::peer_wire::PeerProtocol::V2);
    peer
}

fn v2_hash_response(
    request: rstorrent_protocol::v2_hashes::HashRequest,
    leaves: &[[u8; 32]; 4],
    piece_roots: &[[u8; 32]; 2],
) -> rstorrent_protocol::v2_hashes::HashResponse {
    let hashes = match request.base_layer {
        1 => piece_roots.to_vec(),
        0 if request.index == 0 && request.count == 2 => {
            vec![leaves[0], leaves[1], piece_roots[1]]
        }
        0 if request.index == 2 && request.count == 2 => {
            vec![leaves[2], leaves[3], piece_roots[0]]
        }
        _ => panic!("unexpected v2 hash request: {request:?}"),
    };
    rstorrent_protocol::v2_hashes::HashResponse { request, hashes }
}

async fn serve_corrupt_v2_contributor(
    listener: TcpListener,
    wire_hash: [u8; 20],
    info: Vec<u8>,
    blocks: [Vec<u8>; 4],
    leaves: [[u8; 32]; 4],
    piece_roots: [[u8; 32]; 2],
    released: tokio::sync::watch::Sender<bool>,
) -> u32 {
    let mut peer = begin_v2_scripted_peer(
        &listener,
        wire_hash,
        scripted_peer_id(&listener, *b"-RS-V2BAD-0000000000"),
    )
    .await;
    assert!(matches!(
        next_peer_message(&mut peer).await,
        Ok(PeerMessage::Extended { id: 0, .. })
    ));
    send_message(
        &mut peer,
        &PeerMessage::Extended {
            id: 0,
            payload: encode_extension_handshake(Some(info.len())),
        },
    )
    .await
    .unwrap();
    let PeerMessage::Extended {
        id: UT_METADATA_LOCAL_ID,
        payload,
    } = next_peer_message(&mut peer).await.unwrap()
    else {
        panic!("expected v2 corruption metadata request");
    };
    assert!(matches!(
        parse_metadata_message(&payload).unwrap(),
        MetadataMessage::Request { piece: 0 }
    ));
    send_message(&mut peer, &PeerMessage::Bitfield(vec![0b1000_0000]))
        .await
        .unwrap();
    send_message(&mut peer, &PeerMessage::Unchoke)
        .await
        .unwrap();
    send_message(
        &mut peer,
        &PeerMessage::Extended {
            id: UT_METADATA_LOCAL_ID,
            payload: encode_metadata_data(0, info.len(), &info).unwrap(),
        },
    )
    .await
    .unwrap();

    loop {
        let message = match next_peer_message(&mut peer).await {
            Ok(message) => message,
            Err(_) => panic!("corrupt v2 peer closed before contributing payload"),
        };
        match message {
            PeerMessage::HashRequest(request) => {
                send_message(
                    &mut peer,
                    &PeerMessage::Hashes(v2_hash_response(request, &leaves, &piece_roots)),
                )
                .await
                .unwrap();
            }
            PeerMessage::Request(request) if request.index == 0 => {
                let begin = request.begin as usize;
                let mut block = blocks[request.index as usize * 2 + begin / (16 * 1024)].clone();
                block[0] ^= 0xff;
                send_message(
                    &mut peer,
                    &PeerMessage::Piece {
                        index: request.index,
                        begin: request.begin,
                        block,
                    },
                )
                .await
                .unwrap();
                send_message(&mut peer, &PeerMessage::Choke).await.unwrap();
                let _ = released.send(true);
                return request.begin;
            }
            PeerMessage::KeepAlive
            | PeerMessage::Interested
            | PeerMessage::NotInterested
            | PeerMessage::HaveNone
            | PeerMessage::Have(_)
            | PeerMessage::Cancel(_)
            | PeerMessage::Request(_)
            | PeerMessage::Extended { .. } => {}
            message => panic!("unexpected corrupt v2 message: {message:?}"),
        }
    }
}

#[derive(Clone, Copy)]
enum LeafProofBehavior {
    Serve,
    Reject,
    Stall,
}

async fn serve_clean_v2_repair_peer(
    listener: TcpListener,
    wire_hash: [u8; 20],
    blocks: [Vec<u8>; 4],
    leaves: [[u8; 32]; 4],
    piece_roots: [[u8; 32]; 2],
    mut released: tokio::sync::watch::Receiver<bool>,
    leaf_proofs: LeafProofBehavior,
) -> Vec<(u32, u32)> {
    let mut requested = Vec::new();
    let mut requested_active_leaf_proof = false;
    let mut received_active_leaf_proof = false;
    let mut saw_final_have = false;
    loop {
        let mut peer = begin_v2_scripted_peer(
            &listener,
            wire_hash,
            scripted_peer_id(&listener, *b"-RS-V2FIX-0000000000"),
        )
        .await;
        if send_message(&mut peer, &PeerMessage::Bitfield(vec![0b1100_0000]))
            .await
            .is_err()
        {
            continue;
        }
        while !*released.borrow_and_update() {
            released
                .changed()
                .await
                .expect("corrupt peer release signal");
        }
        if send_message(&mut peer, &PeerMessage::Unchoke)
            .await
            .is_err()
        {
            continue;
        }
        loop {
            match next_peer_message(&mut peer).await {
                Ok(PeerMessage::HashRequest(request)) => match (leaf_proofs, request.base_layer) {
                    (LeafProofBehavior::Reject, 0) => {
                        send_message(&mut peer, &PeerMessage::HashReject(request))
                            .await
                            .unwrap();
                    }
                    (LeafProofBehavior::Stall, 0) => {}
                    _ => {
                        send_message(
                            &mut peer,
                            &PeerMessage::Hashes(v2_hash_response(request, &leaves, &piece_roots)),
                        )
                        .await
                        .unwrap();
                    }
                },
                Ok(PeerMessage::Request(request)) => {
                    requested.push((request.index, request.begin));
                    let block = blocks
                        [request.index as usize * 2 + request.begin as usize / (16 * 1024)]
                        .clone();
                    send_message(
                        &mut peer,
                        &PeerMessage::Piece {
                            index: request.index,
                            begin: request.begin,
                            block,
                        },
                    )
                    .await
                    .unwrap();
                }
                Ok(PeerMessage::Have(0)) if !requested_active_leaf_proof => {
                    let request = rstorrent_protocol::v2_hashes::HashRequest {
                        pieces_root: rstorrent_protocol::merkle::hash_pair(
                            &piece_roots[0],
                            &piece_roots[1],
                        ),
                        base_layer: 0,
                        index: 0,
                        count: 2,
                        proof_layers: 1,
                    };
                    send_message(&mut peer, &PeerMessage::HashRequest(request))
                        .await
                        .unwrap();
                    requested_active_leaf_proof = true;
                }
                Ok(PeerMessage::Have(1)) => saw_final_have = true,
                Ok(PeerMessage::Hashes(response)) if requested_active_leaf_proof => {
                    assert_eq!(
                        response,
                        v2_hash_response(response.request, &leaves, &piece_roots),
                        "initiated active peer serves the authenticated base-zero proof"
                    );
                    received_active_leaf_proof = true;
                }
                Ok(
                    PeerMessage::KeepAlive
                    | PeerMessage::Interested
                    | PeerMessage::NotInterested
                    | PeerMessage::HaveNone
                    | PeerMessage::Have(_)
                    | PeerMessage::Cancel(_)
                    | PeerMessage::Extended { .. },
                ) => {}
                Ok(message) => panic!("unexpected clean v2 message: {message:?}"),
                Err(_) if requested.is_empty() => break,
                Err(_) => return requested,
            }
            if saw_final_have && received_active_leaf_proof {
                return requested;
            }
        }
    }
}

async fn serve_info_only_metadata(listener: TcpListener, wire_hash: [u8; 20], info: Vec<u8>) {
    let (mut stream, _) = listener.accept().await.expect("accept metadata client");
    let mut handshake_bytes = [0; HANDSHAKE_LENGTH];
    stream
        .read_exact(&mut handshake_bytes)
        .await
        .expect("read metadata handshake");
    let handshake = decode_handshake(&handshake_bytes, wire_hash).expect("wire identity");
    assert!(handshake.supports_extensions());
    let mut reserved = [0; 8];
    reserved[EXTENSION_PROTOCOL_RESERVED_INDEX] = EXTENSION_PROTOCOL_RESERVED_BIT;
    stream
        .write_all(&encode_handshake_with_reserved(
            wire_hash,
            scripted_peer_id(&listener, *b"-RS-V2MD-00000000000"),
            reserved,
        ))
        .await
        .expect("send metadata handshake");
    let mut peer = PeerConnection::for_test(test_dial_attempt(), stream, Duration::from_secs(2));
    assert!(matches!(
        next_peer_message(&mut peer).await,
        Ok(PeerMessage::Extended { id: 0, .. })
    ));
    send_message(
        &mut peer,
        &PeerMessage::Extended {
            id: 0,
            payload: encode_extension_handshake(Some(info.len())),
        },
    )
    .await
    .expect("advertise metadata");

    let block_count = info
        .len()
        .div_ceil(rstorrent_protocol::metadata::METADATA_BLOCK_LENGTH);
    for _ in 0..block_count {
        let PeerMessage::Extended {
            id: UT_METADATA_LOCAL_ID,
            payload,
        } = next_peer_message(&mut peer)
            .await
            .expect("metadata request")
        else {
            panic!("expected metadata request");
        };
        let MetadataMessage::Request { piece } =
            parse_metadata_message(&payload).expect("parse metadata request")
        else {
            panic!("expected metadata request payload");
        };
        let index = usize::try_from(piece).expect("nonnegative metadata piece");
        let piece = u32::try_from(piece).expect("bounded metadata piece");
        let start = index * rstorrent_protocol::metadata::METADATA_BLOCK_LENGTH;
        let end = (start + rstorrent_protocol::metadata::METADATA_BLOCK_LENGTH).min(info.len());
        send_message(
            &mut peer,
            &PeerMessage::Extended {
                id: UT_METADATA_LOCAL_ID,
                payload: encode_metadata_data(piece, info.len(), &info[start..end])
                    .expect("encode metadata"),
            },
        )
        .await
        .expect("send metadata");
    }
    let _ = timeout(Duration::from_secs(1), next_peer_message(&mut peer)).await;
}

async fn serve_v2_metadata_hashes_and_payload(
    listener: TcpListener,
    wire_hash: [u8; 20],
    info: Vec<u8>,
    pieces: [Vec<u8>; 2],
    piece_roots: [[u8; 32]; 2],
) {
    let (mut stream, _) = listener.accept().await.expect("accept v2 client");
    let mut handshake_bytes = [0; HANDSHAKE_LENGTH];
    stream
        .read_exact(&mut handshake_bytes)
        .await
        .expect("read v2 handshake");
    let handshake = decode_handshake(&handshake_bytes, wire_hash).expect("v2 wire identity");
    assert!(handshake.supports_extensions());
    let mut reserved = [0; 8];
    reserved[EXTENSION_PROTOCOL_RESERVED_INDEX] = EXTENSION_PROTOCOL_RESERVED_BIT;
    stream
        .write_all(&encode_handshake_with_reserved(
            wire_hash,
            scripted_peer_id(&listener, *b"-RS-V2DL-00000000000"),
            reserved,
        ))
        .await
        .expect("send v2 handshake");
    let mut peer = PeerConnection::for_test(test_dial_attempt(), stream, Duration::from_secs(5));
    peer.set_protocol(rstorrent_protocol::peer_wire::PeerProtocol::V2);
    assert!(matches!(
        next_peer_message(&mut peer).await,
        Ok(PeerMessage::Extended { id: 0, .. })
    ));
    send_message(
        &mut peer,
        &PeerMessage::Extended {
            id: 0,
            payload: encode_extension_handshake(Some(info.len())),
        },
    )
    .await
    .expect("advertise v2 metadata");
    let PeerMessage::Extended {
        id: UT_METADATA_LOCAL_ID,
        payload,
    } = next_peer_message(&mut peer)
        .await
        .expect("v2 metadata request")
    else {
        panic!("expected v2 metadata request");
    };
    assert!(matches!(
        parse_metadata_message(&payload).expect("parse v2 metadata request"),
        MetadataMessage::Request { piece: 0 }
    ));
    send_message(&mut peer, &PeerMessage::Bitfield(vec![0b1100_0000]))
        .await
        .expect("send v2 availability");
    send_message(&mut peer, &PeerMessage::Unchoke)
        .await
        .expect("unchoke v2 client");
    send_message(
        &mut peer,
        &PeerMessage::Extended {
            id: UT_METADATA_LOCAL_ID,
            payload: encode_metadata_data(0, info.len(), &info).expect("encode v2 metadata"),
        },
    )
    .await
    .expect("send v2 metadata");

    let mut hash_served = false;
    let mut payload_requests = 0_usize;
    while payload_requests != pieces.len() {
        match next_peer_message(&mut peer)
            .await
            .expect("read v2 content message")
        {
            PeerMessage::HashRequest(request) => {
                assert!(!hash_served, "piece layer is requested once");
                assert_eq!(request.base_layer, 0);
                assert_eq!(request.index, 0);
                assert_eq!(request.count, 2);
                assert_eq!(request.proof_layers, 0);
                hash_served = true;
                send_message(
                    &mut peer,
                    &PeerMessage::Hashes(rstorrent_protocol::v2_hashes::HashResponse {
                        request,
                        hashes: piece_roots.to_vec(),
                    }),
                )
                .await
                .expect("send authenticated piece layer");
            }
            PeerMessage::Request(request) => {
                assert!(hash_served, "payload must not precede authenticated hashes");
                let piece = usize::try_from(request.index).expect("bounded piece index");
                let begin = usize::try_from(request.begin).expect("bounded block begin");
                let length = usize::try_from(request.length).expect("bounded block length");
                send_message(
                    &mut peer,
                    &PeerMessage::Piece {
                        index: request.index,
                        begin: request.begin,
                        block: pieces[piece][begin..begin + length].to_vec(),
                    },
                )
                .await
                .expect("send v2 payload");
                payload_requests += 1;
            }
            PeerMessage::KeepAlive
            | PeerMessage::Interested
            | PeerMessage::NotInterested
            | PeerMessage::HaveNone
            | PeerMessage::Have(_)
            | PeerMessage::Extended { .. } => {}
            message => panic!("unexpected v2 client message: {message:?}"),
        }
    }
}

async fn serve_v2_hashes_and_missing_payload(
    listener: TcpListener,
    wire_hash: [u8; 20],
    pieces: [Vec<u8>; 2],
    piece_roots: [[u8; 32]; 2],
) -> Vec<u32> {
    let (mut stream, _) = listener.accept().await.expect("accept resumed v2 client");
    let mut handshake_bytes = [0; HANDSHAKE_LENGTH];
    stream
        .read_exact(&mut handshake_bytes)
        .await
        .expect("read resumed v2 handshake");
    decode_handshake(&handshake_bytes, wire_hash).expect("resumed v2 wire identity");
    let mut reserved = [0; 8];
    reserved[EXTENSION_PROTOCOL_RESERVED_INDEX] = EXTENSION_PROTOCOL_RESERVED_BIT;
    stream
        .write_all(&encode_handshake_with_reserved(
            wire_hash,
            scripted_peer_id(&listener, *b"-RS-V2RS-00000000000"),
            reserved,
        ))
        .await
        .expect("send resumed v2 handshake");
    let mut peer = PeerConnection::for_test(test_dial_attempt(), stream, Duration::from_secs(5));
    peer.set_protocol(rstorrent_protocol::peer_wire::PeerProtocol::V2);
    send_message(&mut peer, &PeerMessage::Bitfield(vec![0b1100_0000]))
        .await
        .expect("send resumed v2 availability");
    send_message(&mut peer, &PeerMessage::Unchoke)
        .await
        .expect("unchoke resumed v2 client");

    let mut requested = Vec::new();
    loop {
        match next_peer_message(&mut peer)
            .await
            .expect("read resumed v2 content message")
        {
            PeerMessage::HashRequest(request) => {
                send_message(
                    &mut peer,
                    &PeerMessage::Hashes(rstorrent_protocol::v2_hashes::HashResponse {
                        request,
                        hashes: piece_roots.to_vec(),
                    }),
                )
                .await
                .expect("send resumed authenticated piece layer");
            }
            PeerMessage::Request(request) => {
                requested.push(request.index);
                assert_eq!(
                    request.index, 1,
                    "valid candidate piece must not be refetched"
                );
                let begin = usize::try_from(request.begin).expect("bounded block begin");
                let length = usize::try_from(request.length).expect("bounded block length");
                send_message(
                    &mut peer,
                    &PeerMessage::Piece {
                        index: request.index,
                        begin: request.begin,
                        block: pieces[1][begin..begin + length].to_vec(),
                    },
                )
                .await
                .expect("send resumed missing payload");
                if begin + length == pieces[1].len() {
                    return requested;
                }
            }
            PeerMessage::KeepAlive
            | PeerMessage::Interested
            | PeerMessage::NotInterested
            | PeerMessage::HaveNone
            | PeerMessage::Have(_)
            | PeerMessage::Extended { .. } => {}
            message => panic!("unexpected resumed v2 client message: {message:?}"),
        }
    }
}

#[tokio::test]
async fn pure_v2_magnet_metadata_uses_full_sha256_and_strict_format() {
    async fn acquire(info: Vec<u8>) -> Result<Vec<u8>, DownloadError> {
        let full_hash = <[u8; 32]>::from(Sha256::digest(&info));
        let identity = V2InfoHash::new(full_hash);
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind v2 metadata peer");
        let address = listener.local_addr().expect("v2 metadata address");
        let peer = tokio::spawn(serve_info_only_metadata(
            listener,
            identity.swarm_key().into_bytes(),
            info,
        ));
        let result = download_magnet_metadata_with_control(
            test_v2_identity(full_hash),
            format!("magnet:?xt=urn:btmh:1220{identity}&x.pe={address}"),
            loopback_network(Duration::from_secs(2)),
            DownloadControl::new(),
        )
        .await;
        peer.await.expect("join v2 metadata peer");
        result
    }

    let valid = pure_v2_info(b"v2 metadata payload");
    assert_eq!(
        acquire(valid.clone())
            .await
            .expect("acquire strict pure-v2 info"),
        valid
    );

    let v1 = single_file_info(b"v1 shape under a btmh identity");
    assert!(matches!(
        acquire(v1).await,
        Err(DownloadError::InvalidPremetadataState(_))
    ));
    assert!(matches!(
        acquire(b"not bencoded metadata".to_vec()).await,
        Err(DownloadError::Metainfo(_))
    ));
}

#[tokio::test]
async fn pure_v2_magnet_authenticates_piece_layer_before_payload() {
    let pieces = [vec![0x31; 16 * 1024], vec![0x52; 16 * 1024]];
    let (info, piece_roots) = two_piece_v2_info(&pieces[0], &pieces[1]);
    let full_hash = <[u8; 32]>::from(Sha256::digest(&info));
    let identity = V2InfoHash::new(full_hash);
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind v2 content peer");
    let address = listener.local_addr().expect("v2 content address");
    let peer = tokio::spawn(serve_v2_metadata_hashes_and_payload(
        listener,
        identity.swarm_key().into_bytes(),
        info,
        pieces.clone(),
        piece_roots,
    ));
    let output = test_path("v2-magnet-output");
    let report = timeout(
        Duration::from_secs(10),
        download_magnet(MagnetDownloadConfig {
            identity: test_v2_identity(full_hash),
            magnet: format!("magnet:?xt=urn:btmh:1220{identity}&x.pe={address}"),
            output_path: output.clone(),
            network: loopback_network(Duration::from_secs(2)),
            resource_limits: resource_limits(MIN_PAYLOAD_ALLOWANCE),
            skip_files: Vec::new(),
            high_priority_files: Vec::new(),
            dht: None,
        }),
    )
    .await
    .expect("bounded v2 magnet download")
    .expect("v2 magnet download");
    assert_eq!(report.verified_piece_count, 2);
    assert_eq!(
        tokio::fs::read(output.join("a"))
            .await
            .expect("read v2 selected file"),
        pieces.concat()
    );
    peer.await.expect("join v2 peer");
    let _ = tokio::fs::remove_dir_all(output).await;
}

#[tokio::test]
async fn pure_v2_leaf_proof_repairs_only_the_corrupt_block() {
    let blocks = [
        vec![0x11; 16 * 1024],
        vec![0x22; 16 * 1024],
        vec![0x33; 16 * 1024],
        vec![0x44; 16 * 1024],
    ];
    let (info, leaves, piece_roots) = four_block_v2_info(&blocks);
    let full_hash = <[u8; 32]>::from(Sha256::digest(&info));
    let identity = V2InfoHash::new(full_hash);
    let wire_hash = identity.swarm_key().into_bytes();
    let corrupt_listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind corrupt v2 contributor");
    let corrupt_address = corrupt_listener
        .local_addr()
        .expect("corrupt v2 contributor address");
    let clean_listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind clean v2 repair peer");
    let clean_address = clean_listener
        .local_addr()
        .expect("clean v2 repair address");
    let (released, release) = tokio::sync::watch::channel(false);
    let corrupt_peer = tokio::spawn(serve_corrupt_v2_contributor(
        corrupt_listener,
        wire_hash,
        info,
        blocks.clone(),
        leaves,
        piece_roots,
        released,
    ));
    let clean_peer = tokio::spawn(serve_clean_v2_repair_peer(
        clean_listener,
        wire_hash,
        blocks.clone(),
        leaves,
        piece_roots,
        release,
        LeafProofBehavior::Serve,
    ));
    let output = test_path("v2-leaf-repair-output");
    let report = timeout(
        Duration::from_secs(15),
        download_magnet(MagnetDownloadConfig {
            identity: test_v2_identity(full_hash),
            magnet: format!(
                "magnet:?xt=urn:btmh:1220{identity}&x.pe={corrupt_address}&x.pe={clean_address}"
            ),
            output_path: output.clone(),
            network: loopback_network(Duration::from_secs(2)),
            resource_limits: resource_limits(MIN_PAYLOAD_ALLOWANCE),
            skip_files: Vec::new(),
            high_priority_files: Vec::new(),
            dht: None,
        }),
    )
    .await
    .expect("bounded v2 corrupt-block recovery")
    .expect("recover v2 corrupt block");
    let corrupt_begin = corrupt_peer.await.expect("join corrupt v2 peer");
    let clean_requests = clean_peer.await.expect("join clean v2 peer");

    assert_eq!(report.verified_piece_count, 2);
    assert_eq!(
        tokio::fs::read(output.join("a"))
            .await
            .expect("read repaired v2 file"),
        blocks.concat()
    );
    assert_eq!(
        clean_requests
            .iter()
            .filter(|request| **request == (0, corrupt_begin))
            .count(),
        1,
        "the diagnosed corrupt block is fetched exactly once from the repair peer"
    );
    let retained_begin = if corrupt_begin == 0 { 16 * 1024 } else { 0 };
    assert_eq!(
        clean_requests
            .iter()
            .filter(|request| **request == (0, retained_begin))
            .count(),
        1,
        "the already-good block is not discarded and fetched again after diagnosis"
    );
    tokio::fs::remove_dir_all(output)
        .await
        .expect("remove v2 repair output");
}

#[tokio::test]
async fn pure_v2_leaf_reject_falls_back_to_whole_piece_repair() {
    let blocks = [
        vec![0x51; 16 * 1024],
        vec![0x62; 16 * 1024],
        vec![0x73; 16 * 1024],
        vec![0x84; 16 * 1024],
    ];
    let (info, leaves, piece_roots) = four_block_v2_info(&blocks);
    let full_hash = <[u8; 32]>::from(Sha256::digest(&info));
    let identity = V2InfoHash::new(full_hash);
    let wire_hash = identity.swarm_key().into_bytes();
    let corrupt_listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind fallback corrupt contributor");
    let corrupt_address = corrupt_listener
        .local_addr()
        .expect("fallback corrupt contributor address");
    let clean_listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind fallback repair peer");
    let clean_address = clean_listener
        .local_addr()
        .expect("fallback repair address");
    let (released, release) = tokio::sync::watch::channel(false);
    let corrupt_peer = tokio::spawn(serve_corrupt_v2_contributor(
        corrupt_listener,
        wire_hash,
        info,
        blocks.clone(),
        leaves,
        piece_roots,
        released,
    ));
    let clean_peer = tokio::spawn(serve_clean_v2_repair_peer(
        clean_listener,
        wire_hash,
        blocks.clone(),
        leaves,
        piece_roots,
        release,
        LeafProofBehavior::Reject,
    ));
    let output = test_path("v2-leaf-reject-output");
    timeout(
        Duration::from_secs(15),
        download_magnet(MagnetDownloadConfig {
            identity: test_v2_identity(full_hash),
            magnet: format!(
                "magnet:?xt=urn:btmh:1220{identity}&x.pe={corrupt_address}&x.pe={clean_address}"
            ),
            output_path: output.clone(),
            network: loopback_network(Duration::from_secs(2)),
            resource_limits: resource_limits(MIN_PAYLOAD_ALLOWANCE),
            skip_files: Vec::new(),
            high_priority_files: Vec::new(),
            dht: None,
        }),
    )
    .await
    .expect("bounded v2 leaf-reject fallback")
    .expect("recover whole v2 piece after leaf reject");
    let corrupt_begin = corrupt_peer.await.expect("join fallback corrupt peer");
    let clean_requests = clean_peer.await.expect("join fallback repair peer");

    assert_eq!(
        tokio::fs::read(output.join("a"))
            .await
            .expect("read fallback-repaired v2 file"),
        blocks.concat()
    );
    assert_eq!(
        clean_requests
            .iter()
            .filter(|request| **request == (0, corrupt_begin))
            .count(),
        1
    );
    let discarded_good_begin = if corrupt_begin == 0 { 16 * 1024 } else { 0 };
    assert_eq!(
        clean_requests
            .iter()
            .filter(|request| **request == (0, discarded_good_begin))
            .count(),
        2,
        "leaf rejection conservatively resets and fetches the whole piece"
    );
    tokio::fs::remove_dir_all(output)
        .await
        .expect("remove fallback repair output");
}

#[tokio::test]
async fn pure_v2_leaf_stall_falls_back_to_whole_piece_repair() {
    let blocks = [
        vec![0x91; 16 * 1024],
        vec![0xa2; 16 * 1024],
        vec![0xb3; 16 * 1024],
        vec![0xc4; 16 * 1024],
    ];
    let (info, leaves, piece_roots) = four_block_v2_info(&blocks);
    let full_hash = <[u8; 32]>::from(Sha256::digest(&info));
    let identity = V2InfoHash::new(full_hash);
    let wire_hash = identity.swarm_key().into_bytes();
    let corrupt_listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind stalled-proof corrupt contributor");
    let corrupt_address = corrupt_listener
        .local_addr()
        .expect("stalled-proof corrupt contributor address");
    let clean_listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind stalled-proof repair peer");
    let clean_address = clean_listener
        .local_addr()
        .expect("stalled-proof repair address");
    let (released, release) = tokio::sync::watch::channel(false);
    let corrupt_peer = tokio::spawn(serve_corrupt_v2_contributor(
        corrupt_listener,
        wire_hash,
        info,
        blocks.clone(),
        leaves,
        piece_roots,
        released,
    ));
    let clean_peer = tokio::spawn(serve_clean_v2_repair_peer(
        clean_listener,
        wire_hash,
        blocks.clone(),
        leaves,
        piece_roots,
        release,
        LeafProofBehavior::Stall,
    ));
    let output = test_path("v2-leaf-stall-output");
    timeout(
        Duration::from_secs(15),
        download_magnet(MagnetDownloadConfig {
            identity: test_v2_identity(full_hash),
            magnet: format!(
                "magnet:?xt=urn:btmh:1220{identity}&x.pe={corrupt_address}&x.pe={clean_address}"
            ),
            output_path: output.clone(),
            network: loopback_network(Duration::from_secs(2)),
            resource_limits: resource_limits(MIN_PAYLOAD_ALLOWANCE),
            skip_files: Vec::new(),
            high_priority_files: Vec::new(),
            dht: None,
        }),
    )
    .await
    .expect("bounded v2 stalled-leaf fallback")
    .expect("recover whole v2 piece after stalled leaf proof");
    let corrupt_begin = corrupt_peer.await.expect("join stalled-proof corrupt peer");
    let clean_requests = clean_peer.await.expect("join stalled-proof repair peer");

    assert_eq!(
        tokio::fs::read(output.join("a"))
            .await
            .expect("read stalled-proof repaired v2 file"),
        blocks.concat()
    );
    assert_eq!(
        clean_requests
            .iter()
            .filter(|request| **request == (0, corrupt_begin))
            .count(),
        1
    );
    let discarded_good_begin = if corrupt_begin == 0 { 16 * 1024 } else { 0 };
    assert_eq!(
        clean_requests
            .iter()
            .filter(|request| **request == (0, discarded_good_begin))
            .count(),
        2,
        "leaf timeout conservatively resets and fetches the whole piece"
    );
    tokio::fs::remove_dir_all(output)
        .await
        .expect("remove stalled-proof repair output");
}

#[tokio::test]
async fn pure_v2_restart_refetches_hashes_before_promoting_candidate_payload() {
    let pieces = [vec![0x41; 16 * 1024], vec![0x62; 16 * 1024]];
    let (info, piece_roots) = two_piece_v2_info(&pieces[0], &pieces[1]);
    let full_hash = <[u8; 32]>::from(Sha256::digest(&info));
    let identity = V2InfoHash::new(full_hash);
    let runtime = TorrentContent::from_v2_info_bytes_with_limits(&info, BEP9_METAINFO_LIMITS)
        .expect("info-only v2 runtime");
    let root = test_path("v2-candidate-restart");
    tokio::fs::create_dir(&root)
        .await
        .expect("create v2 candidate root");
    let artifact_identity = TorrentArtifactIdentity {
        torrent_id: test_torrent_id(),
        content_fingerprint: ContentFingerprint::for_info_bytes(&info),
    };
    let mut storage = SelectiveStorage::create_content(
        root.join(runtime.content.name()),
        artifact_identity,
        Arc::new(runtime.content.clone()),
        &[],
    )
    .await
    .expect("create v2 candidate content");
    storage
        .write_block(0, 0, pieces[0].clone())
        .await
        .expect("write candidate piece");
    storage.sync_piece(0).await.expect("sync candidate piece");
    storage
        .set_verified(0, true)
        .expect("record pre-restart candidate bit");
    drop(storage);

    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind resumed v2 peer");
    let address = listener.local_addr().expect("resumed v2 peer address");
    let peer = tokio::spawn(serve_v2_hashes_and_missing_payload(
        listener,
        identity.swarm_key().into_bytes(),
        pieces.clone(),
        piece_roots,
    ));
    let checkpoints = Arc::new(RecordingCheckpointSink::default());
    let report = timeout(
        Duration::from_secs(10),
        resume_magnet(
            ResumableMagnetDownloadConfig {
                identity: test_v2_identity(full_hash),
                magnet: format!("magnet:?xt=urn:btmh:1220{identity}&x.pe={address}"),
                storage_root: root.clone(),
                network: loopback_network(Duration::from_secs(2)),
                peer_budget: PeerBudget::system_default(),
                mse_dh: crate::MseDhWorkOwner::new(),
                encryption: crate::PeerEncryptionPolicyHandle::default(),
                torrent_peers: None,
                resource_limits: resource_limits(MIN_PAYLOAD_ALLOWANCE),
                skip_files: Vec::new(),
                high_priority_files: Vec::new(),
                verified_info: Some(info),
                verified_pieces: vec![true, false],
                resume_validation: ResumeValidationIntent::Full,
                download_missing: true,
                dht: None,
                trackers: Some(Vec::new()),
            },
            checkpoints,
        ),
    )
    .await
    .expect("bounded v2 candidate restart")
    .expect("complete v2 candidate restart");
    let requested = peer.await.expect("join resumed v2 peer");
    assert!(!requested.is_empty());
    assert!(requested.iter().all(|piece| *piece == 1));
    assert_eq!(report.verified_piece_count, 2);
    assert_eq!(
        tokio::fs::read(root.join("root/a"))
            .await
            .expect("read resumed v2 completion"),
        pieces.concat()
    );
    tokio::fs::remove_dir_all(root)
        .await
        .expect("remove v2 candidate root");
}

#[tokio::test]
async fn explicit_policies_gate_non_loopback_peers_and_offline_dns() {
    let public = "192.0.2.1:6881".parse().expect("documentation peer");
    let loopback = TorrentPeerCoordinator::from_endpoint(
        public,
        PeerSource::Manual,
        loopback_network(Duration::from_secs(1)),
    );
    assert!(matches!(
        loopback,
        Err(DownloadError::NetworkPolicyDenied {
            address,
            policy: NetworkPolicy::LoopbackOnly,
        }) if address == public
    ));

    let online = TorrentPeerCoordinator::from_endpoint(
        public,
        PeerSource::Manual,
        NetworkConfig::new(
            NetworkPolicy::Online,
            Duration::from_secs(1),
            Duration::from_secs(1),
        ),
    )
    .expect("online policy accepts valid public peer");
    assert_eq!(online.registry_len(), 1);

    let offline = download_magnet_metadata_with_control(
        test_identity([
            0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d,
            0x0e, 0x0f, 0x10, 0x11, 0x12, 0x13,
        ]),
        "magnet:?xt=urn:btih:000102030405060708090a0b0c0d0e0f10111213\
             &x.pe=must-not-resolve.invalid:6881"
            .to_owned(),
        NetworkConfig::new(
            NetworkPolicy::Offline,
            Duration::from_secs(1),
            Duration::from_secs(1),
        ),
        DownloadControl::new(),
    )
    .await;
    assert!(matches!(offline, Err(DownloadError::NetworkDisabled)));
}

#[tokio::test]
async fn final_dial_rechecks_network_policy() {
    let public = "192.0.2.1:6881".parse().expect("documentation peer");
    let mut peers = TorrentPeerCoordinator::from_endpoint(
        public,
        PeerSource::Manual,
        NetworkConfig::new(
            NetworkPolicy::Online,
            Duration::from_secs(1),
            Duration::from_secs(1),
        ),
    )
    .expect("online peer session");
    peers.network.policy = NetworkPolicy::LoopbackOnly;

    let result = peers.connect_next([0; 20], false).await;
    assert!(matches!(
        result,
        Err(DownloadError::NetworkPolicyDenied {
            address,
            policy: NetworkPolicy::LoopbackOnly,
        }) if address == public
    ));
}

#[tokio::test]
async fn fragmented_bytes_cannot_extend_one_message_deadline() {
    let (mut peer, mut server) = connected_pair(Duration::from_millis(50)).await;
    let frame = encode_message(&PeerMessage::KeepAlive).expect("keepalive frame");
    let writer = tokio::spawn(async move {
        for byte in frame {
            if server.write_all(&[byte]).await.is_err() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    });

    let result = next_peer_message(&mut peer).await;
    assert!(matches!(
        result,
        Err(DownloadError::PeerTimedOut {
            operation: "message read",
            ..
        })
    ));
    writer.await.expect("fragment writer");
}

#[tokio::test]
async fn timely_messages_can_outlive_one_io_deadline() {
    let io_timeout = Duration::from_millis(150);
    let (mut peer, mut server) = connected_pair(io_timeout).await;
    let frame = encode_message(&PeerMessage::KeepAlive).expect("keepalive frame");
    let writer = tokio::spawn(async move {
        for _ in 0..4 {
            server
                .write_all(&frame)
                .await
                .expect("write complete keepalive");
            tokio::time::sleep(Duration::from_millis(80)).await;
        }
    });

    for _ in 0..4 {
        assert_eq!(
            next_peer_message(&mut peer)
                .await
                .expect("timely complete message"),
            PeerMessage::KeepAlive
        );
    }
    writer.await.expect("timely message writer");
}

#[tokio::test]
#[ignore = "uses changing public trackers and swarm state"]
async fn live_big_buck_bunny_metadata_probe() {
    let magnet = "magnet:?xt=urn:btih:dd8255ecdc7ca55fb0bbf81323d87062db1f6d1c\
&dn=Big+Buck+Bunny\
&tr=udp%3A%2F%2Fexplodie.org%3A6969\
&tr=udp%3A%2F%2Ftracker.coppersurfer.tk%3A6969\
&tr=udp%3A%2F%2Ftracker.empire-js.us%3A1337\
&tr=udp%3A%2F%2Ftracker.leechers-paradise.org%3A6969\
&tr=udp%3A%2F%2Ftracker.opentrackr.org%3A1337";
    let control = DownloadControl::new();
    let activity = Arc::new(RecordingActivitySink::default());
    control.set_activity_sink(activity.clone());
    let task_control = control.clone();
    let mut task = tokio::spawn(download_magnet_metadata_with_control(
        test_identity(
            Magnet::parse(magnet)
                .expect("valid magnet")
                .identity
                .swarm_key()
                .into_bytes(),
        ),
        magnet.to_owned(),
        NetworkConfig::new(
            NetworkPolicy::Online,
            Duration::from_secs(15),
            Duration::from_secs(30),
        ),
        task_control,
    ));
    let monitor_control = control.clone();
    let monitor = tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(10)).await;
            let snapshot = monitor_control.diagnostic_snapshot().metadata;
            let registry = snapshot.registry.as_ref().map(|registry| registry.counts);
            eprintln!(
                "public metadata probe: elapsed={:?} phase={:?} registry={registry:?} \
                     pending_dials={} active_workers={} attempts={} requests={} blocks={} bytes={} \
                     active={} recent={} dropped={}",
                snapshot.captured_at,
                snapshot.phase,
                snapshot.pending_dials,
                snapshot.active_workers,
                snapshot.total_attempts,
                snapshot.total_requests_sent,
                snapshot.total_blocks_received,
                snapshot.total_bytes_received,
                snapshot.active_attempts.len(),
                snapshot.recent_attempts.len(),
                snapshot.recent_attempts_dropped,
            );
        }
    });

    let raw_info = match timeout(Duration::from_secs(90), &mut task).await {
        Ok(result) => {
            monitor.abort();
            let _ = monitor.await;
            let raw_info = result
                .expect("join public metadata probe")
                .expect("acquire public metadata");
            eprintln!(
                "public metadata probe completed:\n{:#?}",
                control.diagnostic_snapshot()
            );
            raw_info
        }
        Err(_) => {
            monitor.abort();
            let _ = monitor.await;
            let timeout_snapshot = control.diagnostic_snapshot();
            let events = activity
                .events
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .clone();
            eprintln!("public metadata probe timeout snapshot:\n{timeout_snapshot:#?}");
            eprintln!("public metadata probe activity:\n{events:#?}");
            control.cancel();
            if timeout(Duration::from_secs(5), &mut task).await.is_err() {
                task.abort();
                let _ = task.await;
            }
            panic!("public metadata probe exceeded 90 seconds");
        }
    };
    let metainfo = Metainfo::from_info_bytes(&raw_info).expect("verified public metadata");
    assert_eq!(
        hex(&metainfo.info_hash),
        "dd8255ecdc7ca55fb0bbf81323d87062db1f6d1c"
    );
}

#[tokio::test]
#[ignore = "uses changing public Mainline DHT and swarm state"]
async fn live_big_buck_bunny_trackerless_dht_metadata_probe() {
    let expected_info_hash = "dd8255ecdc7ca55fb0bbf81323d87062db1f6d1c";
    let dht = DhtService::start(DhtConfig::for_network(NetworkPolicy::Online))
        .await
        .expect("start public DHT");
    let control = DownloadControl::new();
    let activity = Arc::new(RecordingActivitySink::default());
    control.set_activity_sink(activity.clone());
    let task_control = control.clone();
    let dht_handle = dht.handle();
    let identity = test_identity(
        Magnet::parse(&format!("magnet:?xt=urn:btih:{expected_info_hash}"))
            .expect("valid magnet")
            .identity
            .swarm_key()
            .into_bytes(),
    );
    let mut task = tokio::spawn(async move {
        download_magnet_metadata_with_dht(
            identity,
            format!("magnet:?xt=urn:btih:{expected_info_hash}"),
            NetworkConfig::new(
                NetworkPolicy::Online,
                Duration::from_secs(15),
                Duration::from_secs(30),
            ),
            task_control,
            Some(dht_handle),
            PeerBudget::system_default(),
        )
        .await
    });
    let monitor_control = control.clone();
    let monitor = tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(10)).await;
            let snapshot = monitor_control.diagnostic_snapshot().metadata;
            let registry = snapshot.registry.as_ref().map(|registry| registry.counts);
            eprintln!(
                "public DHT metadata probe: elapsed={:?} phase={:?} registry={registry:?} \
                     pending_dials={} active_workers={} attempts={} requests={} blocks={} bytes={} \
                     active={} recent={} dropped={} last_error={:?}",
                snapshot.captured_at,
                snapshot.phase,
                snapshot.pending_dials,
                snapshot.active_workers,
                snapshot.total_attempts,
                snapshot.total_requests_sent,
                snapshot.total_blocks_received,
                snapshot.total_bytes_received,
                snapshot.active_attempts.len(),
                snapshot.recent_attempts.len(),
                snapshot.recent_attempts_dropped,
                snapshot.last_error,
            );
        }
    });

    let raw_info = match timeout(Duration::from_secs(120), &mut task).await {
        Ok(result) => {
            monitor.abort();
            let _ = monitor.await;
            let raw_info = result
                .expect("join public DHT metadata probe")
                .expect("acquire public DHT metadata");
            let stats = dht.handle().stats().await.ok();
            eprintln!(
                "public DHT metadata probe completed; stats={stats:?}:\n{:#?}",
                control.diagnostic_snapshot()
            );
            raw_info
        }
        Err(_) => {
            monitor.abort();
            let _ = monitor.await;
            let stats = dht.handle().stats().await.ok();
            let timeout_snapshot = control.diagnostic_snapshot();
            let events = activity
                .events
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .clone();
            eprintln!("public DHT metadata timeout snapshot:\n{timeout_snapshot:#?}");
            eprintln!("public DHT metadata activity:\n{events:#?}");
            control.cancel();
            if timeout(Duration::from_secs(5), &mut task).await.is_err() {
                task.abort();
                let _ = task.await;
            }
            dht.shutdown().await.expect("DHT shutdown after timeout");
            panic!("public trackerless DHT metadata probe exceeded 120 seconds; stats={stats:?}");
        }
    };
    dht.shutdown().await.expect("public DHT shutdown");
    let metainfo = Metainfo::from_info_bytes(&raw_info).expect("verified public metadata");
    assert_eq!(hex(&metainfo.info_hash), expected_info_hash);
}

#[tokio::test]
async fn tracker_only_magnet_discovers_registry_peers_and_downloads() {
    let payload = b"tracker-discovered payload".to_vec();
    let info = single_file_info(&payload);
    let info_hash: [u8; 20] = Sha1::digest(&info).into();
    let output_path = test_path("tracker-magnet-output.bin");

    let unreachable_listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind unreachable peer placeholder");
    let unreachable = unreachable_listener
        .local_addr()
        .expect("unreachable peer address");
    drop(unreachable_listener);

    let peer_listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind tracker-discovered peer");
    let reachable = peer_listener.local_addr().expect("peer address");
    let peer_task = tokio::spawn(serve_metadata_then_piece(
        peer_listener,
        info,
        payload.clone(),
        vec![0x80],
    ));

    let tracker_socket = UdpSocket::bind("127.0.0.1:0")
        .await
        .expect("bind scripted UDP tracker");
    let tracker_address = tracker_socket.local_addr().expect("tracker address");
    let tracker_task = tokio::spawn(serve_one_shot_udp_tracker(
        tracker_socket,
        info_hash,
        unreachable,
        reachable,
        Duration::ZERO,
    ));
    let rejecting_tracker = UdpSocket::bind("127.0.0.1:0")
        .await
        .expect("bind rejecting UDP tracker");
    let rejecting_tracker_address = rejecting_tracker
        .local_addr()
        .expect("rejecting tracker address");
    let rejecting_tracker_task = tokio::spawn(serve_rejecting_udp_tracker(rejecting_tracker));

    let magnet = format!(
        "magnet:?xt=urn:btih:{}&tr=udp%3A%2F%2F{rejecting_tracker_address}&\
             tr=udp%3A%2F%2F{tracker_address}%2Fannounce",
        hex(&info_hash)
    );
    let parsed = Magnet::parse(&magnet).expect("parse tracker magnet");
    assert!(parsed.peer_hints.is_empty());
    assert_eq!(parsed.trackers.len(), 2);
    let network = loopback_network(Duration::from_secs(2));
    let control = DownloadControl::new();
    let mut peers = TorrentPeerCoordinator::from_magnet(&parsed, network, control.clone(), None)
        .await
        .expect("prepare tracker discovery");
    assert!(peers.registry_is_empty());

    let report = run_magnet_download_with_peers(
        MagnetDownloadConfig {
            identity: test_identity(info_hash),
            magnet,
            output_path: output_path.clone(),
            network,
            resource_limits: resource_limits(MIN_PAYLOAD_ALLOWANCE),
            skip_files: Vec::new(),
            high_priority_files: Vec::new(),
            dht: None,
        },
        control,
        parsed,
        &mut peers,
    )
    .await
    .expect("tracker-discovered magnet download");

    assert_eq!(peers.registry_len(), 2);
    let failed = peers
        .peers
        .with_state(|state| {
            state
                .registry
                .find_endpoint(PeerEndpoint::new(unreachable).expect("failed endpoint"))
                .cloned()
        })
        .expect("failed tracker peer retained");
    assert_eq!(failed.history().total_failures, 1);
    assert_eq!(failed.history().last_failure, Some(PeerFailure::Connect));
    assert!(failed.sources().contains(PeerSource::Tracker));
    let succeeded = peers
        .peers
        .with_state(|state| {
            state
                .registry
                .find_endpoint(PeerEndpoint::new(reachable).expect("successful endpoint"))
                .cloned()
        })
        .expect("successful tracker peer retained");
    assert_eq!(succeeded.history().total_failures, 0);
    assert!(succeeded.history().last_connected_at.is_some());
    assert!(succeeded.history().last_disconnected_at.is_some());
    assert!(succeeded.sources().contains(PeerSource::Tracker));

    assert_eq!(report.info_hash, info_hash);
    assert_eq!(
        tokio::fs::read(&output_path)
            .await
            .expect("direct tracker output"),
        payload
    );
    peers
        .shutdown_tracker()
        .await
        .expect("stop tracker manager");
    if rejecting_tracker_task.is_finished() {
        rejecting_tracker_task
            .await
            .expect("rejecting tracker task");
    } else {
        rejecting_tracker_task.abort();
        let _ = rejecting_tracker_task.await;
    }
    tracker_task.await.expect("scripted tracker task");
    peer_task.await.expect("scripted peer task");
    let _ = tokio::fs::remove_file(output_path).await;
}

#[tokio::test]
async fn http_tracker_only_magnet_discovers_peer_and_verifies_download() {
    let payload = b"HTTP tracker-discovered payload".to_vec();
    let info = single_file_info(&payload);
    let info_hash: [u8; 20] = Sha1::digest(&info).into();
    let output_path = test_path("http-tracker-magnet-output.bin");

    let peer_listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind HTTP tracker-discovered peer");
    let peer_address = peer_listener.local_addr().expect("peer address");
    let peer_task = tokio::spawn(serve_metadata_then_piece(
        peer_listener,
        info,
        payload.clone(),
        vec![0x80],
    ));

    let tracker_listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind scripted HTTP tracker");
    let tracker_address = tracker_listener.local_addr().expect("tracker address");
    let tracker_task = tokio::spawn(serve_http_tracker_lifecycle(
        tracker_listener,
        info_hash,
        peer_address,
    ));
    let magnet = format!(
        "magnet:?xt=urn:btih:{}&tr=http%3A%2F%2F{tracker_address}%2Fannounce%2Fprivate-token%3Fpasskey%3Dfixture",
        hex(&info_hash)
    );
    let parsed = Magnet::parse(&magnet).expect("parse HTTP tracker magnet");
    assert!(parsed.peer_hints.is_empty());
    assert_eq!(parsed.trackers.len(), 1);
    assert_eq!(
        parsed.trackers[0].transport(),
        rstorrent_protocol::magnet::TrackerUrlTransport::Http
    );

    let network = loopback_network(Duration::from_secs(2));
    let control = DownloadControl::new();
    let activity = Arc::new(RecordingActivitySink::default());
    control.set_activity_sink(activity.clone());
    let mut peers = TorrentPeerCoordinator::from_magnet(&parsed, network, control.clone(), None)
        .await
        .expect("prepare HTTP tracker discovery");
    assert!(peers.registry_is_empty());

    let report = run_magnet_download_with_peers(
        MagnetDownloadConfig {
            identity: test_identity(info_hash),
            magnet,
            output_path: output_path.clone(),
            network,
            resource_limits: resource_limits(MIN_PAYLOAD_ALLOWANCE),
            skip_files: Vec::new(),
            high_priority_files: Vec::new(),
            dht: None,
        },
        control,
        parsed,
        &mut peers,
    )
    .await
    .expect("HTTP tracker-discovered magnet download");

    assert_eq!(peers.registry_len(), 1);
    assert_eq!(report.info_hash, info_hash);
    assert_eq!(
        tokio::fs::read(&output_path)
            .await
            .expect("direct HTTP tracker output"),
        payload
    );
    assert!(peers.peers.with_state(|state| {
        state
            .registry
            .find_endpoint(PeerEndpoint::new(peer_address).expect("HTTP tracker endpoint"))
            .is_some_and(|record| record.sources().contains(PeerSource::Tracker))
    }));
    let rendered_activity = format!(
        "{:?}",
        activity
            .events
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    );
    assert!(!rendered_activity.contains("passkey=fixture"));

    peers
        .finish_tracker()
        .await
        .expect("finish HTTP tracker manager");
    let request_targets = tracker_task.await.expect("scripted HTTP tracker task");
    assert_eq!(request_targets.len(), 3);
    assert!(
        request_targets
            .iter()
            .all(|target| target.starts_with("/announce/private-token?passkey=fixture&"))
    );
    assert!(request_targets[0].contains("info_hash="));
    assert!(request_targets[0].contains("peer_id="));
    assert!(request_targets[0].contains("event=started"));
    assert!(request_targets[1].contains("event=completed"));
    assert!(request_targets[2].contains("event=stopped"));
    assert!(request_targets[1].contains("numwant=200"));
    assert!(request_targets[2].contains("numwant=0"));
    assert!(
        request_targets[1..]
            .iter()
            .all(|target| target.contains("trackerid=%66%69%78%74%75%72%65"))
    );
    peer_task.await.expect("scripted peer task");
    let _ = tokio::fs::remove_file(output_path).await;
}

#[tokio::test]
async fn direct_http_failure_falls_back_to_udp_tracker() {
    let info_hash = [0x5a; 20];
    let peer = SocketAddr::from(([127, 0, 0, 1], 42_424));
    let http_listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind failing HTTP tracker");
    let http_address = http_listener.local_addr().expect("HTTP tracker address");
    let http_task = tokio::spawn(serve_declared_failure_http_tracker(http_listener));
    let udp_socket = UdpSocket::bind("127.0.0.1:0")
        .await
        .expect("bind fallback UDP tracker");
    let udp_address = udp_socket.local_addr().expect("UDP tracker address");
    let udp_task = tokio::spawn(serve_one_shot_udp_tracker(
        udp_socket,
        info_hash,
        peer,
        peer,
        Duration::ZERO,
    ));
    let http_url = format!("http://{http_address}/announce?passkey=fixture");
    let trackers = vec![
        TrackerConfig {
            url: http_url.clone(),
            endpoint: TrackerEndpoint::from_http_url(&http_url).expect("HTTP tracker endpoint"),
            tier: 0,
            position: 0,
            source: TrackerSource::Metainfo,
        },
        TrackerConfig {
            url: format!("udp://{udp_address}"),
            endpoint: TrackerEndpoint::Udp(UdpTrackerUrl {
                host: udp_address.ip().to_string(),
                port: udp_address.port(),
            }),
            tier: 1,
            position: 0,
            source: TrackerSource::Metainfo,
        },
    ];
    let control = DownloadControl::new();
    let activity = Arc::new(RecordingActivitySink::default());
    control.set_activity_sink(activity.clone());
    let mut manager = TrackerManager::start_with_configs(
        trackers,
        info_hash,
        loopback_network(Duration::from_secs(1)),
        control,
    )
    .expect("start mixed direct tracker manager");

    let (tracker, peers) = timeout(Duration::from_secs(2), manager.next_peers())
        .await
        .expect("mixed tracker fallback deadline")
        .expect("UDP fallback result");
    assert_eq!(tracker, format!("udp://{udp_address}"));
    assert!(peers.contains(&peer));
    {
        let events = activity
            .events
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        assert!(events.iter().any(|event| matches!(
            event,
            DownloadActivityEvent::TrackerAnnounceFailed { tracker, detail, .. }
                if tracker == &format!("http://{http_address}")
                    && detail == "controlled tracker failure"
        )));
        assert!(events.iter().any(|event| matches!(
            event,
            DownloadActivityEvent::TrackerFallbackSelected { tracker, tier: 1 }
                if tracker == &format!("udp://{udp_address}")
        )));
    }

    manager
        .shutdown()
        .await
        .expect("stop mixed tracker manager");
    http_task.await.expect("failing HTTP tracker task");
    udp_task.await.expect("fallback UDP tracker task");
}

#[tokio::test]
async fn direct_http_cancellation_joins_and_closes_socket() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind stalled HTTP tracker");
    let tracker_address = listener.local_addr().expect("tracker address");
    let request_seen = Arc::new(Notify::new());
    let server_seen = request_seen.clone();
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener
            .accept()
            .await
            .expect("accept stalled HTTP tracker");
        read_test_http_headers(&mut stream).await;
        server_seen.notify_one();
        let mut request = [0_u8; 4096];
        let closed = timeout(Duration::from_secs(1), stream.read(&mut request))
            .await
            .expect("cancelled HTTP tracker socket closes")
            .expect("read cancelled tracker socket");
        assert_eq!(closed, 0);
    });
    let url = format!("http://{tracker_address}/announce");
    let manager = TrackerManager::start_with_configs(
        vec![TrackerConfig {
            url: url.clone(),
            endpoint: TrackerEndpoint::from_http_url(&url).expect("HTTP tracker endpoint"),
            tier: 0,
            position: 0,
            source: TrackerSource::Magnet,
        }],
        [0x6b; 20],
        loopback_network(Duration::from_secs(1)),
        DownloadControl::new(),
    )
    .expect("start stalled HTTP tracker manager");

    timeout(Duration::from_secs(1), request_seen.notified())
        .await
        .expect("stalled HTTP request starts");
    timeout(Duration::from_secs(1), manager.shutdown())
        .await
        .expect("HTTP tracker cancellation deadline")
        .expect("join HTTP tracker manager");
    server.await.expect("stalled HTTP tracker task");
}

#[tokio::test]
async fn finalizing_an_exhausted_tracker_owner_is_clean() {
    let mut manager = TrackerManager::start_with_configs(
        Vec::new(),
        [0x7c; 20],
        loopback_network(Duration::from_secs(1)),
        DownloadControl::new(),
    )
    .expect("start empty tracker manager");

    let result = timeout(Duration::from_secs(1), manager.next_peers())
        .await
        .expect("empty tracker manager deadline");
    assert!(matches!(result, Err(DownloadError::TrackerTask(_))));
    timeout(Duration::from_secs(1), manager.finish())
        .await
        .expect("exhausted tracker finalization deadline")
        .expect("join exhausted tracker manager");
}

#[cfg(feature = "test-platform-root")]
#[tokio::test]
#[ignore = "opt-in pinned-libtorrent direct HTTPS interoperability harness"]
async fn authenticated_https_tracker_introduces_pinned_libtorrent_peer() {
    let tracker_url = std::env::var("RSTORRENT_INTEROP_TRACKER_URL")
        .expect("RSTORRENT_INTEROP_TRACKER_URL is required");
    assert!(tracker_url.starts_with("https://127.0.0.1:"));
    let info_hash = decode_test_info_hash(
        &std::env::var("RSTORRENT_INTEROP_INFO_HASH")
            .expect("RSTORRENT_INTEROP_INFO_HASH is required"),
    );
    let root_pem = std::fs::read(
        std::env::var("RSTORRENT_INTEROP_ROOT_PEM")
            .expect("RSTORRENT_INTEROP_ROOT_PEM is required"),
    )
    .expect("read controlled root certificate");
    crate::http_tracker::install_test_platform_root(&root_pem)
        .expect("install one test-only platform root");
    let storage_root = PathBuf::from(
        std::env::var("RSTORRENT_INTEROP_DIRECT_ROOT")
            .expect("RSTORRENT_INTEROP_DIRECT_ROOT is required"),
    );
    let magnet = format!(
        "magnet:?xt=urn:btih:{}&tr={}",
        hex(&info_hash),
        percent_encode_test_magnet_value(&tracker_url)
    );
    let network = loopback_network(Duration::from_secs(15));
    let report = timeout(
        Duration::from_secs(60),
        resume_magnet_with_control(
            ResumableMagnetDownloadConfig {
                identity: test_identity(info_hash),
                magnet,
                storage_root,
                network,
                peer_budget: PeerBudget::system_default(),
                mse_dh: crate::mse::MseDhWorkOwner::new(),
                encryption: crate::network::PeerEncryptionPolicyHandle::new(network.encryption),
                torrent_peers: None,
                resource_limits: DownloadResourceLimits::DESKTOP,
                skip_files: Vec::new(),
                high_priority_files: Vec::new(),
                verified_info: None,
                verified_pieces: Vec::new(),
                resume_validation: crate::resume_validation::ResumeValidationIntent::FastEligible,
                download_missing: true,
                dht: None,
                trackers: None,
            },
            Arc::new(RecordingCheckpointSink::default()),
            DownloadControl::new(),
        ),
    )
    .await
    .expect("authenticated direct HTTPS transfer deadline")
    .expect("authenticated direct HTTPS transfer");
    assert_eq!(report.info_hash, info_hash);
    assert_eq!(report.verified_piece_count, report.piece_count);
    assert_ne!(report.bytes_written, 0);
}

#[cfg(feature = "test-platform-root")]
#[tokio::test]
#[ignore = "opt-in controlled untrusted direct HTTPS harness"]
async fn system_trust_rejects_untrusted_https_before_http() {
    let tracker_url = std::env::var("RSTORRENT_INTEROP_TRACKER_URL")
        .expect("RSTORRENT_INTEROP_TRACKER_URL is required");
    assert!(tracker_url.starts_with("https://127.0.0.1:"));
    let control = DownloadControl::new();
    let activity = Arc::new(RecordingActivitySink::default());
    control.set_activity_sink(activity.clone());
    let manager = TrackerManager::start_with_configs(
        vec![TrackerConfig {
            url: tracker_url.clone(),
            endpoint: TrackerEndpoint::from_http_url(&tracker_url)
                .expect("controlled HTTPS tracker URL"),
            tier: 0,
            position: 0,
            source: TrackerSource::Magnet,
        }],
        [0x7c; 20],
        loopback_network(Duration::from_secs(2)),
        control,
    )
    .expect("start direct HTTPS tracker manager");
    timeout(Duration::from_secs(5), async {
        loop {
            if activity
                .events
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .iter()
                .any(|event| matches!(event, DownloadActivityEvent::TrackerAnnounceFailed { .. }))
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("system-trust failure deadline");
    let rendered = format!(
        "{:?}",
        activity
            .events
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    );
    assert!(rendered.contains("TLS failure") || rendered.contains("certificate"));
    assert!(!rendered.contains("passkey=fixture"));
    manager
        .shutdown()
        .await
        .expect("stop rejected HTTPS manager");
}

#[cfg(feature = "test-platform-root")]
fn decode_test_info_hash(value: &str) -> [u8; 20] {
    assert_eq!(value.len(), 40);
    let mut result = [0_u8; 20];
    for (index, byte) in result.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16)
            .expect("hexadecimal info hash");
    }
    result
}

#[cfg(feature = "test-platform-root")]
fn percent_encode_test_magnet_value(value: &str) -> String {
    value
        .bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                char::from(byte).to_string()
            }
            _ => format!("%{byte:02X}"),
        })
        .collect()
}

#[tokio::test]
async fn initial_tracker_operations_start_concurrently_and_merge_results() {
    let barrier = Arc::new(Barrier::new(3));
    let mut tracker_addresses = Vec::new();
    let mut servers = Vec::new();
    for offset in 0..3_u16 {
        let tracker = UdpSocket::bind("127.0.0.1:0")
            .await
            .expect("bind concurrent tracker");
        tracker_addresses.push(tracker.local_addr().expect("concurrent tracker address"));
        servers.push(tokio::spawn(serve_barrier_udp_tracker(
            tracker,
            barrier.clone(),
            41_000 + offset,
        )));
    }
    let trackers = tracker_addresses
        .iter()
        .map(|address| format!("&tr=udp%3A%2F%2F{address}"))
        .collect::<String>();
    let magnet = Magnet::parse(&format!(
        "magnet:?xt=urn:btih:{}{trackers}",
        "00".repeat(20)
    ))
    .expect("parse concurrent tracker magnet");
    let control = DownloadControl::new();
    let activity = Arc::new(RecordingActivitySink::default());
    control.set_activity_sink(activity.clone());
    let mut peers = TorrentPeerCoordinator::from_magnet(
        &magnet,
        loopback_network(Duration::from_secs(1)),
        control,
        None,
    )
    .await
    .expect("start concurrent trackers");

    timeout(Duration::from_secs(2), async {
        for _ in 0..3 {
            peers
                .receive_tracker_peers()
                .await
                .expect("receive concurrent tracker peers");
        }
    })
    .await
    .expect("concurrent tracker result deadline");

    assert_eq!(peers.registry_len(), 3);
    let succeeded = {
        let events = activity
            .events
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        events
            .iter()
            .filter(|event| {
                matches!(
                    event,
                    DownloadActivityEvent::TrackerAnnounceSucceeded { peer_count: 1, .. }
                )
            })
            .count()
    };
    assert_eq!(succeeded, 3);

    peers
        .shutdown_tracker()
        .await
        .expect("stop concurrent trackers");
    for server in servers {
        server.await.expect("concurrent tracker server");
    }
}

#[tokio::test]
async fn initial_tracker_operations_hold_the_ceiling_and_advance_on_failure() {
    let tracker_count = super::MAX_CONCURRENT_TRACKER_OPERATIONS + 1;
    let started = Arc::new(AtomicUsize::new(0));
    let release = Arc::new(Semaphore::new(0));
    let mut tracker_addresses = Vec::new();
    let mut servers = Vec::new();
    for offset in 0..tracker_count {
        let tracker = UdpSocket::bind("127.0.0.1:0")
            .await
            .expect("bind bounded startup tracker");
        tracker_addresses.push(tracker.local_addr().expect("bounded tracker address"));
        servers.push(tokio::spawn(serve_bounded_startup_tracker(
            tracker,
            started.clone(),
            release.clone(),
            42_000 + u16::try_from(offset).expect("bounded peer port"),
        )));
    }
    let trackers = tracker_addresses
        .iter()
        .map(|address| format!("&tr=udp%3A%2F%2F{address}"))
        .collect::<String>();
    let magnet = Magnet::parse(&format!(
        "magnet:?xt=urn:btih:{}{trackers}",
        "00".repeat(20)
    ))
    .expect("parse bounded tracker magnet");
    let mut peers = TorrentPeerCoordinator::from_magnet(
        &magnet,
        loopback_network(Duration::from_secs(1)),
        DownloadControl::new(),
        None,
    )
    .await
    .expect("start bounded trackers");

    timeout(Duration::from_secs(1), async {
        while started.load(Ordering::Acquire) < super::MAX_CONCURRENT_TRACKER_OPERATIONS {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("fill tracker operation ceiling");
    sleep(Duration::from_millis(25)).await;
    assert_eq!(
        started.load(Ordering::Acquire),
        super::MAX_CONCURRENT_TRACKER_OPERATIONS
    );
    release.add_permits(tracker_count);

    timeout(Duration::from_secs(2), peers.receive_tracker_peers())
        .await
        .expect("bounded tracker result deadline")
        .expect("last startup tracker succeeds");
    assert_eq!(started.load(Ordering::Acquire), tracker_count);
    assert_eq!(peers.registry_len(), 1);

    peers
        .shutdown_tracker()
        .await
        .expect("stop bounded trackers");
    let mut successes = 0;
    for server in servers {
        successes += usize::from(server.await.expect("bounded tracker server"));
    }
    assert_eq!(successes, 1);
}

#[tokio::test]
async fn concurrent_tracker_cancellation_joins_and_releases_every_socket() {
    let mut trackers = Vec::new();
    let mut tracker_addresses = Vec::new();
    for _ in 0..3 {
        let tracker = UdpSocket::bind("127.0.0.1:0")
            .await
            .expect("bind silent concurrent tracker");
        tracker_addresses.push(
            tracker
                .local_addr()
                .expect("silent concurrent tracker address"),
        );
        trackers.push(tracker);
    }
    let tracker_parameters = tracker_addresses
        .iter()
        .map(|address| format!("&tr=udp%3A%2F%2F{address}"))
        .collect::<String>();
    let magnet = Magnet::parse(&format!(
        "magnet:?xt=urn:btih:{}{tracker_parameters}",
        "00".repeat(20)
    ))
    .expect("parse silent concurrent trackers");
    let control = DownloadControl::new();
    let activity = Arc::new(RecordingActivitySink::default());
    control.set_activity_sink(activity.clone());
    let manager = TrackerManager::start(
        magnet
            .trackers
            .iter()
            .filter_map(|tracker| tracker.udp_endpoint().cloned())
            .collect(),
        magnet.identity.swarm_key().into_bytes(),
        loopback_network(Duration::from_secs(1)),
        control,
    )
    .expect("start silent concurrent trackers");
    let mut client_addresses = Vec::new();
    for tracker in &trackers {
        let mut packet = [0; 32];
        let (length, client) = timeout(Duration::from_secs(1), tracker.recv_from(&mut packet))
            .await
            .expect("concurrent connect deadline")
            .expect("receive concurrent connect");
        assert_eq!(length, 16);
        client_addresses.push(client);
    }

    manager
        .shutdown()
        .await
        .expect("shutdown concurrent tracker manager");
    {
        let events = activity
            .events
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        assert!(events.iter().any(|event| matches!(
            event,
            DownloadActivityEvent::TrackerState(snapshot)
                if snapshot.active
                    && snapshot.records.iter().any(|record| matches!(
                        record.status,
                        crate::TrackerRuntimeStatus::Announcing
                    ))
        )));
        let terminal = events.iter().rev().find_map(|event| match event {
            DownloadActivityEvent::TrackerState(snapshot) => Some(snapshot),
            _ => None,
        });
        assert!(terminal.is_some_and(|snapshot| {
            !snapshot.active
                && snapshot
                    .records
                    .iter()
                    .all(|record| matches!(record.status, crate::TrackerRuntimeStatus::Inactive))
        }));
    }
    for client in client_addresses {
        UdpSocket::bind(client)
            .await
            .expect("concurrent tracker client socket released");
    }
}

#[tokio::test]
async fn zero_peer_success_waits_for_reannounce_without_tracker_failure() {
    let tracker = UdpSocket::bind("127.0.0.1:0")
        .await
        .expect("bind empty tracker");
    let tracker_address = tracker.local_addr().expect("empty tracker address");
    let server = tokio::spawn(serve_empty_udp_tracker(tracker));
    let magnet = Magnet::parse(&format!(
        "magnet:?xt=urn:btih:{}&tr=udp%3A%2F%2F{tracker_address}",
        "00".repeat(20)
    ))
    .expect("parse empty tracker magnet");
    let control = DownloadControl::new();
    let activity = Arc::new(RecordingActivitySink::default());
    control.set_activity_sink(activity.clone());
    let mut peers = TorrentPeerCoordinator::from_magnet(
        &magnet,
        loopback_network(Duration::from_secs(1)),
        control,
        None,
    )
    .await
    .expect("start empty tracker");

    timeout(Duration::from_secs(1), peers.receive_tracker_peers())
        .await
        .expect("empty tracker result deadline")
        .expect("valid empty tracker result");
    assert!(peers.registry_is_empty());
    timeout(Duration::from_secs(1), async {
        loop {
            let has_reannounce = activity
                .events
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .iter()
                .any(|event| {
                    matches!(
                        event,
                        DownloadActivityEvent::TrackerReannounceScheduled { .. }
                    )
                });
            if has_reannounce {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("reannounce diagnostic deadline");
    {
        let events = activity
            .events
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        assert!(events.iter().any(|event| matches!(
            event,
            DownloadActivityEvent::TrackerAnnounceSucceeded { peer_count: 0, .. }
        )));
        assert!(events.iter().any(|event| matches!(
            event,
            DownloadActivityEvent::TrackerPeersUnavailable { peer_count: 0, .. }
        )));
        assert!(
            !events
                .iter()
                .any(|event| matches!(event, DownloadActivityEvent::TrackerAnnounceFailed { .. }))
        );
    }
    peers.shutdown_tracker().await.expect("stop empty tracker");
    server.await.expect("empty tracker server");
}

#[test]
fn udp_tracker_tokens_expire_after_the_protocol_lifetime() {
    let address = "127.0.0.1:6969".parse().expect("tracker address");
    let inserted_at = Instant::now();
    let mut tokens = UdpTrackerTokenCache::default();
    tokens.insert(address, 42, inserted_at);

    assert_eq!(
        tokens.get(address, inserted_at + Duration::from_secs(59)),
        Some(42)
    );
    assert_eq!(
        tokens.get(address, inserted_at + Duration::from_secs(60)),
        None
    );
}

#[tokio::test]
async fn udp_tracker_retransmits_reuses_token_and_cancels_cleanly() {
    let tracker = UdpSocket::bind("127.0.0.1:0")
        .await
        .expect("bind scripted tracker");
    let tracker_address = tracker.local_addr().expect("tracker address");
    let announced_port = 41_234;
    let server = tokio::spawn(async move {
        let mut packet = [0; 256];

        let (first_connect, first_client) =
            timeout(Duration::from_secs(1), tracker.recv_from(&mut packet))
                .await
                .expect("first connect deadline")
                .expect("first connect");
        assert_eq!(first_connect, 16);
        let connect_transaction =
            u32::from_be_bytes(packet[12..16].try_into().expect("connect transaction"));

        let (second_connect, second_client) =
            timeout(Duration::from_secs(1), tracker.recv_from(&mut packet))
                .await
                .expect("retransmitted connect deadline")
                .expect("retransmitted connect");
        assert_eq!(second_connect, 16);
        assert_eq!(second_client, first_client);
        assert_eq!(
            u32::from_be_bytes(packet[12..16].try_into().expect("connect transaction")),
            connect_transaction
        );
        let connection_id = 0x0102_0304_0506_0708_u64;
        let mut connect_response = Vec::from(0_u32.to_be_bytes());
        connect_response.extend_from_slice(&connect_transaction.to_be_bytes());
        connect_response.extend_from_slice(&connection_id.to_be_bytes());
        tracker
            .send_to(&connect_response, first_client)
            .await
            .expect("connect response");

        let (first_announce, announce_client) =
            timeout(Duration::from_secs(1), tracker.recv_from(&mut packet))
                .await
                .expect("first announce deadline")
                .expect("first announce");
        assert_eq!(first_announce, 98);
        assert_eq!(
            u32::from_be_bytes(packet[80..84].try_into().expect("started event")),
            AnnounceEvent::Started as u32
        );
        assert_eq!(
            u16::from_be_bytes(packet[96..98].try_into().expect("announced port")),
            announced_port
        );
        let announce_transaction =
            u32::from_be_bytes(packet[12..16].try_into().expect("announce transaction"));

        let (second_announce, second_announce_client) =
            timeout(Duration::from_secs(1), tracker.recv_from(&mut packet))
                .await
                .expect("retransmitted announce deadline")
                .expect("retransmitted announce");
        assert_eq!(second_announce, 98);
        assert_eq!(second_announce_client, announce_client);
        assert_eq!(
            u32::from_be_bytes(packet[12..16].try_into().expect("announce transaction")),
            announce_transaction
        );
        assert_eq!(
            u16::from_be_bytes(packet[96..98].try_into().expect("announced port")),
            announced_port
        );
        let mut announce_response = Vec::from(1_u32.to_be_bytes());
        announce_response.extend_from_slice(&announce_transaction.to_be_bytes());
        announce_response.extend_from_slice(&600_u32.to_be_bytes());
        announce_response.extend_from_slice(&0_u32.to_be_bytes());
        announce_response.extend_from_slice(&0_u32.to_be_bytes());
        tracker
            .send_to(&announce_response, announce_client)
            .await
            .expect("first announce response");

        let (cached_announce, cached_client) =
            timeout(Duration::from_secs(1), tracker.recv_from(&mut packet))
                .await
                .expect("cached announce deadline")
                .expect("cached announce");
        assert_eq!(cached_announce, 98, "cached token should skip connect");
        assert_eq!(
            u64::from_be_bytes(packet[0..8].try_into().expect("connection ID")),
            connection_id
        );
        assert_eq!(
            u32::from_be_bytes(packet[80..84].try_into().expect("ordinary event")),
            AnnounceEvent::None as u32
        );
        assert_eq!(
            u16::from_be_bytes(packet[96..98].try_into().expect("announced port")),
            announced_port
        );
        let cached_transaction =
            u32::from_be_bytes(packet[12..16].try_into().expect("announce transaction"));
        announce_response[4..8].copy_from_slice(&cached_transaction.to_be_bytes());
        tracker
            .send_to(&announce_response, cached_client)
            .await
            .expect("cached announce response");
    });

    let timing = UdpTrackerTiming {
        retransmit_after: Duration::from_millis(20),
        completion_timeout: Duration::from_millis(100),
    };
    let control = DownloadControl::new();
    let activity = Arc::new(RecordingActivitySink::default());
    control.set_activity_sink(activity.clone());
    let mut tokens = UdpTrackerTokenCache::default();
    let first = announce_udp_tracker_address(
        tracker_address,
        &mut tokens,
        UdpTrackerAnnounce {
            info_hash: [7; 20],
            peer_id: CLIENT_PEER_ID,
            key: 1,
            downloaded: 0,
            left: 16 * 1024,
            uploaded: 0,
            event: AnnounceEvent::Started,
            num_want: 200,
            port: announced_port,
            ipv6_port: announced_port,
        },
        UdpTrackerExchange {
            timing,
            control: &control,
            tracker_label: "udp://127.0.0.1",
            source_ipv4: None,
            source_ipv6: None,
        },
    )
    .await
    .expect("loss-recovered announce");
    assert!(first.response.peers.is_empty());
    assert_eq!(first.connection_family, TrackerConnectionFamily::Ipv4);
    let second = announce_udp_tracker_address(
        tracker_address,
        &mut tokens,
        UdpTrackerAnnounce {
            info_hash: [7; 20],
            peer_id: CLIENT_PEER_ID,
            key: 1,
            downloaded: 0,
            left: 16 * 1024,
            uploaded: 0,
            event: AnnounceEvent::None,
            num_want: 200,
            port: announced_port,
            ipv6_port: announced_port,
        },
        UdpTrackerExchange {
            timing: UdpTrackerTiming {
                retransmit_after: Duration::from_millis(200),
                completion_timeout: Duration::from_secs(1),
            },
            control: &control,
            tracker_label: "udp://127.0.0.1",
            source_ipv4: None,
            source_ipv6: None,
        },
    )
    .await
    .expect("cached-token announce");
    assert!(second.response.peers.is_empty());
    assert_eq!(second.connection_family, TrackerConnectionFamily::Ipv4);
    server.await.expect("scripted tracker");

    let retransmissions = {
        let events = activity
            .events
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        events
            .iter()
            .filter(|event| matches!(event, DownloadActivityEvent::TrackerUdpRetransmitted { .. }))
            .count()
    };
    assert_eq!(retransmissions, 2);

    assert_tracker_wait_cancels_without_socket_leaks().await;
}

#[tokio::test]
async fn ipv6_udp_tracker_uses_selected_source_and_family_port() {
    let tracker = UdpSocket::bind("[::1]:0")
        .await
        .expect("bind IPv6 UDP tracker");
    let tracker_address = tracker.local_addr().expect("IPv6 tracker address");
    let server = tokio::spawn(async move {
        let mut packet = [0_u8; 256];
        let (connect_length, source) = tracker.recv_from(&mut packet).await.unwrap();
        assert_eq!(connect_length, 16);
        let connect_transaction = u32::from_be_bytes(packet[12..16].try_into().unwrap());
        let mut connect_response = Vec::with_capacity(16);
        connect_response.extend_from_slice(&0_u32.to_be_bytes());
        connect_response.extend_from_slice(&connect_transaction.to_be_bytes());
        connect_response.extend_from_slice(&0x1020_3040_5060_7080_u64.to_be_bytes());
        tracker.send_to(&connect_response, source).await.unwrap();

        let (announce_length, announce_source) = tracker.recv_from(&mut packet).await.unwrap();
        assert_eq!(announce_length, 98);
        let announce_transaction = u32::from_be_bytes(packet[12..16].try_into().unwrap());
        let announced_port = u16::from_be_bytes(packet[96..98].try_into().unwrap());
        let mut announce_response = Vec::with_capacity(20);
        announce_response.extend_from_slice(&1_u32.to_be_bytes());
        announce_response.extend_from_slice(&announce_transaction.to_be_bytes());
        announce_response.extend_from_slice(&900_u32.to_be_bytes());
        announce_response.extend_from_slice(&0_u32.to_be_bytes());
        announce_response.extend_from_slice(&0_u32.to_be_bytes());
        tracker
            .send_to(&announce_response, announce_source)
            .await
            .unwrap();
        (announce_source, announced_port)
    });

    let control = DownloadControl::new();
    let mut tokens = UdpTrackerTokenCache::default();
    let result = announce_udp_tracker_address(
        tracker_address,
        &mut tokens,
        UdpTrackerAnnounce {
            info_hash: [8; 20],
            peer_id: CLIENT_PEER_ID,
            key: 2,
            downloaded: 0,
            left: 1,
            uploaded: 0,
            event: AnnounceEvent::Started,
            num_want: 0,
            port: 41_004,
            ipv6_port: 41_006,
        },
        UdpTrackerExchange {
            timing: UdpTrackerTiming {
                retransmit_after: Duration::from_millis(100),
                completion_timeout: Duration::from_secs(1),
            },
            control: &control,
            tracker_label: "udp://[::1]",
            source_ipv4: None,
            source_ipv6: Some(Ipv6Addr::LOCALHOST.into()),
        },
    )
    .await
    .expect("IPv6 UDP announce");
    assert_eq!(result.connection_family, TrackerConnectionFamily::Ipv6);
    let (source, port) = server.await.expect("IPv6 tracker task");
    assert_eq!(source.ip(), IpAddr::V6(Ipv6Addr::LOCALHOST));
    assert_eq!(port, 41_006);
}

#[tokio::test]
async fn ipv4_only_policy_rejects_an_ipv6_udp_tracker_before_io() {
    let control = DownloadControl::new();
    let mut tokens = UdpTrackerTokenCache::default();
    let result = announce_udp_tracker(
        &UdpTrackerUrl {
            host: "::1".to_owned(),
            port: 49_001,
        },
        NetworkPolicy::LoopbackOnly,
        AddressFamilyPolicy::ipv4_only(),
        &mut tokens,
        UdpTrackerAnnounce {
            info_hash: [9; 20],
            peer_id: CLIENT_PEER_ID,
            key: 3,
            downloaded: 0,
            left: 1,
            uploaded: 0,
            event: AnnounceEvent::Started,
            num_want: 0,
            port: 41_004,
            ipv6_port: 41_006,
        },
        UdpTrackerExchange {
            timing: UdpTrackerTiming {
                retransmit_after: Duration::from_millis(20),
                completion_timeout: Duration::from_millis(100),
            },
            control: &control,
            tracker_label: "udp://[::1]",
            source_ipv4: None,
            source_ipv6: None,
        },
    )
    .await;
    assert!(matches!(result, Err(DownloadError::NoUsableTrackerAddress)));
}

#[tokio::test]
async fn stalled_metadata_peer_does_not_delay_useful_peer() {
    let payload = b"parallel verified metadata".to_vec();
    let info = single_file_info(&payload);
    let info_hash: [u8; 20] = Sha1::digest(&info).into();
    let stalled_listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind stalled metadata peer");
    let stalled_address = stalled_listener
        .local_addr()
        .expect("stalled metadata address");
    let stalled_task = tokio::spawn(serve_stalled_metadata_peer(
        stalled_listener,
        info_hash,
        info.len(),
    ));
    let useful_listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind useful metadata peer");
    let useful_address = useful_listener
        .local_addr()
        .expect("useful metadata address");
    let useful_task = tokio::spawn(serve_metadata_then_piece(
        useful_listener,
        info,
        payload.clone(),
        vec![0x80],
    ));
    let magnet = format!(
        "magnet:?xt=urn:btih:{}&x.pe={stalled_address}&x.pe={useful_address}",
        hex(&info_hash)
    );
    let parsed = Magnet::parse(&magnet).expect("parse parallel metadata magnet");
    let network = loopback_network(Duration::from_secs(5));
    let mut peers =
        TorrentPeerCoordinator::from_magnet(&parsed, network, DownloadControl::new(), None)
            .await
            .expect("resolve metadata peers");

    let (raw_info, metainfo) = timeout(
        Duration::from_secs(4),
        peers.acquire_metadata(
            info_hash,
            DownloadResourceLimits::DESKTOP.metadata_connections,
        ),
    )
    .await
    .expect("stalled metadata peer must not set the completion deadline")
    .expect("useful metadata peer supplies verified metadata");

    assert_eq!(raw_info, single_file_info(&payload));
    assert_eq!(metainfo.v1().expect("v1 metadata").info_hash, info_hash);
    let stalled = peers
        .peers
        .with_state(|state| {
            state
                .registry
                .find_endpoint(PeerEndpoint::new(stalled_address).expect("stalled endpoint"))
                .cloned()
        })
        .expect("stalled peer retained");
    assert_eq!(stalled.phase(), PeerPhase::Idle);
    assert_eq!(stalled.history().dial_attempts, 1);
    assert_eq!(stalled.history().total_failures, 0);
    peers.close_current(None).expect("close metadata winner");
    for task in [stalled_task, useful_task] {
        timeout(Duration::from_secs(1), task)
            .await
            .expect("metadata peer joined")
            .expect("metadata peer task");
    }
}

#[tokio::test]
async fn metadata_cancellation_publishes_empty_peers_after_joined_cleanup() {
    let payload = b"cancelled metadata owner".to_vec();
    let info = single_file_info(&payload);
    let info_hash: [u8; 20] = Sha1::digest(&info).into();
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind cancelled metadata peer");
    let address = listener.local_addr().expect("metadata peer address");
    let peer_task = tokio::spawn(serve_stalled_metadata_peer(listener, info_hash, info.len()));
    let control = DownloadControl::new();
    let activity = Arc::new(RecordingActivitySink::default());
    control.set_activity_sink(activity.clone());
    let task_control = control.clone();
    let task = tokio::spawn(download_magnet_metadata_with_control(
        test_identity(info_hash),
        format!("magnet:?xt=urn:btih:{}&x.pe={address}", hex(&info_hash)),
        loopback_network(Duration::from_secs(5)),
        task_control,
    ));

    timeout(Duration::from_secs(1), async {
        loop {
            let diagnostics = control.diagnostic_snapshot();
            if diagnostics
                .peer_connections
                .iter()
                .any(|peer| peer.lifecycle == PeerConnectionLifecycle::Connected)
                && diagnostics.metadata.total_requests_sent > 0
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("metadata peer reached connected state");

    control.cancel();
    let result = timeout(Duration::from_secs(1), task)
        .await
        .expect("metadata cancellation joined")
        .expect("metadata task");
    assert!(matches!(result, Err(DownloadError::Cancelled)));
    timeout(Duration::from_secs(1), peer_task)
        .await
        .expect("metadata peer closed before terminal result")
        .expect("metadata peer task");

    let diagnostics = control.diagnostic_snapshot();
    assert!(diagnostics.peer_connections.is_empty());
    assert_eq!(diagnostics.metadata.pending_dials, 0);
    assert_eq!(diagnostics.metadata.active_workers, 0);
    let events = activity
        .events
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let peer_snapshots = events
        .iter()
        .filter_map(|event| match event {
            DownloadActivityEvent::PeerConnections { peers, .. } => Some(peers.as_slice()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(peer_snapshots.iter().any(|peers| {
        peers
            .iter()
            .any(|peer| peer.lifecycle == PeerConnectionLifecycle::Connected)
    }));
    assert!(peer_snapshots.iter().any(|peers| {
        peers
            .iter()
            .any(|peer| peer.lifecycle == PeerConnectionLifecycle::Disconnecting)
    }));
    assert!(peer_snapshots.last().is_some_and(|peers| peers.is_empty()));
}

#[tokio::test]
async fn metadata_default_cohort_is_paced_to_thirty_and_cancels_exactly() {
    let payload = b"paced metadata cohort".to_vec();
    let info = single_file_info(&payload);
    let info_hash: [u8; 20] = Sha1::digest(&info).into();
    let mut magnet = format!("magnet:?xt=urn:btih:{}", hex(&info_hash));
    let mut peer_tasks = Vec::new();
    for _ in 0..=MAX_METADATA_PEERS {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind paced metadata peer");
        let address = listener.local_addr().expect("paced metadata address");
        magnet.push_str(&format!("&x.pe={address}"));
        peer_tasks.push(tokio::spawn(serve_idle_metadata_peer(
            listener,
            info_hash,
            info.len(),
        )));
    }

    let control = DownloadControl::new();
    let task_control = control.clone();
    let peer_budget = crate::PeerBudget::new(crate::PeerBudgetConfig {
        configured_limit: MAX_METADATA_PEERS,
        incoming_slack: 0,
        max_open_files: 1_024,
    });
    let task_budget = peer_budget.clone();
    let task = tokio::spawn(download_magnet_metadata_with_dht(
        test_identity(info_hash),
        magnet,
        loopback_network(Duration::from_secs(6)),
        task_control,
        None,
        task_budget,
    ));

    timeout(Duration::from_secs(5), async {
        loop {
            let metadata = control.diagnostic_snapshot().metadata;
            if metadata.pending_dials + metadata.active_workers == MAX_METADATA_PEERS {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("paced metadata cohort reaches its default bound");

    let active = control.diagnostic_snapshot().metadata;
    assert_eq!(active.total_attempts, MAX_METADATA_PEERS);
    assert_eq!(active.active_attempts.len(), MAX_METADATA_PEERS);
    let registry = active.registry.expect("paced metadata registry");
    assert_eq!(registry.counts.total, MAX_METADATA_PEERS + 1);
    assert_eq!(registry.counts.eligible, 1);
    assert_eq!(peer_budget.snapshot().total, MAX_METADATA_PEERS);
    assert_eq!(peer_budget.snapshot().total_high_water, MAX_METADATA_PEERS);

    control.cancel();
    let result = timeout(Duration::from_secs(2), task)
        .await
        .expect("paced metadata cancellation joins")
        .expect("paced metadata task");
    assert!(matches!(result, Err(DownloadError::Cancelled)));
    assert_eq!(peer_budget.snapshot().total, 0);
    let terminal = control.diagnostic_snapshot().metadata;
    assert_eq!(terminal.pending_dials, 0);
    assert_eq!(terminal.active_workers, 0);
    assert!(terminal.active_attempts.is_empty());

    timeout(Duration::from_secs(2), async {
        loop {
            if peer_tasks.iter().filter(|task| task.is_finished()).count() == MAX_METADATA_PEERS {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("every admitted remote metadata peer observes closure");
    assert_eq!(
        peer_tasks.iter().filter(|task| !task.is_finished()).count(),
        1
    );
    for task in &peer_tasks {
        if !task.is_finished() {
            task.abort();
        }
    }
    for task in peer_tasks {
        match task.await {
            Ok(()) => {}
            Err(error) => assert!(error.is_cancelled()),
        }
    }
}

#[tokio::test]
async fn saturated_metadata_cohort_replaces_one_zero_contributor_and_protects_progress() {
    let payload = vec![0x6b; 1_700];
    let info = single_file_info_with_piece_length(&payload, 1);
    assert!(
        info.len() > 2 * rstorrent_protocol::metadata::METADATA_BLOCK_LENGTH,
        "fixture must span at least three metadata blocks"
    );
    let info_hash: [u8; 20] = Sha1::digest(&info).into();
    let mut listeners = Vec::new();
    for _ in 0..=MAX_METADATA_PEERS {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind turnover metadata peer");
        listeners.push(listener);
    }
    listeners.sort_by_key(|listener| listener.local_addr().expect("metadata address"));
    let useful_listener = listeners.pop().expect("useful listener");
    let contributing_listener = listeners.remove(0);
    let contributing_address = contributing_listener
        .local_addr()
        .expect("contributing address");
    let useful_address = useful_listener.local_addr().expect("useful address");
    let mut magnet = format!(
        "magnet:?xt=urn:btih:{}&x.pe={contributing_address}",
        hex(&info_hash)
    );
    let contributing_task = tokio::spawn(serve_one_block_then_idle_metadata_peer(
        contributing_listener,
        info.clone(),
    ));
    let mut idle_tasks = Vec::new();
    for listener in listeners {
        let address = listener.local_addr().expect("idle address");
        magnet.push_str(&format!("&x.pe={address}"));
        idle_tasks.push(tokio::spawn(serve_idle_metadata_peer(
            listener,
            info_hash,
            info.len(),
        )));
    }
    magnet.push_str(&format!("&x.pe={useful_address}"));
    let useful_task = tokio::spawn(serve_metadata_bytes_after_delay_with_timeout(
        useful_listener,
        info_hash,
        info.clone(),
        Duration::ZERO,
        Duration::from_secs(8),
    ));
    let parsed = Magnet::parse(&magnet).expect("parse turnover magnet");
    let control = DownloadControl::new();
    let mut peers = TorrentPeerCoordinator::from_magnet(
        &parsed,
        loopback_network(Duration::from_secs(6)),
        control.clone(),
        None,
    )
    .await
    .expect("resolve turnover peers");
    let limits = MetadataConnectionLimits::DEFAULT
        .with_saturated_no_progress_grace(Duration::from_millis(250));

    let acquisition = timeout(
        Duration::from_secs(12),
        peers.acquire_metadata(info_hash, limits),
    )
    .await
    .unwrap_or_else(|_| {
        panic!(
            "saturated turnover completion bound: {:#?}",
            control.diagnostic_snapshot().metadata
        )
    });
    let (raw_info, _) = acquisition.expect("replacement candidate completes metadata");
    assert_eq!(raw_info, info);
    let snapshot = control.diagnostic_snapshot().metadata;
    assert_eq!(snapshot.phase, MetadataAcquisitionPhase::Complete);
    assert_eq!(snapshot.total_attempts, MAX_METADATA_PEERS + 1);
    assert_eq!(snapshot.total_blocks_received, 3);
    let replaced = snapshot
        .recent_attempts
        .iter()
        .filter(|peer| {
            peer.terminal_detail.as_deref()
                == Some("metadata peer replaced after saturated no-progress grace")
        })
        .collect::<Vec<_>>();
    assert_eq!(replaced.len(), 1);
    assert_eq!(replaced[0].blocks_received, 0);
    assert!(snapshot.recent_attempts.iter().any(|peer| {
        peer.blocks_received == 1
            && peer.terminal_detail.as_deref()
                != Some("metadata peer replaced after saturated no-progress grace")
    }));
    assert!(
        snapshot
            .recent_attempts
            .iter()
            .any(|peer| { peer.stage == MetadataPeerStage::Complete && peer.blocks_received == 2 })
    );
    assert_eq!(snapshot.pending_dials, 0);
    assert_eq!(snapshot.active_workers, 0);
    assert!(snapshot.active_attempts.is_empty());

    peers.close_current(None).expect("close metadata winner");
    for task in idle_tasks
        .into_iter()
        .chain([contributing_task, useful_task])
    {
        timeout(Duration::from_secs(1), task)
            .await
            .expect("turnover fixture joined")
            .expect("turnover fixture task");
    }
}

#[tokio::test]
async fn sparse_metadata_swarm_does_not_turn_over_without_a_replacement() {
    let payload = b"sparse metadata turnover protection".to_vec();
    let info = single_file_info(&payload);
    let info_hash: [u8; 20] = Sha1::digest(&info).into();
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind sparse metadata peer");
    let address = listener.local_addr().expect("sparse metadata address");
    let peer_task = tokio::spawn(serve_idle_metadata_peer(listener, info_hash, info.len()));
    let magnet = Magnet::parse(&format!(
        "magnet:?xt=urn:btih:{}&x.pe={address}",
        hex(&info_hash)
    ))
    .expect("parse sparse metadata magnet");
    let control = DownloadControl::new();
    let peers = TorrentPeerCoordinator::from_magnet(
        &magnet,
        loopback_network(Duration::from_secs(3)),
        control.clone(),
        None,
    )
    .await
    .expect("resolve sparse metadata peer");
    let limits = MetadataConnectionLimits::DEFAULT
        .with_saturated_no_progress_grace(Duration::from_millis(100));
    let task_control = control.clone();
    let task = tokio::spawn(async move {
        let mut peers = peers;
        let result = peers.acquire_metadata(info_hash, limits).await;
        (peers, result)
    });

    timeout(Duration::from_secs(1), async {
        loop {
            if control.diagnostic_snapshot().metadata.active_workers == 1 {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("sparse metadata worker connects");
    tokio::time::sleep(Duration::from_millis(350)).await;
    let active = control.diagnostic_snapshot().metadata;
    assert_eq!(active.total_attempts, 1);
    assert_eq!(active.active_workers, 1);
    assert!(active.recent_attempts.iter().all(|peer| {
        peer.terminal_detail.as_deref()
            != Some("metadata peer replaced after saturated no-progress grace")
    }));

    task_control.cancel();
    let (peers, result) = timeout(Duration::from_secs(1), task)
        .await
        .expect("sparse cancellation joins")
        .expect("sparse metadata task");
    assert!(matches!(result, Err(DownloadError::Cancelled)));
    drop(peers);
    let terminal = control.diagnostic_snapshot().metadata;
    assert_eq!(terminal.pending_dials, 0);
    assert_eq!(terminal.active_workers, 0);
    assert!(terminal.active_attempts.is_empty());
    timeout(Duration::from_secs(1), peer_task)
        .await
        .expect("sparse peer observes closure")
        .expect("sparse peer task");
}

#[tokio::test]
async fn metadata_blocks_from_multiple_peers_complete_one_dictionary() {
    let payload = vec![0x5a; 1_700];
    let info = single_file_info_with_piece_length(&payload, 1);
    assert!(
        info.len() > 2 * rstorrent_protocol::metadata::METADATA_BLOCK_LENGTH,
        "fixture must span three metadata blocks"
    );
    let info_hash: [u8; 20] = Sha1::digest(&info).into();
    let partial_listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind partial metadata peer");
    let partial_address = partial_listener.local_addr().expect("partial address");
    let complete_listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind complementary metadata peer");
    let complete_address = complete_listener.local_addr().expect("complete address");
    let partial_task = tokio::spawn(serve_partial_metadata_peer(
        partial_listener,
        info.clone(),
        true,
    ));
    let complete_task = tokio::spawn(serve_partial_metadata_peer(
        complete_listener,
        info.clone(),
        false,
    ));
    let magnet = format!(
        "magnet:?xt=urn:btih:{}&x.pe={partial_address}&x.pe={complete_address}",
        hex(&info_hash)
    );
    let parsed = Magnet::parse(&magnet).expect("parse multi-source metadata magnet");
    let control = DownloadControl::new();
    let mut peers = TorrentPeerCoordinator::from_magnet(
        &parsed,
        loopback_network(Duration::from_secs(2)),
        control.clone(),
        None,
    )
    .await
    .expect("resolve multi-source metadata peers");

    let (raw_info, metainfo) = timeout(
        Duration::from_secs(3),
        peers.acquire_metadata(
            info_hash,
            DownloadResourceLimits::DESKTOP.metadata_connections,
        ),
    )
    .await
    .expect("multi-source metadata completion bound")
    .expect("combine metadata blocks across peers");
    assert_eq!(raw_info, info);
    assert_eq!(metainfo.v1().expect("v1 metadata").info_hash, info_hash);
    let snapshot = control.diagnostic_snapshot().metadata;
    assert_eq!(snapshot.total_blocks_received, 3);
    assert!(
        snapshot
            .recent_attempts
            .iter()
            .filter(|peer| peer.blocks_received > 0)
            .count()
            >= 2
    );

    peers.close_current(None).expect("close metadata winner");
    for task in [partial_task, complete_task] {
        timeout(Duration::from_secs(1), task)
            .await
            .expect("multi-source peer joined")
            .expect("multi-source peer task");
    }
}

#[tokio::test]
async fn corrupt_metadata_generation_resets_before_clean_peer_completes() {
    let payload = vec![0x39; 1_700];
    let info = single_file_info_with_piece_length(&payload, 1);
    assert!(
        info.len() > 2 * rstorrent_protocol::metadata::METADATA_BLOCK_LENGTH,
        "fixture must span three metadata blocks"
    );
    let info_hash: [u8; 20] = Sha1::digest(&info).into();
    let mut corrupt = info.clone();
    corrupt[rstorrent_protocol::metadata::METADATA_BLOCK_LENGTH + 7] ^= 0x01;

    let corrupt_listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind corrupt metadata peer");
    let corrupt_address = corrupt_listener.local_addr().expect("corrupt address");
    let clean_listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind clean metadata peer");
    let clean_address = clean_listener.local_addr().expect("clean address");
    let corrupt_task = tokio::spawn(serve_metadata_bytes_after_delay(
        corrupt_listener,
        info_hash,
        corrupt,
        Duration::ZERO,
    ));
    let clean_task = tokio::spawn(serve_metadata_bytes_after_delay(
        clean_listener,
        info_hash,
        info.clone(),
        Duration::from_millis(200),
    ));
    let magnet = format!(
        "magnet:?xt=urn:btih:{}&x.pe={corrupt_address}&x.pe={clean_address}",
        hex(&info_hash)
    );
    let parsed = Magnet::parse(&magnet).expect("parse corrupt recovery magnet");
    let control = DownloadControl::new();
    let mut peers = TorrentPeerCoordinator::from_magnet(
        &parsed,
        loopback_network(Duration::from_secs(2)),
        control.clone(),
        None,
    )
    .await
    .expect("resolve corrupt recovery peers");

    let (raw_info, metainfo) = timeout(
        Duration::from_secs(3),
        peers.acquire_metadata(
            info_hash,
            DownloadResourceLimits::DESKTOP.metadata_connections,
        ),
    )
    .await
    .expect("corrupt metadata recovery bound")
    .expect("clean source completes after corrupt generation");
    assert_eq!(raw_info, info);
    assert_eq!(metainfo.v1().expect("v1 metadata").info_hash, info_hash);
    let snapshot = control.diagnostic_snapshot().metadata;
    assert_eq!(snapshot.total_hash_failures, 1);
    assert_eq!(snapshot.last_hash_failure_contributors, 1);
    assert_eq!(snapshot.total_blocks_received, 6);

    peers
        .close_current(None)
        .expect("close clean metadata winner");
    for task in [corrupt_task, clean_task] {
        timeout(Duration::from_secs(1), task)
            .await
            .expect("corrupt recovery peer joined")
            .expect("corrupt recovery peer task");
    }
}

#[tokio::test]
async fn metadata_requests_ramp_for_one_at_a_time_peer() {
    let payload = vec![0x71; 1_000];
    let info = single_file_info_with_piece_length(&payload, 1);
    assert!(
        info.len() > rstorrent_protocol::metadata::METADATA_BLOCK_LENGTH
            && info.len() <= 2 * rstorrent_protocol::metadata::METADATA_BLOCK_LENGTH,
        "fixture must span exactly two metadata blocks"
    );
    let info_hash: [u8; 20] = Sha1::digest(&info).into();
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind one-at-a-time metadata peer");
    let address = listener.local_addr().expect("one-at-a-time address");
    let server = tokio::spawn(serve_one_at_a_time_metadata_peer(listener, info.clone()));
    let magnet = format!("magnet:?xt=urn:btih:{}&x.pe={address}", hex(&info_hash));
    let parsed = Magnet::parse(&magnet).expect("parse one-at-a-time magnet");
    let control = DownloadControl::new();
    let mut peers = TorrentPeerCoordinator::from_magnet(
        &parsed,
        loopback_network(Duration::from_secs(2)),
        control.clone(),
        None,
    )
    .await
    .expect("resolve one-at-a-time peer");

    let (raw_info, metainfo) = timeout(
        Duration::from_secs(2),
        peers.acquire_metadata(
            info_hash,
            DownloadResourceLimits::DESKTOP.metadata_connections,
        ),
    )
    .await
    .expect("one-at-a-time metadata completion bound")
    .expect("pace requests until first response");
    assert_eq!(raw_info, info);
    assert_eq!(metainfo.v1().expect("v1 metadata").info_hash, info_hash);
    let snapshot = control.diagnostic_snapshot().metadata;
    assert_eq!(snapshot.total_requests_sent, 2);
    assert_eq!(snapshot.total_blocks_received, 2);

    peers.close_current(None).expect("close metadata winner");
    timeout(Duration::from_secs(1), server)
        .await
        .expect("one-at-a-time peer joined")
        .expect("one-at-a-time peer task");
}

#[tokio::test]
async fn peers_without_ut_metadata_release_slots_and_remain_diagnosable() {
    let payload = b"diagnosable metadata failover".to_vec();
    let info = single_file_info(&payload);
    let info_hash: [u8; 20] = Sha1::digest(&info).into();
    let mut missing_addresses = Vec::new();
    let mut missing_tasks = Vec::new();
    for _ in 0..MAX_METADATA_PEERS {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind metadata-incapable peer");
        missing_addresses.push(listener.local_addr().expect("missing metadata address"));
        missing_tasks.push(tokio::spawn(serve_metadata_peer_without_ut_metadata(
            listener, info_hash,
        )));
    }
    let useful_listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind useful metadata peer");
    let useful_address = useful_listener
        .local_addr()
        .expect("useful metadata address");
    let useful_task = tokio::spawn(serve_metadata_then_piece(
        useful_listener,
        info.clone(),
        payload,
        vec![0x80],
    ));
    let mut magnet = format!("magnet:?xt=urn:btih:{}", hex(&info_hash));
    for address in &missing_addresses {
        magnet.push_str(&format!("&x.pe={address}"));
    }
    magnet.push_str(&format!("&x.pe={useful_address}"));
    let parsed = Magnet::parse(&magnet).expect("parse diagnostic metadata magnet");
    let control = DownloadControl::new();
    let mut peers = TorrentPeerCoordinator::from_magnet(
        &parsed,
        loopback_network(Duration::from_secs(1)),
        control.clone(),
        None,
    )
    .await
    .expect("resolve diagnostic metadata peers");

    let (raw_info, _) = timeout(
        Duration::from_secs(5),
        peers.acquire_metadata(
            info_hash,
            DownloadResourceLimits::DESKTOP.metadata_connections,
        ),
    )
    .await
    .expect("metadata-incapable peers must release all slots")
    .expect("later useful peer supplies metadata");
    assert_eq!(raw_info, info);

    let snapshot = control.diagnostic_snapshot().metadata;
    assert_eq!(snapshot.phase, MetadataAcquisitionPhase::Complete);
    assert_eq!(snapshot.total_attempts, MAX_METADATA_PEERS + 1);
    assert_eq!(snapshot.total_requests_sent, 1);
    assert_eq!(snapshot.total_blocks_received, 1);
    assert_eq!(snapshot.active_attempts, Vec::new());
    assert_eq!(
        snapshot
            .recent_attempts
            .iter()
            .filter(|peer| peer.stage == MetadataPeerStage::Failed)
            .count(),
        MAX_METADATA_PEERS
    );
    assert!(snapshot.recent_attempts.iter().any(|peer| {
        peer.stage == MetadataPeerStage::Complete
            && peer.remote_metadata_id == Some(UT_METADATA_LOCAL_ID)
            && peer.blocks_received == 1
    }));
    assert!(
        snapshot
            .recent_attempts
            .iter()
            .filter(|peer| {
                peer.terminal_detail
                    .as_deref()
                    .is_some_and(|detail| detail.contains("does not advertise"))
            })
            .count()
            >= MAX_METADATA_PEERS
    );
    let registry = snapshot.registry.expect("peer registry snapshot");
    assert_eq!(registry.counts.total, MAX_METADATA_PEERS + 1);

    peers.close_current(None).expect("close metadata winner");
    for task in missing_tasks.into_iter().chain([useful_task]) {
        timeout(Duration::from_secs(1), task)
            .await
            .expect("metadata fixture joined")
            .expect("metadata fixture task");
    }
}

#[tokio::test]
async fn unrelated_messages_cannot_hold_every_metadata_slot() {
    let payload = b"metadata after bounded chatter".to_vec();
    let info = single_file_info(&payload);
    let info_hash: [u8; 20] = Sha1::digest(&info).into();
    let mut chatter_addresses = Vec::new();
    let mut chatter_tasks = Vec::new();
    for _ in 0..MAX_METADATA_PEERS {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind chattering peer");
        chatter_addresses.push(listener.local_addr().expect("chattering peer address"));
        chatter_tasks.push(tokio::spawn(
            serve_chattering_peer_without_extension_handshake(listener, info_hash),
        ));
    }
    let useful_listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind useful metadata peer");
    let useful_address = useful_listener
        .local_addr()
        .expect("useful metadata address");
    let useful_task = tokio::spawn(serve_metadata_then_piece(
        useful_listener,
        info.clone(),
        payload,
        vec![0x80],
    ));
    let mut magnet = format!("magnet:?xt=urn:btih:{}", hex(&info_hash));
    for address in &chatter_addresses {
        magnet.push_str(&format!("&x.pe={address}"));
    }
    magnet.push_str(&format!("&x.pe={useful_address}"));
    let parsed = Magnet::parse(&magnet).expect("parse chattering metadata magnet");
    let control = DownloadControl::new();
    let mut peers = TorrentPeerCoordinator::from_magnet(
        &parsed,
        loopback_network(Duration::from_millis(150)),
        control.clone(),
        None,
    )
    .await
    .expect("resolve chattering metadata peers");

    let (raw_info, _) = timeout(
        Duration::from_secs(6),
        peers.acquire_metadata(
            info_hash,
            DownloadResourceLimits::DESKTOP.metadata_connections,
        ),
    )
    .await
    .expect("metadata progress deadline releases chattering peers")
    .expect("later useful peer supplies metadata");
    assert_eq!(raw_info, info);
    let snapshot = control.diagnostic_snapshot().metadata;
    assert_eq!(snapshot.phase, MetadataAcquisitionPhase::Complete);
    assert_eq!(snapshot.total_attempts, MAX_METADATA_PEERS + 1);
    let metadata_timeouts = snapshot
        .recent_attempts
        .iter()
        .filter_map(|peer| peer.terminal_detail.as_deref())
        .filter(|detail| detail.contains("metadata progress timed out"))
        .count();
    assert!(
        metadata_timeouts >= MAX_METADATA_PEERS - 2,
        "expected all but the bounded overlap to time out, observed {metadata_timeouts}"
    );
    assert_eq!(
        snapshot
            .recent_attempts
            .iter()
            .filter(|peer| matches!(
                peer.stage,
                MetadataPeerStage::Failed | MetadataPeerStage::Cancelled
            ))
            .count(),
        MAX_METADATA_PEERS
    );
    assert!(
        snapshot
            .recent_attempts
            .iter()
            .any(|peer| { peer.stage == MetadataPeerStage::Complete && peer.blocks_received == 1 })
    );

    peers.close_current(None).expect("close metadata winner");
    for task in chatter_tasks.into_iter().chain([useful_task]) {
        timeout(Duration::from_secs(1), task)
            .await
            .expect("metadata fixture joined")
            .expect("metadata fixture task");
    }
}

#[tokio::test]
async fn metadata_rejections_release_slots_and_are_counted() {
    let payload = b"metadata after explicit rejects".to_vec();
    let info = single_file_info(&payload);
    let info_hash: [u8; 20] = Sha1::digest(&info).into();
    let mut rejecting_addresses = Vec::new();
    let mut rejecting_tasks = Vec::new();
    for _ in 0..MAX_METADATA_PEERS {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind rejecting peer");
        rejecting_addresses.push(listener.local_addr().expect("rejecting peer address"));
        rejecting_tasks.push(tokio::spawn(serve_metadata_rejecting_peer(
            listener,
            info_hash,
            info.len(),
        )));
    }
    let useful_listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind useful metadata peer");
    let useful_address = useful_listener
        .local_addr()
        .expect("useful metadata address");
    let useful_task = tokio::spawn(serve_metadata_then_piece(
        useful_listener,
        info.clone(),
        payload,
        vec![0x80],
    ));
    let mut magnet = format!("magnet:?xt=urn:btih:{}", hex(&info_hash));
    for address in &rejecting_addresses {
        magnet.push_str(&format!("&x.pe={address}"));
    }
    magnet.push_str(&format!("&x.pe={useful_address}"));
    let parsed = Magnet::parse(&magnet).expect("parse rejecting metadata magnet");
    let control = DownloadControl::new();
    let mut peers = TorrentPeerCoordinator::from_magnet(
        &parsed,
        loopback_network(Duration::from_secs(1)),
        control.clone(),
        None,
    )
    .await
    .expect("resolve rejecting metadata peers");

    let (raw_info, _) = timeout(
        Duration::from_secs(5),
        peers.acquire_metadata(
            info_hash,
            DownloadResourceLimits::DESKTOP.metadata_connections,
        ),
    )
    .await
    .expect("rejecting peers must release all slots")
    .expect("later useful peer supplies metadata");
    assert_eq!(raw_info, info);
    let snapshot = control.diagnostic_snapshot().metadata;
    assert_eq!(snapshot.phase, MetadataAcquisitionPhase::Complete);
    assert_eq!(snapshot.total_attempts, MAX_METADATA_PEERS + 1);
    let rejected_requests = snapshot
        .recent_attempts
        .iter()
        .map(|peer| peer.rejects_received)
        .sum::<usize>();
    assert!((1..=MAX_METADATA_PEERS).contains(&rejected_requests));
    assert!(
        snapshot
            .recent_attempts
            .iter()
            .any(|peer| { peer.stage == MetadataPeerStage::Complete && peer.blocks_received == 1 })
    );

    peers.close_current(None).expect("close metadata winner");
    for task in rejecting_tasks.into_iter().chain([useful_task]) {
        timeout(Duration::from_secs(1), task)
            .await
            .expect("metadata fixture joined")
            .expect("metadata fixture task");
    }
}

#[tokio::test]
async fn tracker_discovery_continues_while_metadata_peer_stalls() {
    let payload = b"late tracker metadata".to_vec();
    let info = single_file_info(&payload);
    let info_hash: [u8; 20] = Sha1::digest(&info).into();
    let stalled_listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind stalled metadata peer");
    let stalled_address = stalled_listener
        .local_addr()
        .expect("stalled metadata address");
    let stalled_task = tokio::spawn(serve_stalled_metadata_peer(
        stalled_listener,
        info_hash,
        info.len(),
    ));
    let useful_listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind tracker metadata peer");
    let useful_address = useful_listener
        .local_addr()
        .expect("tracker metadata address");
    let useful_task = tokio::spawn(serve_metadata_then_piece(
        useful_listener,
        info,
        payload,
        vec![0x80],
    ));
    let unavailable_listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind unavailable placeholder");
    let unavailable = unavailable_listener
        .local_addr()
        .expect("unavailable address");
    drop(unavailable_listener);
    let tracker = UdpSocket::bind("127.0.0.1:0")
        .await
        .expect("bind delayed tracker");
    let tracker_address = tracker.local_addr().expect("tracker address");
    let tracker_task = tokio::spawn(serve_one_shot_udp_tracker(
        tracker,
        info_hash,
        unavailable,
        useful_address,
        Duration::from_millis(100),
    ));
    let magnet = Magnet::parse(&format!(
        "magnet:?xt=urn:btih:{}&x.pe={stalled_address}&\
             tr=udp%3A%2F%2F{tracker_address}%2Fannounce",
        hex(&info_hash)
    ))
    .expect("parse late metadata discovery magnet");
    let mut peers = TorrentPeerCoordinator::from_magnet(
        &magnet,
        loopback_network(Duration::from_secs(2)),
        DownloadControl::new(),
        None,
    )
    .await
    .expect("start metadata discovery");

    let (_, metainfo) = timeout(
        Duration::from_secs(4),
        peers.acquire_metadata(
            info_hash,
            DownloadResourceLimits::DESKTOP.metadata_connections,
        ),
    )
    .await
    .expect("late tracker peer must be consumed during metadata work")
    .expect("tracker peer supplies metadata");

    assert_eq!(metainfo.v1().expect("v1 metadata").info_hash, info_hash);
    let discovered = peers
        .peers
        .with_state(|state| {
            state
                .registry
                .find_endpoint(PeerEndpoint::new(useful_address).expect("tracker endpoint"))
                .cloned()
        })
        .expect("tracker peer retained");
    assert!(discovered.sources().contains(PeerSource::Tracker));
    peers.close_current(None).expect("close metadata winner");
    peers
        .shutdown_tracker()
        .await
        .expect("shutdown metadata tracker");
    for task in [stalled_task, useful_task, tracker_task] {
        timeout(Duration::from_secs(1), task)
            .await
            .expect("metadata fixture joined")
            .expect("metadata fixture task");
    }
}

#[tokio::test]
async fn magnet_registry_fails_over_and_hands_same_peer_to_content_download() {
    let payload = b"verified magnet payload".to_vec();
    let info = single_file_info(&payload);
    let info_hash: [u8; 20] = Sha1::digest(&info).into();
    let output_path = test_path("magnet-output.bin");
    let unreachable_listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind unreachable peer placeholder");
    let unreachable = unreachable_listener
        .local_addr()
        .expect("unreachable peer address");
    drop(unreachable_listener);
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind scripted metadata peer");
    let address = listener.local_addr().expect("listener address");
    let peer_task = tokio::spawn(serve_metadata_then_piece(
        listener,
        info,
        payload.clone(),
        vec![0x80],
    ));

    let magnet = format!(
        "magnet:?xt=urn:btih:{}&x.pe={unreachable}&x.pe={address}",
        hex(&info_hash)
    );
    let parsed = Magnet::parse(&magnet).expect("parse failover magnet");
    let network = loopback_network(Duration::from_secs(2));
    let mut peers =
        TorrentPeerCoordinator::from_magnet(&parsed, network, DownloadControl::new(), None)
            .await
            .expect("resolve failover peers");
    assert_eq!(peers.registry_len(), 2);

    let report = run_magnet_download_with_peers(
        MagnetDownloadConfig {
            identity: test_identity(info_hash),
            magnet,
            output_path: output_path.clone(),
            network,
            resource_limits: resource_limits(MIN_PAYLOAD_ALLOWANCE),
            skip_files: Vec::new(),
            high_priority_files: Vec::new(),
            dht: None,
        },
        DownloadControl::new(),
        parsed,
        &mut peers,
    )
    .await
    .expect("magnet metadata and content after failover");

    let failed = peers
        .peers
        .with_state(|state| {
            state
                .registry
                .find_endpoint(PeerEndpoint::new(unreachable).expect("failed endpoint"))
                .cloned()
        })
        .expect("failed peer record retained");
    assert_eq!(failed.phase(), PeerPhase::Idle);
    assert_eq!(failed.history().dial_attempts, 1);
    assert_eq!(failed.history().total_failures, 1);
    assert_eq!(failed.history().last_failure, Some(PeerFailure::Connect));
    assert!(failed.history().retry_at.is_some());
    assert!(failed.sources().contains(PeerSource::MagnetHint));

    let connected = peers
        .peers
        .with_state(|state| {
            state
                .registry
                .find_endpoint(PeerEndpoint::new(address).expect("connected endpoint"))
                .cloned()
        })
        .expect("connected peer record retained");
    assert_eq!(connected.phase(), PeerPhase::Idle);
    assert_eq!(connected.history().dial_attempts, 1);
    assert_eq!(connected.history().total_failures, 0);
    assert!(connected.history().last_connected_at.is_some());
    assert!(connected.history().last_disconnected_at.is_some());
    assert!(connected.sources().contains(PeerSource::MagnetHint));

    assert_eq!(report.info_hash, info_hash);
    assert_eq!(
        tokio::fs::read(&output_path).await.expect("direct output"),
        payload
    );
    peer_task.await.expect("scripted peer task");
    let _ = tokio::fs::remove_file(output_path).await;
}

#[tokio::test]
async fn public_magnet_entry_starts_tracker_and_uses_peer_registry_path() {
    let payload = b"public entry payload".to_vec();
    let info = single_file_info(&payload);
    let info_hash: [u8; 20] = Sha1::digest(&info).into();
    let output_path = test_path("public-magnet-output.bin");
    let unsupported_listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind non-extension peer");
    let unsupported_address = unsupported_listener
        .local_addr()
        .expect("non-extension peer address");
    let unsupported_task = tokio::spawn(async move {
        let (mut stream, _) = unsupported_listener
            .accept()
            .await
            .expect("accept magnet client");
        let mut handshake_bytes = [0; HANDSHAKE_LENGTH];
        stream
            .read_exact(&mut handshake_bytes)
            .await
            .expect("read magnet handshake");
        assert!(
            decode_handshake(&handshake_bytes, info_hash)
                .expect("valid client handshake")
                .supports_extensions()
        );
        stream
            .write_all(&encode_handshake_with_reserved(
                info_hash,
                *b"-RS-NOEXT-0000000000",
                [0; 8],
            ))
            .await
            .expect("send non-extension handshake");
    });
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind scripted metadata peer");
    let address = listener.local_addr().expect("listener address");
    let peer_task = tokio::spawn(serve_metadata_then_piece(
        listener,
        info,
        payload.clone(),
        vec![0x80],
    ));
    let unused_tracker = UdpSocket::bind("127.0.0.1:0")
        .await
        .expect("bind unused tracker");
    let unused_tracker_address = unused_tracker.local_addr().expect("unused tracker address");

    let report = download_magnet(MagnetDownloadConfig {
        identity: test_identity(info_hash),
        magnet: format!(
            "magnet:?xt=urn:btih:{}&x.pe={unsupported_address}&x.pe={address}&\
                 tr=udp%3A%2F%2F{unused_tracker_address}",
            hex(&info_hash)
        ),
        output_path: output_path.clone(),
        network: loopback_network(Duration::from_secs(2)),
        resource_limits: resource_limits(MIN_PAYLOAD_ALLOWANCE),
        skip_files: Vec::new(),
        high_priority_files: Vec::new(),
        dht: None,
    })
    .await
    .expect("public magnet entry");

    assert_eq!(report.info_hash, info_hash);
    assert_eq!(
        tokio::fs::read(&output_path).await.expect("direct output"),
        payload
    );
    unsupported_task.await.expect("non-extension peer task");
    peer_task.await.expect("scripted peer task");
    let mut tracker_packet = [0; 16];
    let (tracker_length, _) = timeout(
        Duration::from_secs(1),
        unused_tracker.recv_from(&mut tracker_packet),
    )
    .await
    .expect("tracker lifecycle should start alongside explicit hints")
    .expect("receive initial tracker connect");
    assert_eq!(tracker_length, 16);
    let _ = tokio::fs::remove_file(output_path).await;
}

#[tokio::test]
async fn dual_topic_magnet_constructs_two_fixed_tracker_lanes() {
    let tracker = UdpSocket::bind("127.0.0.1:0")
        .await
        .expect("bind hybrid tracker");
    let tracker_address = tracker.local_addr().expect("hybrid tracker address");
    let v1 = [0x41; 20];
    let v2 = [0x42; 32];
    let magnet = Magnet::parse(&format!(
        "magnet:?xt=urn:btih:{}&xt=urn:btmh:1220{}&tr=udp%3A%2F%2F{}",
        hex(&v1),
        hex(&v2),
        tracker_address,
    ))
    .expect("dual topic magnet");
    let mut peers = TorrentPeerCoordinator::from_magnet(
        &magnet,
        loopback_network(Duration::from_millis(100)),
        DownloadControl::new(),
        None,
    )
    .await
    .expect("hybrid coordinator");
    assert_eq!(peers.swarm_keys.len(), 2);
    assert_eq!(peers.trackers.len(), 2);
    assert_eq!(
        peers
            .trackers
            .iter()
            .map(|lane| lane.swarm_key)
            .collect::<BTreeSet<_>>(),
        peers.swarm_keys.iter().copied().collect()
    );
    peers.shutdown_tracker().await.expect("shutdown both lanes");
    assert!(peers.trackers.is_empty());
    drop(tracker);
}

#[tokio::test]
async fn transient_dht_miss_retries_without_becoming_terminal() {
    let info_hash = [8; 20];
    let peer = SocketAddr::from(([127, 0, 0, 1], 49_999));
    let dht_socket = UdpSocket::bind("127.0.0.1:0")
        .await
        .expect("bind scripted DHT");
    let dht_address = dht_socket.local_addr().expect("DHT address");
    let dht_task = tokio::spawn(serve_dht_peer_after_retry(dht_socket, info_hash, peer));
    let dht = DhtService::start(dht_config(dht_address))
        .await
        .expect("start DHT client");
    let control = DownloadControl::new();
    let activity = Arc::new(RecordingActivitySink::default());
    control.set_activity_sink(activity.clone());

    let peers = retrying_dht_lookup(
        dht.handle(),
        info_hash,
        control,
        DhtRetryTiming {
            initial_delay: Duration::from_millis(10),
            maximum_delay: Duration::from_millis(20),
        },
        Duration::ZERO,
    )
    .await
    .expect("retry DHT lookup");

    assert_eq!(peers, vec![peer]);
    {
        let events = activity
            .events
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        assert!(
            events
                .iter()
                .any(|event| matches!(event, DownloadActivityEvent::DhtRetryScheduled { .. }))
        );
        assert!(events.iter().any(|event| matches!(
            event,
            DownloadActivityEvent::DhtLookupSucceeded { peer_count: 1 }
        )));
    }
    dht_task.await.expect("scripted DHT task");
    dht.shutdown().await.expect("DHT shutdown");
}

#[tokio::test]
async fn trackerless_dht_peer_completes_metadata_and_content_path() {
    let payload = b"peer discovered through DHT".to_vec();
    let info = single_file_info(&payload);
    let info_hash: [u8; 20] = Sha1::digest(&info).into();
    let output_path = test_path("dht-magnet-output.bin");
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind DHT-discovered peer");
    let peer_address = listener.local_addr().expect("peer address");
    let peer_task = tokio::spawn(serve_metadata_then_piece(
        listener,
        info,
        payload.clone(),
        vec![0x80],
    ));
    let dht_socket = UdpSocket::bind("127.0.0.1:0")
        .await
        .expect("bind scripted DHT");
    let dht_address = dht_socket.local_addr().expect("DHT address");
    let dht_task = tokio::spawn(serve_dht_peer(dht_socket, info_hash, peer_address));
    let dht = DhtService::start(dht_config(dht_address))
        .await
        .expect("start DHT client");

    let report = download_magnet(MagnetDownloadConfig {
        identity: test_identity(info_hash),
        magnet: format!("magnet:?xt=urn:btih:{}", hex(&info_hash)),
        output_path: output_path.clone(),
        network: loopback_network(Duration::from_secs(2)),
        resource_limits: resource_limits(MIN_PAYLOAD_ALLOWANCE),
        skip_files: Vec::new(),
        high_priority_files: Vec::new(),
        dht: Some(dht.handle()),
    })
    .await
    .expect("DHT-discovered download");

    assert_eq!(report.info_hash, info_hash);
    assert_eq!(
        tokio::fs::read(&output_path).await.expect("direct output"),
        payload
    );
    dht_task.await.expect("scripted DHT task");
    peer_task.await.expect("scripted peer task");
    dht.shutdown().await.expect("DHT shutdown");
    let _ = tokio::fs::remove_file(output_path).await;
}

#[tokio::test]
async fn verified_private_metadata_purges_dht_only_peer_before_content() {
    let payload = b"must not be fetched from decentralized peer".to_vec();
    let info = private_single_file_info(&payload);
    let metainfo = Metainfo::from_info_bytes(&info).expect("private metadata");
    assert!(metainfo.private);
    let info_hash = metainfo.info_hash;
    let output_path = test_path("private-dht-output.bin");
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind DHT-only peer");
    let peer_address = listener.local_addr().expect("peer address");
    let peer_task = tokio::spawn(serve_metadata_then_piece(
        listener,
        info,
        payload,
        vec![0x80],
    ));
    let dht_socket = UdpSocket::bind("127.0.0.1:0")
        .await
        .expect("bind scripted DHT");
    let dht_address = dht_socket.local_addr().expect("DHT address");
    let dht_task = tokio::spawn(serve_dht_peer(dht_socket, info_hash, peer_address));
    let dht = DhtService::start(dht_config(dht_address))
        .await
        .expect("start DHT client");

    let result = download_magnet(MagnetDownloadConfig {
        identity: test_identity(info_hash),
        magnet: format!("magnet:?xt=urn:btih:{}", hex(&info_hash)),
        output_path: output_path.clone(),
        network: loopback_network(Duration::from_secs(2)),
        resource_limits: resource_limits(MIN_PAYLOAD_ALLOWANCE),
        skip_files: Vec::new(),
        high_priority_files: Vec::new(),
        dht: Some(dht.handle()),
    })
    .await;

    assert!(matches!(result, Err(DownloadError::NoUsablePeer)));
    assert!(!tokio::fs::try_exists(&output_path).await.expect("output"));
    dht_task.await.expect("scripted DHT task");
    peer_task.await.expect("scripted peer task");
    dht.shutdown().await.expect("DHT shutdown");
}

#[tokio::test]
async fn invalid_premetadata_bitfield_fails_before_storage_creation() {
    let payload = b"not written".to_vec();
    let info = single_file_info(&payload);
    let info_hash: [u8; 20] = Sha1::digest(&info).into();
    let output_path = test_path("bad-premetadata-output.bin");
    let content = output_path.clone();
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind scripted metadata peer");
    let address = listener.local_addr().expect("listener address");
    let peer_task = tokio::spawn(serve_metadata_then_piece(
        listener,
        info,
        payload,
        vec![0x80, 0],
    ));

    let result = download_magnet(MagnetDownloadConfig {
        identity: test_identity(info_hash),
        magnet: format!("magnet:?xt=urn:btih:{}&x.pe={address}", hex(&info_hash)),
        output_path: output_path.clone(),
        network: loopback_network(Duration::from_secs(2)),
        resource_limits: resource_limits(MIN_PAYLOAD_ALLOWANCE),
        skip_files: Vec::new(),
        high_priority_files: Vec::new(),
        dht: None,
    })
    .await;

    assert!(matches!(
        result,
        Err(DownloadError::InvalidPremetadataState(_))
    ));
    assert!(!tokio::fs::try_exists(&output_path).await.expect("output"));
    assert!(!tokio::fs::try_exists(&content).await.expect("content"));
    peer_task.abort();
    let _ = peer_task.await;
}

#[tokio::test]
async fn magnet_peer_without_extension_support_fails_before_storage() {
    let info = single_file_info(b"not written");
    let info_hash: [u8; 20] = Sha1::digest(&info).into();
    let output_path = test_path("no-extension-output.bin");
    let content = output_path.clone();
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind non-extension peer");
    let address = listener.local_addr().expect("listener address");
    let peer_task = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.expect("accept magnet client");
        let mut handshake_bytes = [0; HANDSHAKE_LENGTH];
        stream
            .read_exact(&mut handshake_bytes)
            .await
            .expect("read magnet handshake");
        assert!(
            decode_handshake(&handshake_bytes, info_hash)
                .expect("valid client handshake")
                .supports_extensions()
        );
        stream
            .write_all(&encode_handshake_with_reserved(
                info_hash,
                *b"-RS-NOEXT-0000000000",
                [0; 8],
            ))
            .await
            .expect("send non-extension handshake");
    });

    let result = download_magnet(MagnetDownloadConfig {
        identity: test_identity(info_hash),
        magnet: format!("magnet:?xt=urn:btih:{}&x.pe={address}", hex(&info_hash)),
        output_path: output_path.clone(),
        network: loopback_network(Duration::from_secs(2)),
        resource_limits: resource_limits(MIN_PAYLOAD_ALLOWANCE),
        skip_files: Vec::new(),
        high_priority_files: Vec::new(),
        dht: None,
    })
    .await;

    assert!(matches!(
        result,
        Err(DownloadError::ExtensionProtocolUnsupported)
    ));
    assert!(!tokio::fs::try_exists(&output_path).await.expect("output"));
    assert!(!tokio::fs::try_exists(&content).await.expect("content"));
    peer_task.await.expect("non-extension peer task");
}

#[tokio::test]
async fn magnet_peer_disconnect_during_metadata_fails_before_storage() {
    let info = single_file_info(b"not written");
    let info_hash: [u8; 20] = Sha1::digest(&info).into();
    let output_path = test_path("metadata-disconnect-output.bin");
    let content = output_path.clone();
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind disconnecting peer");
    let address = listener.local_addr().expect("listener address");
    let peer_task = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.expect("accept magnet client");
        let mut handshake_bytes = [0; HANDSHAKE_LENGTH];
        stream
            .read_exact(&mut handshake_bytes)
            .await
            .expect("read magnet handshake");
        decode_handshake(&handshake_bytes, info_hash).expect("valid client handshake");
        let mut reserved = [0; 8];
        reserved[EXTENSION_PROTOCOL_RESERVED_INDEX] = EXTENSION_PROTOCOL_RESERVED_BIT;
        stream
            .write_all(&encode_handshake_with_reserved(
                info_hash,
                *b"-RS-DROP--0000000000",
                reserved,
            ))
            .await
            .expect("send extension handshake");
    });

    let result = download_magnet(MagnetDownloadConfig {
        identity: test_identity(info_hash),
        magnet: format!("magnet:?xt=urn:btih:{}&x.pe={address}", hex(&info_hash)),
        output_path: output_path.clone(),
        network: loopback_network(Duration::from_secs(2)),
        resource_limits: resource_limits(MIN_PAYLOAD_ALLOWANCE),
        skip_files: Vec::new(),
        high_priority_files: Vec::new(),
        dht: None,
    })
    .await;

    assert!(
        matches!(
            &result,
            Err(DownloadError::PeerClosed)
                | Err(DownloadError::Io {
                    operation: "read peer message",
                    ..
                })
        ),
        "unexpected disconnect result: {result:?}"
    );
    assert!(!tokio::fs::try_exists(&output_path).await.expect("output"));
    assert!(!tokio::fs::try_exists(&content).await.expect("content"));
    peer_task.await.expect("disconnecting peer task");
}
