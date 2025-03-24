use axum::body::Bytes;
use axum::extract::ws::{Message, WebSocket};
use firestore::{
    FirestoreDb, FirestoreListenEvent, FirestoreListenerTarget, FirestoreMemListenStateStorage,
    ParentPathBuilder,
};
use futures::{SinkExt, StreamExt};
use otp::types::Patch;
use std::net::SocketAddr;
use std::sync::mpsc;

//allows to split the websocket stream into separate TX and RX branches
// use futures::{sink::SinkExt, stream::StreamExt};

use crate::AppState;

pub(crate) async fn handle_socket(
    mut socket: WebSocket,
    who: SocketAddr,
    state: AppState,
    parent_path: ParentPathBuilder,
) {
    let (mut sender, mut receiver) = socket.split();
    let (tx, rx) = mpsc::channel();
    // let sender_arc = Arc::new(Mutex::new(sender));

    // ping the client every 10 seconds
    let _ping = tokio::spawn(async move {
        loop {
            // TODO what is in the ping message
            if sender
                .send(Message::Ping(Bytes::from_static(&[1])))
                .await
                .is_err()
            {
                // TODO signal abort
                break;
            }

            tokio::time::sleep(std::time::Duration::from_secs(10)).await;
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
                                tx.send(obj_id.clone() as String).unwrap();
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

    // TODO thread to listen to objids
    // let received = rx.recv().unwrap();
    // and listen for doc changes

    // tokio::select! {
    //     rv_a = (&mut ping) => {
    //         match rv_a {
    //             Ok(a) => println!("{a} messages sent to {who}"),
    //             Err(a) => println!("Error sending messages {a:?}")
    //         }
    //         return;
    //     }
    // }

    // TODO can we also stream all patches and filter later?
    // TODO is this setup from scratch for each client? so the ID we use here has to be unique!
    const LISTENER_ID: FirestoreListenerTarget = FirestoreListenerTarget::new(42_u32);

    // now start streaming patches using firestore listeners: https://github.com/abdolence/firestore-rs/blob/master/examples/listen-changes.rs
    let mut listener = match state
        .db
        .create_listener(FirestoreMemListenStateStorage::new())
        .await
    {
        Ok(l) => l,
        Err(..) => return,
    };

    let _ = state
        .db
        .fluent()
        .select()
        .from("patches")
        // TODO add .filter? here
        .parent(parent_path)
        .listen()
        .add_target(LISTENER_ID, &mut listener);

    let _ = listener
        .start(|event| async move {
            match event {
                FirestoreListenEvent::DocumentChange(ref doc_change) => {
                    tracing::debug!("document changed: {doc_change:?}");

                    if let Some(doc) = &doc_change.document {
                        // here we need the object id so we need to parse
                        let obj: Patch = FirestoreDb::deserialize_doc_to::<Patch>(doc)
                            .expect("deserialized object");
                        tracing::debug!("sending patch {}", obj);

                        // FIXME cant move sender
                        let msg = Message::Text(serde_json::to_string(&obj).expect("").into());
                        // put in channel?
                        // if sender.send(msg).await.is_ok() {
                        //     tracing::debug!("handle_socket: sent path to client");
                        // } else {
                        //     tracing::debug!("handle_socket: failed to sent patch {obj}");
                        // }
                    }
                }
                _ => {
                    tracing::debug!("received a listen response event to handle: {event:?}");
                }
            }

            Ok(())
        })
        .await;

    let _ = listener.shutdown().await;
}
