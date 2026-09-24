//! El codec funciona sobre el transporte real: named pipe en Windows, unix socket en Unix.

use interprocess::local_socket::tokio::prelude::*;
use interprocess::local_socket::{GenericNamespaced, ListenerOptions};
use serde_json::json;
use symphony_protocol::{Connection, Message, Request, Response};

#[tokio::test]
async fn request_response_over_local_socket() {
    let name_str = format!("symphony-protocol-test-{}.sock", std::process::id());
    let name = name_str.clone().to_ns_name::<GenericNamespaced>().unwrap();
    let listener = ListenerOptions::new().name(name).create_tokio().unwrap();

    let server = tokio::spawn(async move {
        let mut conn = Connection::new(listener.accept().await.unwrap());
        while let Some(Message::Request(req)) = conn.recv().await.unwrap() {
            let reply = Response::ok(req.id, json!({"echo": req.method}));
            conn.send(&Message::Response(reply)).await.unwrap();
        }
    });

    let name = name_str.to_ns_name::<GenericNamespaced>().unwrap();
    let mut client = Connection::new(LocalSocketStream::connect(name).await.unwrap());
    for i in 0..50 {
        let id = format!("req-{i}");
        client
            .send(&Message::Request(Request::new(&id, "ping", json!({}))))
            .await
            .unwrap();
        let reply = client.recv().await.unwrap();
        assert_eq!(
            reply,
            Some(Message::Response(Response::ok(id, json!({"echo": "ping"}))))
        );
    }
    drop(client);
    server.await.unwrap();
}
