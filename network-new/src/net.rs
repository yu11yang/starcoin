// Copyright (c) The Starcoin Core Contributors
// SPDX-License-Identifier: Apache-2.0

use crate::{
    convert_account_address_to_peer_id, convert_peer_id_to_account_address,
    helper::convert_boot_nodes, PayloadMsg, PeerEvent,
};
use crypto::{
    ed25519::{Ed25519PrivateKey, Ed25519PublicKey},
    test_utils::KeyPair,
};

use crate::messages::{Message, NetworkMessage};
use futures::{
    channel::{
        mpsc,
        oneshot::{self, Canceled, Sender},
    },
    prelude::*,
    task::AtomicWaker,
};

use anyhow::*;
use config::NetworkConfig;
use network_p2p::{
    identity, GenericProtoOut as ServiceEvent, NetworkConfiguration,
    NetworkWorker as Libp2pService, NodeKeyConfig, Params, Secret,
};
use parity_codec::alloc::collections::HashSet;
use parking_lot::Mutex;
use scs::SCSCodec;
use slog::Drain;
use std::task::{Context, Poll};
use std::{collections::HashMap, io, sync::Arc, thread};
use types::account_address::AccountAddress;

#[derive(Clone)]
pub struct NetworkService {
    pub libp2p_service: Arc<Mutex<Libp2pService>>,
    acks: Arc<Mutex<HashMap<u128, Sender<()>>>>,
}

pub fn build_network_service(
    cfg: &NetworkConfig,
    key_pair: Arc<KeyPair<Ed25519PrivateKey, Ed25519PublicKey>>,
) -> (
    NetworkService,
    mpsc::UnboundedSender<NetworkMessage>,
    mpsc::UnboundedReceiver<NetworkMessage>,
    mpsc::UnboundedReceiver<PeerEvent>,
    oneshot::Sender<()>,
) {
    let config = NetworkConfiguration {
        listen_addresses: vec![cfg.listen.parse().expect("Failed to parse network config")],
        boot_nodes: convert_boot_nodes(cfg.seeds.clone()),
        node_key: {
            let secret =
                identity::ed25519::SecretKey::from_bytes(&mut key_pair.private_key.to_bytes())
                    .unwrap();
            NodeKeyConfig::Ed25519(Secret::Input(secret))
        },
        ..NetworkConfiguration::default()
    };
    NetworkService::new(config)
}

fn build_libp2p_service(cfg: NetworkConfiguration) -> Result<Arc<Mutex<Libp2pService>>> {
    let protocol = network_p2p::ProtocolId::from("stargate".as_bytes());
    match Libp2pService::new(Params::new(cfg, protocol)) {
        Ok(srv) => Ok(Arc::new(Mutex::new(srv))),
        Err(err) => Err(err.into()),
    }
}

fn run_network(
    net_srv: Arc<Mutex<Libp2pService>>,
    acks: Arc<Mutex<HashMap<u128, Sender<()>>>>,
) -> (
    mpsc::UnboundedSender<NetworkMessage>,
    mpsc::UnboundedReceiver<NetworkMessage>,
    mpsc::UnboundedReceiver<PeerEvent>,
    impl Future<Output = Result<(), std::io::Error>>,
) {
    let (mut _tx, net_rx) = mpsc::unbounded();
    let (net_tx, mut _rx) = mpsc::unbounded::<NetworkMessage>();
    let (event_tx, mut event_rx) = mpsc::unbounded::<PeerEvent>();

    let net_srv_2 = net_srv.clone();
    let ack_sender = net_srv.clone();
    let task_notify = Arc::new(AtomicWaker::new());
    let notify = task_notify.clone();
    let network_fut = stream::poll_fn(move |ctx| {
        notify.register(ctx.waker());
        match net_srv_2.lock().poll(ctx) {
            Poll::Ready(Ok(t)) => Poll::Ready(Some(t)),
            Poll::Ready(Err(e)) => {
                warn!("sth wrong?");
                Poll::Pending
            }
            Poll::Pending => Poll::Pending,
        }
    })
    .for_each(|event| handle_event(acks_sener, _tx, event_tx, ack_sender, event))
    .and_then(|_| {
        debug!("Finish network poll");
        Ok(())
    });

    let protocol_fut = async move {
        while let message = _rx.await {
            send_network_message(message, net_srv.clone()).await?;
        }
        Ok(())
    };
    let futures: Vec<Box<dyn Future<Output = Result<(), io::Error>> + Send + Unpin>> = vec![
        Box::new(network_fut) as Box<_>,
        Box::new(protocol_fut) as Box<_>,
    ];

    let futs = futures::future::select_all(futures)
        .and_then(move |_| {
            debug!("Networking ended");
            Ok(())
        })
        .map_err(|(r, _, _)| r);

    (net_tx, net_rx, event_rx, futs)
}

fn handle_event(
    acks: Arc<Mutex<Libp2pService>>,
    mut _tx: UnboundedSender<NetworkMessage>,
    event_tx: UnboundedSender<PeerEvent>,
    ack_sender: Arc<Mutex<Libp2pService>>,
    event: ServiceEvent,
) -> Result<()> {
    match event {
        ServiceEvent::CustomMessage { peer_id, message } => {
            //todo: Error handle
            let message = Message::from_bytes(message.as_ref()).unwrap();
            match message {
                Message::Payload(payload) => {
                    //receive message
                    info!("Receive message with peer_id:{:?}", &peer_id);
                    let address = convert_peer_id_to_account_address(&peer_id).unwrap();
                    let user_msg = NetworkMessage {
                        peer_id: address,
                        data: payload.data,
                    };
                    let _ = _tx.unbounded_send(user_msg);
                    if payload.id != 0 {
                        ack_sender
                            .lock()
                            .send_custom_message(&peer_id, Message::ACK(payload.id).into_bytes());
                    }
                }
                Message::ACK(message_id) => {
                    info!("Receive message ack");
                    if let Some(mut tx) = acks.lock().remove(&message_id) {
                        let _ = tx.send(());
                    } else {
                        error!(
                            "Receive a invalid ack, message id:{}, peer id:{}",
                            message_id, peer_id
                        );
                    }
                }
            }
        }
        ServiceEvent::CustomProtocolOpen {
            peer_id,
            endpoint: _,
        } => {
            let addr = convert_peer_id_to_account_address(&peer_id).unwrap();
            info!("Connected peer {:?}", addr);
            let open_msg = PeerEvent::Open(addr);
            let _ = event_tx.unbounded_send(open_msg);
        }
        ServiceEvent::CustomProtocolClosed { peer_id, reason: _ } => {
            let addr = convert_peer_id_to_account_address(&peer_id).unwrap();
            info!("Close peer {:?}", addr);
            let open_msg = PeerEvent::Close(addr);
            let _ = event_tx.unbounded_send(open_msg);
        }
        ServiceEvent::Clogged {
            peer_id: _,
            messages: _,
        } => debug!("Network clogged"),
    };
    Ok(())
}

async fn send_network_message(
    message: NetworkMessage,
    net_srv: Arc<Mutex<Libp2pService>>,
) -> Result<()> {
    let peer_id = convert_account_address_to_peer_id(message.peer_id).unwrap();
    net_srv
        .lock()
        .send_custom_message(&peer_id, Message::new_message(message.data).into_bytes());
    task_notify.wake();
    if net_srv.lock().is_open(&peer_id) == false {
        error!(
            "Message send to peer :{} is not connected",
            convert_peer_id_to_account_address(&peer_id).unwrap()
        );
    }
    info!("Already send message {:?}", &peer_id);
    Ok(())
}

fn spawn_network(
    libp2p_service: Arc<Mutex<Libp2pService>>,
    acks: Arc<Mutex<HashMap<u128, Sender<()>>>>,
    close_rx: oneshot::Receiver<()>,
) -> (
    mpsc::UnboundedSender<NetworkMessage>,
    mpsc::UnboundedReceiver<NetworkMessage>,
    mpsc::UnboundedReceiver<PeerEvent>,
) {
    let (network_sender, network_receiver, event_rx, network_future) =
        run_network(libp2p_service, acks);

    let futures = vec![Box::new(network_future), Box::new(close_rx)];

    let future = futures::future::select_all(futures)
        .and_then(move |_| {
            debug!("Networking ended");
            Ok(())
        })
        .map_err(|(r, _, _)| r);

    let mut runtime = tokio::runtime::Builder::new()
        .thread_name("libp2p")
        .build()
        .unwrap();
    let _thread = thread::Builder::new()
        .name("network".to_string())
        .spawn(move || {
            let _ = runtime.block_on(future);
        });
    (network_sender, network_receiver, event_rx)
}

impl NetworkService {
    fn new(
        cfg: NetworkConfiguration,
    ) -> (
        NetworkService,
        mpsc::UnboundedSender<NetworkMessage>,
        mpsc::UnboundedReceiver<NetworkMessage>,
        mpsc::UnboundedReceiver<PeerEvent>,
        oneshot::Sender<()>,
    ) {
        let (close_tx, close_rx) = oneshot::channel::<()>();
        let libp2p_service = build_libp2p_service(cfg).unwrap();
        let acks = Arc::new(Mutex::new(HashMap::new()));
        let (network_sender, network_receiver, event_rx) =
            spawn_network(libp2p_service.clone(), acks.clone(), close_rx);
        info!("Network started, connected peers:");
        for p in libp2p_service.lock().connected_peers() {
            info!("peer_id:{}", p);
        }

        (
            Self {
                libp2p_service,
                acks,
            },
            network_sender,
            network_receiver,
            event_rx,
            close_tx,
        )
    }

    pub fn is_connected(&self, address: AccountAddress) -> bool {
        self.libp2p_service
            .lock()
            .is_open(&convert_account_address_to_peer_id(address).unwrap())
    }

    pub fn identify(&self) -> AccountAddress {
        convert_peer_id_to_account_address(self.libp2p_service.lock().peer_id()).unwrap()
    }

    pub fn send_message(
        &mut self,
        account_address: AccountAddress,
        message: Vec<u8>,
    ) -> impl Future<Output = Result<(), Canceled>> {
        let (tx, rx) = oneshot::channel::<()>();
        let (protocol_msg, message_id) = Message::new_payload(message);
        let peer_id =
            convert_account_address_to_peer_id(account_address).expect("Invalid account address");

        self.libp2p_service
            .lock()
            .send_custom_message(&peer_id, protocol_msg.into_bytes());
        debug!("Send message with ack");
        self.acks.lock().insert(message_id, tx);
        rx
    }

    pub fn broadcast_message(&mut self, message: Vec<u8>) {
        debug!("start send broadcast message");
        let (protocol_msg, message_id) = Message::new_payload(message);

        let message_bytes = protocol_msg.into_bytes();

        let mut peers = HashSet::new();

        for p in self.libp2p_service.lock().connected_peers() {
            debug!("will send message to {}", p);
            peers.insert(p.clone());
        }

        for peer_id in peers {
            self.libp2p_service
                .lock()
                .send_custom_message(&peer_id, message_bytes.clone());
        }
        debug!("finish send broadcast message");
    }
}

pub type NetworkComponent = (
    NetworkService,
    mpsc::UnboundedSender<NetworkMessage>,
    mpsc::UnboundedReceiver<NetworkMessage>,
    oneshot::Sender<()>,
);
