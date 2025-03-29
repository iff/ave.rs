use axum::body::Bytes;
use axum::extract::ws::{Message, WebSocket};
use firestore::{
    FirestoreDb, FirestoreListenEvent, FirestoreListener, FirestoreListenerTarget,
    FirestoreMemListenStateStorage, ParentPathBuilder,
};
use futures::{SinkExt, StreamExt};
use otp::types::{ObjectId, Patch};
use std::net::SocketAddr;
use tokio::sync::mpsc;

use crate::AppState;

async fn patch_listener(
    state: AppState,
    parent_path: ParentPathBuilder,
) -> Option<FirestoreListener<FirestoreDb, FirestoreMemListenStateStorage>> {
    // TODO is this setup from scratch for each client? so the ID we use here has to be unique?
    const LISTENER_ID: FirestoreListenerTarget = FirestoreListenerTarget::new(42_u32);

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
        .from("patches")
        .parent(parent_path)
        .listen()
        .add_target(LISTENER_ID, &mut listener);

    Some(listener)
}

pub(crate) async fn handle_socket(
    socket: WebSocket, // FIXME why not mut?
    who: SocketAddr,
    state: AppState,
    parent_path: ParentPathBuilder,
) {
    let (mut sender, mut receiver) = socket.split();

    // TODO use unbounded channel?
    // channel for subscriptions
    let (tx, mut rx) = mpsc::channel(100);

    // TODO use unbounded channel?
    // channel for messages to be sent back
    let (send_tx, mut send_rx) = mpsc::channel(100);

    let mut listener = match patch_listener(state, parent_path).await {
        Some(listener) => listener,
        None => return,
    };

    // TODO here we need to send out patches somehow
    let send_tx_patch = send_tx.clone();
    let _patches = tokio::spawn(async move {
        let mut objs: Vec<String> = Vec::new();

        let _ = listener
            .start(move |event| {
                let send_tx_patch_ = send_tx_patch.clone();
                async move {
                    match event {
                        FirestoreListenEvent::DocumentChange(ref doc_change) => {
                            tracing::debug!("document changed: {doc_change:?}");

                            if let Some(doc) = &doc_change.document {
                                // here we need the object id so we need to parse
                                let obj: Patch = FirestoreDb::deserialize_doc_to::<Patch>(doc)
                                    .expect("deserialized object");
                                // tracing::debug!("sending patch {}", obj);

                                // TODO only if object id is in subs

                                let msg =
                                    Message::Text(serde_json::to_string(&obj).expect("").into());
                                let ps = send_tx_patch_.send(msg).await;
                                if let Err(err) = ps {
                                    tracing::debug!("error: failed to sent patch with {err}");
                                    // TODO break
                                }
                            }
                        }
                        _ => {
                            tracing::debug!(
                                "received a listen response event to handle: {event:?}"
                            );
                        }
                    }

                    Ok(())
                }
            })
            .await;

        loop {
            if let Ok(obj_id) = rx.try_recv() {
                objs.push(obj_id)
            };
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }

        // let _ = listener.shutdown().await;
    });

    // ping the client every 10 seconds
    let _ping = tokio::spawn(async move {
        loop {
            let sp = send_tx.send(Message::Ping(Bytes::from_static(&[1]))).await;
            if let Err(err) = sp {
                tracing::debug!("error: failed to send ping with {err}");
                break;
            }
            tokio::time::sleep(std::time::Duration::from_secs(10)).await;
        }
    });

    // keep on sending out what we get on the send channel
    let _ws_send = tokio::spawn(async move {
        while let Ok(msg) = send_rx.try_recv() {
            let s = sender.send(msg).await;
            if let Err(err) = s {
                tracing::debug!("error: failed send message over websocket with {err}");
                // TODO signal abort
                break;
            }
        }
    });

    // recieve object ids the client wants to subscibe
    let _objects = tokio::spawn(async move {
        loop {
            // termination handling?
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
                                let ps = tx.send(obj_id.clone() as String).await;
                                if let Err(err) = ps {
                                    tracing::debug!("failed to send subscribe: {}", err);
                                    break;
                                }
                            } else {
                                tracing::debug!(">>> {who} send an unxepected op {op}");
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
                    Message::Pong(v) => {
                        tracing::debug!(">>> {who} sent pong with {v:?}");
                    }
                    Message::Ping(v) => {
                        tracing::debug!(">>> {who} sent pong with {v:?}");
                    }
                }
            }
        }
    });

    // tokio::select! {
    //     rv_a = (&mut ping) => {
    //         match rv_a {
    //             Ok(a) => println!("{a} messages sent to {who}"),
    //             Err(a) => println!("Error sending messages {a:?}")
    //         }
    //         return;
    //     }
    // }
}
