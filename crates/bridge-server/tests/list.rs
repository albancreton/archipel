//! `_bridge/sessions/list` in pages: `limit` and `cursor`.

mod common;

use bridge_schema::ext;
use bridge_server::Server;
use bridge_transport::Link;
use common::{agent, answer, ask, next, response, started};
use serde_json::{Value, json};
use tokio::sync::mpsc;

/// Asks for a list, lets the adapter report its sessions, and returns the response.
async fn list(app: &mut Link, adapter: &mut Link, id: u64, params: Value) -> Value {
    ask(app, id, ext::METHOD_SESSIONS_LIST, params).await;
    let listing = next(adapter).await;
    assert_eq!(listing["method"], "session/list");
    let at = |day: &str| format!("2026-01-{day}T00:00:00Z");
    let sessions = json!([
        {"sessionId": "h4", "cwd": "/work", "updatedAt": at("01")},
        {"sessionId": "h5", "cwd": "/work"},
        {"sessionId": "h2", "cwd": "/work", "updatedAt": at("02")},
        {"sessionId": "h3", "cwd": "/work", "updatedAt": at("01")},
        {"sessionId": "h1", "cwd": "/work", "updatedAt": at("03")},
    ]);
    answer(adapter, &listing, json!({"sessions": sessions})).await;
    response(app, id).await.0
}

fn ids(listed: &Value) -> Vec<&str> {
    listed["result"]["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|session| session["sessionId"].as_str().unwrap())
        .collect()
}

#[tokio::test]
async fn the_session_list_comes_in_pages_when_asked_with_a_limit() {
    let (launched, mut adapters) = mpsc::unbounded_channel();
    let server = Server::builder()
        .agent(agent("claude", None, launched))
        .start();
    let (mut app, theirs) = Link::pair();
    server.connect(theirs);
    // One session in the daemon's log, updated now: the newest of all.
    ask(&app, 1, "session/new", json!({"cwd": "/work"})).await;
    let mut adapter = started(&mut adapters).await;
    let new = next(&mut adapter).await;
    answer(&adapter, &new, json!({"sessionId": "s1"})).await;
    response(&mut app, 1).await;

    // Without a limit: everything, as before, and no cursor.
    let all = list(&mut app, &mut adapter, 2, json!({})).await;
    assert_eq!(ids(&all), ["s1", "h1", "h2", "h3", "h4", "h5"]);
    assert!(all["result"].get("nextCursor").is_none());

    // In pages of two. Logged and harness sessions are merged before they are cut, sessions
    // of the same time keep one order, and one without a time comes last.
    let first = list(&mut app, &mut adapter, 3, json!({"limit": 2})).await;
    assert_eq!(ids(&first), ["s1", "h1"]);
    let cursor = first["result"]["nextCursor"].clone();
    let second = list(
        &mut app,
        &mut adapter,
        4,
        json!({"limit": 2, "cursor": cursor}),
    )
    .await;
    assert_eq!(ids(&second), ["h2", "h3"]);
    let cursor = second["result"]["nextCursor"].clone();
    let third = list(
        &mut app,
        &mut adapter,
        5,
        json!({"limit": 2, "cursor": cursor}),
    )
    .await;
    assert_eq!(ids(&third), ["h4", "h5"]);
    assert!(
        third["result"].get("nextCursor").is_none(),
        "the last page has no cursor"
    );

    // A cursor works with the daemon's own sessions alone, which starts no agent.
    let logged = json!({"loggedOnly": true, "limit": 1});
    ask(&app, 6, ext::METHOD_SESSIONS_LIST, logged).await;
    let (only, _) = response(&mut app, 6).await;
    assert_eq!(ids(&only), ["s1"]);
    assert!(only["result"].get("nextCursor").is_none());

    // What cannot be a page is refused before any agent is asked.
    for (id, params) in [
        (7, json!({"limit": 0})),
        (8, json!({"limit": 2, "cursor": "nonsense"})),
    ] {
        ask(&app, id, ext::METHOD_SESSIONS_LIST, params).await;
        let (refused, _) = response(&mut app, id).await;
        assert_eq!(refused["error"]["code"], -32602);
    }
    server.shutdown().await;
}
