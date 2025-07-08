use axum::body::Bytes;
use axum::extract::ws::{Message, WebSocket};
use firestore::{
    FirestoreDb, FirestoreListenEvent, FirestoreListener, FirestoreListenerTarget,
    FirestoreMemListenStateStorage, ParentPathBuilder,
};
use futures::{SinkExt, StreamExt};
use otp::types::{ObjectId, Patch};
use std::error::Error;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::{mpsc, mpsc::Sender, Mutex};

use crate::storage::PATCHES_COLLECTION;
use crate::AppState;

async fn patch_listener(
    state: AppState,
    parent_path: ParentPathBuilder,
    who: SocketAddr,
) -> Option<FirestoreListener<FirestoreDb, FirestoreMemListenStateStorage>> {
    // TODO is this setup from scratch for each client? so the ID we use here has to be unique?
    // TODO is this enough? also use ip address?
    let listener_id: FirestoreListenerTarget = FirestoreListenerTarget::new(who.port() as u32);

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

    Ok(())
}

pub(crate) async fn handle_socket(
    socket: WebSocket, // FIXME why not mut?
    who: SocketAddr,
    state: AppState,
    parent_path: ParentPathBuilder,
) {
    let (mut sender, mut receiver) = socket.split();

    // TODO use unbounded channel?
    // channel for messages to be sent back
    let (ws_tx, mut ws_rx) = mpsc::channel(1000);
    // for firestore listener
    let ws_tx_listener = ws_tx.clone();

    let subscriptions: Arc<Mutex<Vec<ObjectId>>> = Arc::new(Mutex::new(Vec::new()));
    let subscriptions_clone = Arc::clone(&subscriptions);

    let mut listener = match patch_listener(state, parent_path, who).await {
        Some(listener) => listener,
        None => return,
    };

    // start firestore listener
    let mut listener_task = tokio::spawn(async move {
        let _ = listener
            .start(move |event| handle_listener_event(event, ws_tx_listener.clone()))
            .await;
    });

    // ping the client every 10 seconds
    let mut ping = tokio::spawn(async move {
        loop {
            // TODO this eventually fills the channel?
            let sp = ws_tx.send(Message::Ping(Bytes::from_static(&[1]))).await;
            if let Err(err) = sp {
                tracing::debug!("error: failed to send ping with {err}");
                break;
            }
            tokio::time::sleep(std::time::Duration::from_secs(10)).await;
        }
    });

    // keep on sending out what we get on the send channel
    // we expect and rely patches (Text) and Pings on this channel
    let mut ws_send = tokio::spawn(async move {
        // this only stops once we close the channel?
        while let Some(msg) = ws_rx.recv().await {
            let processed_msg = match msg {
                Message::Text(t) => {
                    let patch: Patch = serde_json::from_str(&t).expect("parsing patch");

                    // Try to get lock and check subscription
                    let should_send = match subscriptions_clone.try_lock() {
                        Ok(subscriptions) => subscriptions.contains(&patch.object_id),
                        Err(_) => {
                            tracing::debug!("error: failed to lock subscriptions");
                            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                            continue;
                        }
                    };

                    if should_send {
                        tracing::debug!("sending patch to {who}");
                        Message::Text(t)
                    } else {
                        continue;
                    }
                }
                Message::Ping(bytes) => Message::Ping(bytes),
                t => {
                    tracing::debug!("error: received unexpected message from on ws_send: {t:?}");
                    continue;
                }
            };

            if let Err(err) = sender.send(processed_msg).await {
                tracing::debug!("error: failed send message over websocket with {err}");
                break;
            }
        }
    });

    // recieve object ids the client wants to subscibe
    let mut handle_subscriptions = tokio::spawn(async move {
        // this only stops once we close the channel?
        while let Some(Ok(msg)) = receiver.next().await {
            match msg {
                Message::Text(t) => {
                    // message looks like: 169.254.169.126:40748 subscribing to object id ["+","FaI1zp28CfCswCX4I991"]
                    // changeFeedSubscription(h, ["+", id]);
                    let json: Vec<String> =
                        serde_json::from_str(&t).expect("json subscribe message");

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
                        tracing::debug!(">>> {who} somehow sent close message without CloseFrame");
                    }
                    break;
                }
                Message::Pong(v) => {
                    tracing::debug!(">>> {who} sent pong with {v:?}");
                }
                Message::Ping(v) => {
                    tracing::debug!(">>> {who} sent ping with {v:?}");
                }
            }
        }
    });

    tokio::select! {
        _ = &mut ping => {},
        _ = &mut ws_send => {},
        _ = &mut handle_subscriptions => {},
        _ = &mut listener_task => {},
    }

    ping.abort();
    ws_send.abort();
    handle_subscriptions.abort();
    // TODO cleaner shutdown possible?
    listener_task.abort();
}
