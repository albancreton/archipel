//! Deleting a session through the daemon removes it at the harness and from the daemon's log.

mod common;

use archipel_schema::ext;
use archipel_server::{Scope, Server};
use archipel_transport::Link;
use common::{agent, answer, ask, next, response, started};
use serde_json::{Value, json};
use tokio::sync::mpsc;

async fn logged(app: &mut Link, id: u64) -> Vec<Value> {
    ask(
        app,
        id,
        ext::METHOD_SESSIONS_LIST,
        json!({"loggedOnly": true}),
    )
    .await;
    let (listed, _) = response(app, id).await;
    listed["result"]["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|session| session["sessionId"].clone())
        .collect()
}

#[tokio::test]
async fn a_deleted_session_leaves_the_log_once_the_harness_agrees_and_every_app_is_told() {
    let (launched, mut adapters) = mpsc::unbounded_channel();
    let server = Server::builder()
        .agent(agent("claude", None, launched))
        .start();
    let (mut app, theirs) = Link::pair();
    server.connect(theirs);
    let (mut other, theirs) = Link::pair();
    server.connect(theirs);

    ask(&app, 1, "session/new", json!({"cwd": "/work"})).await;
    let mut adapter = started(&mut adapters).await;
    let new = next(&mut adapter).await;
    answer(&adapter, &new, json!({"sessionId": "s1"})).await;
    response(&mut app, 1).await;
    assert_eq!(logged(&mut app, 2).await, [json!("s1")]);
    assert_eq!(
        next(&mut other).await["method"],
        ext::NOTIFICATION_SESSIONS_UPDATED
    );

    // Deleting cannot be undone, so it needs the admin scope. An app that may prompt, or an
    // extension, is refused and the harness is not asked.
    for scope in [Scope::Interact, Scope::Extend] {
        let (mut lesser, theirs) = Link::pair();
        server.connect_as(theirs, scope);
        ask(&lesser, 1, "session/delete", json!({"sessionId": "s1"})).await;
        let (refused, _) = response(&mut lesser, 1).await;
        assert_eq!(refused["error"]["code"], -32003, "{scope:?}");
        assert_eq!(refused["error"]["message"], "this needs the admin scope");
    }
    assert!(adapter.rx.try_recv().is_err(), "the harness was not asked");
    assert_eq!(logged(&mut app, 9).await, [json!("s1")]);

    // The harness refuses: nothing changes.
    ask(&app, 3, "session/delete", json!({"sessionId": "s1"})).await;
    let asked = next(&mut adapter).await;
    assert_eq!(asked["method"], "session/delete");
    assert_eq!(asked["params"], json!({"sessionId": "s1"}));
    let refusal =
        json!({"jsonrpc": "2.0", "id": asked["id"], "error": {"code": -32000, "message": "busy"}});
    adapter.tx.send(refusal).await.unwrap();
    let (refused, _) = response(&mut app, 3).await;
    assert_eq!(refused["error"]["message"], "busy");
    assert_eq!(logged(&mut app, 4).await, [json!("s1")]);

    // The harness agrees: the session is gone from the log, and the other app hears of it.
    ask(&app, 5, "session/delete", json!({"sessionId": "s1"})).await;
    let asked = next(&mut adapter).await;
    answer(&adapter, &asked, json!({})).await;
    let (deleted, _) = response(&mut app, 5).await;
    assert_eq!(deleted["result"], json!({}));
    assert_eq!(logged(&mut app, 6).await, Vec::<Value>::new());
    let told = next(&mut other).await;
    assert_eq!(told["method"], ext::NOTIFICATION_SESSIONS_REMOVED);
    assert_eq!(told["params"], json!({"sessionId": "s1"}));

    // It cannot be opened any more, and deleting what the daemon never heard of is refused
    // without asking the harness.
    for (id, method) in [(7, ext::METHOD_SESSION_ATTACH), (8, "session/delete")] {
        ask(&app, id, method, json!({"sessionId": "s1"})).await;
        let (unknown, _) = response(&mut app, id).await;
        assert_eq!(unknown["error"]["code"], -32602, "{method}");
    }

    server.shutdown().await;
}
