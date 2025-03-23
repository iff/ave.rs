use axum::body::Bytes;
use axum::extract::ws::{Message, WebSocket};
use firestore::{
    FirestoreDb, FirestoreListenEvent, FirestoreListenerTarget, FirestoreMemListenStateStorage,
    ParentPathBuilder,
};
use futures::{SinkExt, StreamExt};
use otp::types::Patch;
use std::net::SocketAddr;

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

            tokio::time::sleep(std::time::Duration::from_millis(1000)).await;
        }
    });

    // recieve object ids the client wants to subscibe
    let _objects = tokio::spawn(async move {
        loop {
            // termination handling?
            while let Some(Ok(msg)) = receiver.next().await {
                // TODO add object id to channel
                // message looks like: 169.254.169.126:40748 subscribing to object id ["+","FaI1zp28CfCswCX4I991"]
                tracing::debug!(
                    "{who} subscribing to object id {}",
                    msg.into_text().expect("parsing object id")
                )
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

// helper to print contents of messages to stdout. Has special treatment for Close.
// fn process_message(msg: Message, who: SocketAddr) -> ControlFlow<(), ()> {
//     match msg {
//         Message::Text(t) => {
//             println!(">>> {who} sent str: {t:?}");
//         }
//         Message::Binary(d) => {
//             println!(">>> {} sent {} bytes: {:?}", who, d.len(), d);
//         }
//         Message::Close(c) => {
//             if let Some(cf) = c {
//                 println!(
//                     ">>> {} sent close with code {} and reason `{}`",
//                     who, cf.code, cf.reason
//                 );
//             } else {
//                 println!(">>> {who} somehow sent close message without CloseFrame");
//             }
//             return ControlFlow::Break(());
//         }
//
//         Message::Pong(v) => {
//             println!(">>> {who} sent pong with {v:?}");
//         }
//         // You should never need to manually handle Message::Ping, as axum's websocket library
//         // will do so for you automagically by replying with Pong and copying the v according to
//         // spec. But if you need the contents of the pings you can see them here.
//         Message::Ping(v) => {
//             println!(">>> {who} sent ping with {v:?}");
//         }
//     }
//     ControlFlow::Continue(())
// }
