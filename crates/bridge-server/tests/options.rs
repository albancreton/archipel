//! `session/options` reaches the agent it is about and wakes nothing, and the agent list says
//! what each agent can do.

mod common;

use bridge_schema::ext;
use bridge_server::Server;
use bridge_transport::Link;
use common::{agent, answer, ask, next, response};
use serde_json::json;
use tokio::sync::mpsc;

#[tokio::test]
async fn options_go_to_the_agent_they_are_about_without_waking_a_session() {
    let (claude_launched, mut claudes) = mpsc::unbounded_channel();
    let (pi_launched, mut pis) = mpsc::unbounded_channel();
    let server = Server::builder()
        .agent(agent("claude", None, claude_launched))
        .agent(agent("pi", None, pi_launched))
        .start();
    let (mut app, theirs) = Link::pair();
    server.connect(theirs);

    // Named by agent: the second agent is started for it, and the first is left alone.
    let about = json!({"cwd": "/work", "_meta": {"bridge": {"agentId": "pi"}}});
    ask(&app, 1, "session/options", about).await;
    let mut pi = pis.recv().await.unwrap();
    let hello = next(&mut pi).await;
    assert_eq!(hello["method"], "initialize");
    let can = json!({"session": {"options": {}, "title": {}}});
    let welcome = json!({"protocolVersion": 2, "capabilities": can, "info": {"name": "pi-acp", "version": "1"}});
    answer(&pi, &hello, welcome).await;
    let asked = next(&mut pi).await;
    assert_eq!(asked["method"], "session/options");
    assert_eq!(asked["params"]["cwd"], "/work");
    let settings = json!({"configOptions": [{"id": "model"}]});
    answer(&pi, &asked, settings.clone()).await;
    let (got, _) = response(&mut app, 1).await;
    assert_eq!(got["result"], settings, "the answer comes back unchanged");
    assert!(
        claudes.try_recv().is_err(),
        "the first agent was not started"
    );

    // The agent list now says what the started agent can do, and nothing for the other.
    ask(&app, 2, ext::METHOD_AGENTS_LIST, json!({})).await;
    let (listed, _) = response(&mut app, 2).await;
    let agents = listed["result"]["agents"].as_array().unwrap();
    assert_eq!(agents[0]["agentId"], "claude");
    assert!(agents[0].get("capabilities").is_none());
    assert_eq!(agents[0]["readiness"], "not_started");
    assert_eq!(agents[1]["readiness"], "ready");
    assert_eq!(agents[1]["capabilities"], can);

    // Named by session, one only the harness knows: asked directly, not loaded or resumed.
    ask(&app, 3, ext::METHOD_SESSIONS_LIST, json!({"agentId": "pi"})).await;
    let listing = next(&mut pi).await;
    assert_eq!(listing["method"], "session/list");
    answer(
        &pi,
        &listing,
        json!({"sessions": [{"sessionId": "h1", "cwd": "/work"}]}),
    )
    .await;
    response(&mut app, 3).await;
    ask(
        &app,
        4,
        "session/options",
        json!({"cwd": "/work", "sessionId": "h1"}),
    )
    .await;
    let asked = next(&mut pi).await;
    assert_eq!(
        asked["method"], "session/options",
        "nothing was resumed first"
    );
    assert_eq!(asked["params"]["sessionId"], "h1");
    answer(&pi, &asked, settings.clone()).await;
    let (got, _) = response(&mut app, 4).await;
    assert_eq!(got["result"], settings);

    // A session or an agent nobody knows is refused without asking anyone.
    for (id, params) in [
        (5, json!({"cwd": "/work", "sessionId": "nope"})),
        (
            6,
            json!({"cwd": "/work", "_meta": {"bridge": {"agentId": "nope"}}}),
        ),
    ] {
        ask(&app, id, "session/options", params).await;
        let (refused, _) = response(&mut app, id).await;
        assert_eq!(refused["error"]["code"], -32602);
    }
    assert!(pi.rx.try_recv().is_err());

    server.shutdown().await;
}
