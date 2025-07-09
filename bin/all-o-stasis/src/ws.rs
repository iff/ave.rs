use axum::body::Bytes;
use axum::extract::ws::{Message, Utf8Bytes, WebSocket};
use chrono::{DateTime, Utc};
use firestore::{
    FirestoreDb, FirestoreListenEvent, FirestoreListener, FirestoreListenerTarget,
    FirestoreMemListenStateStorage, ParentPathBuilder,
};
use futures::stream::{SplitSink, SplitStream};
use futures::{SinkExt, StreamExt, TryStreamExt};
use otp::types::{ObjectId, Patch};
use serde::Serialize;
use std::collections::hash_map::DefaultHasher;
use std::error::Error;
use std::hash::{Hash, Hasher};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::{mpsc, mpsc::Receiver, mpsc::Sender, Mutex};

use crate::storage::PATCHES_COLLECTION;
use crate::{AppError, AppState};

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct WsPatchResponse {
    content: Patch,
    #[serde(rename = "type")]
    ot_type: String,
}

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
    // tracing::debug!("got listener change: {event:?}");
    match event {
        FirestoreListenEvent::DocumentChange(ref doc_change) => {
            tracing::debug!("document changed: {doc_change:?}");

            if let Some(doc) = &doc_change.document {
                // here we need the object id so we need to parse
                let patch: Patch =
                    FirestoreDb::deserialize_doc_to::<Patch>(doc).expect("deserialized object");

                let msg = Message::Text(
                    serde_json::to_string(&patch)
                        .expect("encode message")
                        .into(),
                );
                let ps = send_tx_patch.send(msg).await;
                if let Err(err) = ps {
                    tracing::error!("failed to sent patch with {err}");
                }
            }
        }
        _ => {
            tracing::error!("received a listen response event to handle: {event:?}");
        }
    }

    Ok(())
}

async fn ping_client(ws_tx: Sender<Message>) {
    loop {
        // TODO this eventually fills the channel?
        let sp = ws_tx.send(Message::Ping(Bytes::from_static(&[1]))).await;
        if let Err(err) = sp {
            tracing::error!("failed to send ping with {err}");
            break;
        }
        tokio::time::sleep(std::time::Duration::from_secs(10)).await;
    }
}

async fn drain_channel(
    ws_rx: &mut Receiver<Message>,
    subscriptions: Arc<Mutex<Vec<ObjectId>>>,
    sender: &mut SplitSink<WebSocket, Message>,
    listener_start_time: DateTime<Utc>,
) {
    loop {
        match ws_rx.recv().await {
            None => {
                tracing::error!("drain_channel: should never be in this case!");
            }
            Some(msg) => {
                let processed_msg = match msg {
                    Message::Text(t) => {
                        let patch: Patch = serde_json::from_str(&t).expect("parsing patch");

                        // only send patches that came in after we started the listener
                        if let Some(created) = patch.created_at {
                            if created < listener_start_time {
                                // tracing::debug!("skipping patch {}", patch.revision_id);
                                continue;
                            }
                        } else {
                            tracing::error!("patch without created_at");
                            continue;
                        }

                        // only send out patches the client subscribed
                        match subscriptions.try_lock() {
                            Ok(subscriptions) => {
                                if subscriptions.contains(&patch.object_id) {
                                    let reply = WsPatchResponse {
                                        content: patch,
                                        ot_type: String::from("patch"),
                                    };
                                    Message::Text(
                                        serde_json::to_string(&reply)
                                            .expect("serialize reply")
                                            .into(),
                                    )
                                } else {
                                    continue;
                                }
                            }
                            Err(_) => {
                                tracing::debug!("failed to lock subscriptions.. retrying");
                                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                                continue;
                            }
                        }
                    }
                    Message::Ping(bytes) => Message::Ping(bytes),
                    t => {
                        tracing::error!("received unexpected message from on ws_send: {t:?}");
                        continue;
                    }
                };

                if let Err(err) = sender.send(processed_msg).await {
                    tracing::error!("failed send message over websocket with {err}");
                    break;
                }
            }
        }
    }
}

fn handle_subscribe(t: &Utf8Bytes) -> Result<String, AppError> {
    // we expect the text to have the format: "["+", id]"
    let json: Vec<String> = serde_json::from_str(t).map_err(|e| {
        AppError::ParseError(format!("unexpected text: {t}, parsing failed with {e:?}"))
    })?;

    match &json[..] {
        [op, obj_id] if op == "+" => Ok(obj_id.clone()),
        _ => Err(AppError::ParseError(format!(
            "unexpected subscribe message: {json:?}"
        ))),
    }
}

async fn sub(
    receiver: &mut SplitStream<WebSocket>,
    subscriptions: Arc<Mutex<Vec<ObjectId>>>,
    who: SocketAddr,
) {
    loop {
        match receiver.try_next().await {
            Err(e) => {
                // Protocol(ResetWithoutClosingHandshake) means client closed connection
                // or some other failure and we should exit
                // TODO what other errors can we expect here?
                tracing::error!("while receiving: {e:?}");
                break;
            }
            Ok(None) => {
                // no new message available
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
            Ok(Some(msg)) => match msg {
                Message::Text(t) => match handle_subscribe(&t) {
                    Ok(object_id) => {
                        subscriptions.lock().await.push(object_id.clone());
                        // tracing::debug!("+++ {who} subscribing to {object_id}");
                    }
                    Err(e) => {
                        tracing::error!("{who} sent unexpected message: {e:?}");
                    }
                },
                Message::Binary(_) => tracing::debug!(">>> {who} send binary data!"),
                Message::Close(_c) => {
                    tracing::debug!(">>> {who} sent close");
                    break;
                }
                Message::Pong(v) => tracing::debug!(">>> {who} sent pong with {v:?}"),
                Message::Ping(v) => tracing::debug!(">>> {who} sent ping with {v:?}"),
            },
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

    let mut listener = match patch_listener(state, parent_path, who).await {
        Some(listener) => listener,
        None => return,
    };

    // so this calls tokio::spawn
    // starting the listener_loop: https://docs.rs/firestore/0.44.1/src/firestore/db/listen_changes.rs.html#360
    let _ = listener
        .start(move |event| handle_listener_event(event, ws_tx_listener.clone()))
        .await;
    // hack to only send out patches that are added to the collection after we start the listener
    let listener_start_time = Utc::now();

    // ping the client every 10 seconds
    let mut ping = tokio::spawn(async move { ping_client(ws_tx).await });

    // keep on sending out what we get on the send channel
    // we expect and rely patches (Text) for subscribed object ids and Pings on this channel
    // sender needs to know about subscriptions of object ids - it implements the filtering
    let subs_for_sender = Arc::clone(&subscriptions);
    let mut ws_send = tokio::spawn(async move {
        drain_channel(
            &mut ws_rx,
            subs_for_sender,
            &mut sender,
            listener_start_time,
        )
        .await;
    });

    // recieve object ids the client wants to subscibe
    let mut handle_obj_subs =
        tokio::spawn(async move { sub(&mut receiver, subscriptions, who).await });

    tokio::select! {
        _ = &mut ping => { tracing::debug!(">>> ping aborted") },
        _ = &mut ws_send => {tracing::debug!(">>> ws_send aborted") },
        _ = &mut handle_obj_subs => {tracing::debug!(">>> handle_subscriptions aborted") },
    }

    let _ = listener.shutdown().await;
    ping.abort();
    ws_send.abort();
    handle_obj_subs.abort();
}
