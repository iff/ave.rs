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
    parent_path: ParentPathBuilder,
    state: AppState,
) {
    let (mut sender, mut receiver) = socket.split();

    // ping the client every 10 seconds
    let mut ping = tokio::spawn(async move {
        loop {
            if sender
                // .send(Message::Text(format!("Server message {i} ...").into()))
                // TODO what is in the ping message
                .send(Message::Ping(Bytes::from_static(&[1])))
                .await
                .is_err()
            {
                // TODO signal abort
                break;
            }

            tokio::time::sleep(std::time::Duration::from_millis(1000)).await;
        }

        return;
    });

    // recieve object ids the client wants to subscibe
    let mut objects = tokio::spawn(async move {
        loop {
            // termination handling?
            while let Some(Ok(msg)) = receiver.next().await {
                // add object id to channel
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
                    println!("Doc changed: {doc_change:?}");

                    if let Some(doc) = &doc_change.document {
                        let obj: Patch = FirestoreDb::deserialize_doc_to::<Patch>(doc)
                            .expect("Deserialized object");
                        println!("As object: {obj}");
                        // TODO not sure if we have to deserialise or if we can
                        // send the doc directly
                        if socket.send(Message::Text(obj)).await.is_ok() {
                            println!("handle_socket: sent path to client");
                        } else {
                            println!("handle_socket: failed to sent patch {obj}");
                        }
                    }
                }
                _ => {
                    println!(
                        "handle_socket: received a listen response event to handle: {event:?}"
                    );
                }
            }

            Ok(())
        })
        .await;

    let _ = listener.shutdown().await;
}

// receive single message from a client (we can either receive or send with socket).
// this will likely be the Pong for our Ping or a hello message from client.
// waiting for message from a client will block this task, but will not block other client's
// connections.
// if let Some(msg) = socket.recv().await {
//     if let Ok(msg) = msg {
//         if process_message(msg, who).is_break() {
//             return;
//         }
//     } else {
//         println!("client {who} abruptly disconnected");
//         return;
//     }
// }

// Since each client gets individual statemachine, we can pause handling
// when necessary to wait for some external event (in this case illustrated by sleeping).
// Waiting for this client to finish getting its greetings does not prevent other clients from
// connecting to server and receiving their greetings.
// for i in 1..5 {
//     if socket
//         .send(Message::Text(format!("Hi {i} times!").into()))
//         .await
//         .is_err()
//     {
//         println!("client {who} abruptly disconnected");
//         return;
//     }
//     tokio::time::sleep(std::time::Duration::from_millis(100)).await;
// }

// By splitting socket we can send and receive at the same time. In this example we will send
// unsolicited messages to client based on some sort of server's internal event (i.e .timer).
// let (mut sender, mut receiver) = socket.split();

// Spawn a task that will push several messages to the client (does not matter what client does)
// let mut send_task = tokio::spawn(async move {
//     let n_msg = 20;
//     for i in 0..n_msg {
//         // In case of any websocket error, we exit.
//         if sender
//             .send(Message::Text(format!("Server message {i} ...").into()))
//             .await
//             .is_err()
//         {
//             return i;
//         }
//
//         tokio::time::sleep(std::time::Duration::from_millis(300)).await;
//     }
//
//     println!("Sending close to {who}...");
//     if let Err(e) = sender
//         .send(Message::Close(Some(CloseFrame {
//             code: axum::extract::ws::close_code::NORMAL,
//             reason: Utf8Bytes::from_static("Goodbye"),
//         })))
//         .await
//     {
//         println!("Could not send Close due to {e}, probably it is ok?");
//     }
//     n_msg
// });

// This second task will receive messages from client and print them on server console
// let mut recv_task = tokio::spawn(async move {
//     let mut cnt = 0;
//     while let Some(Ok(msg)) = receiver.next().await {
//         cnt += 1;
//         // print message and break if instructed to do so
//         if process_message(msg, who).is_break() {
//             break;
//         }
//     }
//     cnt
// });

// If any one of the tasks exit, abort the other.
// tokio::select! {
//     rv_a = (&mut send_task) => {
//         match rv_a {
//             Ok(a) => println!("{a} messages sent to {who}"),
//             Err(a) => println!("Error sending messages {a:?}")
//         }
//         recv_task.abort();
//     },
//     rv_b = (&mut recv_task) => {
//         match rv_b {
//             Ok(b) => println!("Received {b} messages"),
//             Err(b) => println!("Error receiving messages {b:?}")
//         }
//         send_task.abort();
//     }
// }

// returning from the handler closes the websocket connection
// println!("Websocket context {who} destroyed");

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
