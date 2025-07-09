use axum::body::Bytes;
use axum::extract::ws::{Message, WebSocket};
use firestore::{
    FirestoreDb, FirestoreListenEvent, FirestoreListener, FirestoreListenerTarget,
    FirestoreMemListenStateStorage, ParentPathBuilder,
};
use futures::stream::{SplitSink, SplitStream};
use futures::{SinkExt, StreamExt, TryStreamExt};
use otp::types::{ObjectId, Patch};
use std::collections::hash_map::DefaultHasher;
use std::error::Error;
use std::hash::{Hash, Hasher};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::{mpsc, mpsc::Receiver, mpsc::Sender, Mutex};

use crate::storage::PATCHES_COLLECTION;
use crate::AppState;

fn hash_addr(addr: &SocketAddr) -> u64 {
    let mut hasher = DefaultHasher::new();

    match addr {
        SocketAddr::V4(v4) => {
            v4.ip().octets().hash(&mut hasher);
            v4.port().hash(&mut hasher);
        }
        SocketAddr::V6(v6) => {
            v6.ip().octets().hash(&mut hasher);
            v6.port().hash(&mut hasher);
        }
    }

    hasher.finish()
}

async fn patch_listener(
    state: AppState,
    parent_path: ParentPathBuilder,
    who: SocketAddr,
) -> Option<FirestoreListener<FirestoreDb, FirestoreMemListenStateStorage>> {
    let client_id = hash_addr(&who) as u32;
    let listener_id: FirestoreListenerTarget = FirestoreListenerTarget::new(client_id);
    tracing::debug!("connection {who} gets firestore listener id: {client_id:?}");

    // now start streaming patches using firestore listeners: https://github.com/abdolence/firestore-rs/blob/master/examples/listen-changes.rs
    // do we have enough mem?
    let mut listener = match state
        .db
        .create_listener(FirestoreMemListenStateStorage::new())
        .await
    {
        Ok(l) => l,
        Err(..) => return None,
    };

    let _ = state
        .db
        .fluent()
        .select()
        .from(PATCHES_COLLECTION)
        .parent(parent_path)
        .listen()
        .add_target(listener_id, &mut listener);

    Some(listener)
}

async fn handle_listener_event(
    event: FirestoreListenEvent,
    send_tx_patch: Sender<Message>,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    tracing::debug!("got listener change: {event:?}");
    match event {
        FirestoreListenEvent::DocumentChange(ref doc_change) => {
            tracing::debug!("document changed: {doc_change:?}");

            if let Some(doc) = &doc_change.document {
                // here we need the object id so we need to parse
                let patch: Patch =
                    FirestoreDb::deserialize_doc_to::<Patch>(doc).expect("deserialized object");
                tracing::debug!("sending patch {}", patch);

                let msg = Message::Text(
                    serde_json::to_string(&patch)
                        .expect("encode message")
                        .into(),
                );
                let ps = send_tx_patch.send(msg).await;
                if let Err(err) = ps {
                    tracing::debug!("error: failed to sent patch with {err}");
                }
            }
        }
        _ => {
            tracing::debug!("received a listen response event to handle: {event:?}");
        }
    }

    tracing::debug!("finished listener change: {event:?}");
    Ok(())
}

async fn ping_client(ws_tx: Sender<Message>) {
    loop {
        // TODO this eventually fills the channel?
        let sp = ws_tx.send(Message::Ping(Bytes::from_static(&[1]))).await;
        if let Err(err) = sp {
            tracing::debug!("error: failed to send ping with {err}");
            break;
        }
        tokio::time::sleep(std::time::Duration::from_secs(10)).await;
    }
}

async fn drain_channel(
    ws_rx: &mut Receiver<Message>,
    subscriptions: Arc<Mutex<Vec<ObjectId>>>,
    sender: &mut SplitSink<WebSocket, Message>,
) {
    loop {
        match ws_rx.recv().await {
            None => {
                // tracing::debug!("drain: no new messages");
                // rate-limit?
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            }
            Some(msg) => {
                let processed_msg = match msg {
                    Message::Text(t) => {
                        let patch: Patch = serde_json::from_str(&t).expect("parsing patch");

                        // Try to get lock and check subscription
                        let should_send = match subscriptions.try_lock() {
                            Ok(subscriptions) => subscriptions.contains(&patch.object_id),
                            Err(_) => {
                                tracing::debug!("error: failed to lock subscriptions");
                                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                                continue;
                            }
                        };

                        if should_send {
                            tracing::debug!("sending patch");
                            Message::Text(t)
                        } else {
                            continue;
                        }
                    }
                    Message::Ping(bytes) => Message::Ping(bytes),
                    t => {
                        tracing::debug!(
                            "error: received unexpected message from on ws_send: {t:?}"
                        );
                        continue;
                    }
                };

                if let Err(err) = sender.send(processed_msg).await {
                    tracing::debug!("error: failed send message over websocket with {err}");
                    break;
                }
            }
        }
    }
}

async fn sub(
    receiver: &mut SplitStream<WebSocket>,
    subscriptions: Arc<Mutex<Vec<ObjectId>>>,
    who: SocketAddr,
) {
    loop {
        match receiver.try_next().await {
            Err(e) => tracing::debug!("sub error: {e:?}"),
            Ok(None) => {
                // tracing::debug!("sub: no new messages");
                // rate-limit?
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            }
            Ok(Some(msg)) => {
                match msg {
                    Message::Text(t) => {
                        // we expect the text to have the format: "changeFeedSubscription(h, ["+", id])"
                        let json: Vec<String> = match serde_json::from_str(&t) {
                            Ok(json) => json,
                            Err(_e) => {
                                tracing::debug!("{who} send unexpected text: {t}");
                                continue;
                            }
                        };

                        if let [op, obj_id] = &json[..] {
                            if op == "+" {
                                tracing::debug!("{who} subscribing to object id {obj_id}");
                                subscriptions.lock().await.push(obj_id.to_string());
                            } else {
                                tracing::debug!(">>> {who} send an unexpected op {op}");
                            }
                        } else {
                            tracing::debug!(">>> {who} sent unexpected subscribe message {json:?}");
                        }
                    }
                    Message::Binary(_) => tracing::debug!(">>> {who} send binary data!"),
                    Message::Close(c) => {
                        if let Some(cf) = c {
                            tracing::debug!(
                                ">>> {} sent close with code {} and reason `{}`",
                                who,
                                cf.code,
                                cf.reason
                            );
                        } else {
                            tracing::debug!(
                                ">>> {who} somehow sent close message without CloseFrame"
                            );
                        }
                        break;
                    }
                    Message::Pong(v) => tracing::debug!(">>> {who} sent pong with {v:?}"),
                    Message::Ping(v) => tracing::debug!(">>> {who} sent ping with {v:?}"),
                }
            }
        }
    }
}

pub(crate) async fn handle_socket(
    socket: WebSocket,
    who: SocketAddr,
    state: AppState,
    parent_path: ParentPathBuilder,
) {
    let (mut sender, mut receiver) = socket.split();

    // TODO use unbounded channel?
    // channel for messages to be sent back
    let (ws_tx, mut ws_rx) = mpsc::channel(1000);
    // firestore listener also needs to be able to sned on the channel
    let ws_tx_listener = ws_tx.clone();

    // collect all objects ids the client wants to get notified about changes
    let subscriptions: Arc<Mutex<Vec<ObjectId>>> = Arc::new(Mutex::new(Vec::new()));

    // start firestore listener
    // let mut listener_task = tokio::spawn(async move {
    //     let mut listener = match patch_listener(state, parent_path, who).await {
    //         Some(listener) => listener,
    //         None => return,
    //     };
    //
    //     // so this really calls tokio::spawn again
    //     // starting the listener_loop: https://docs.rs/firestore/0.44.1/src/firestore/db/listen_changes.rs.html#360
    //     // and should only shutdown if we explicitly do that or if there is an error in the
    //     // listener callback
    //     // our listener only retrun Ok(()) so this should not stop
    //     let _ = listener
    //         .start(move |event| handle_listener_event(event, ws_tx_listener.clone()))
    //         .await;
    // });
    let mut listener = match patch_listener(state, parent_path, who).await {
        Some(listener) => listener,
        None => return,
    };

    // so this really calls tokio::spawn again
    // starting the listener_loop: https://docs.rs/firestore/0.44.1/src/firestore/db/listen_changes.rs.html#360
    // and should only shutdown if we explicitly do that or if there is an error in the
    // listener callback
    // our listener only retrun Ok(()) so this should not stop
    let _ = listener
        .start(move |event| handle_listener_event(event, ws_tx_listener.clone()))
        .await;
    tracing::debug!("started firestore listener");

    // ping the client every 10 seconds
    let mut ping = tokio::spawn(async move { ping_client(ws_tx).await });

    // keep on sending out what we get on the send channel
    // we expect and rely patches (Text) and Pings on this channel
    let subs_for_sender = Arc::clone(&subscriptions);
    let mut ws_send = tokio::spawn(async move {
        drain_channel(&mut ws_rx, subs_for_sender, &mut sender).await;
    });

    // recieve object ids the client wants to subscibe
    let mut handle_subscriptions =
        tokio::spawn(async move { sub(&mut receiver, subscriptions, who).await });

    tokio::select! {
        _ = &mut ping => { tracing::debug!(">>> ping aborted") },
        _ = &mut ws_send => {tracing::debug!(">>> ws_send aborted") },
        _ = &mut handle_subscriptions => {tracing::debug!(">>> handle_subscriptions aborted") },
        // _ = &mut listener_task => {tracing::debug!(">>> listener_task aborted") },
    }

    let _ = listener.shutdown().await;
    ping.abort();
    ws_send.abort();
    handle_subscriptions.abort();
    // TODO cleaner shutdown possible?
    // listener_task.abort();
}
