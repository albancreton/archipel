// A page that follows the daemon's sessions live, in a browser.
//
//   bridged --ws 127.0.0.1:7890 --ws-origin http://localhost:5173
//   pnpm demo:live                      (in ui/react; serves http://localhost:5173)
//   open "http://localhost:5173/?ws=ws://127.0.0.1:7890#TOKEN"
//
// The token is after the `#`, so the browser does not send it to the page's server.

import { BridgeClient, webSocketTransport, type Session, type SessionSummary } from "@harness-bridge/client";
import { useEffect, useState } from "react";
import { createRoot } from "react-dom/client";
import { ChatView, useSession } from "../src/index.ts";

function Chat({ session, onFork }: { session: Session; onFork: (copy: Session) => void }) {
  const { conversation, actions, openElsewhere } = useSession(session, { onFork });
  return <ChatView conversation={conversation} sessionId={session.id} actions={actions} openElsewhere={openElsewhere} />;
}

function App() {
  const query = new URLSearchParams(location.search);
  const [client, setClient] = useState<BridgeClient | null>(null);
  const [sessions, setSessions] = useState<SessionSummary[]>([]);
  const [session, setSession] = useState<Session | null>(null);
  const [problem, setProblem] = useState("");

  useEffect(() => {
    const url = query.get("ws") ?? "ws://127.0.0.1:7890";
    const token = location.hash.slice(1);
    BridgeClient.connect({
      // A page cannot set the Authorization header, so the token goes as a subprotocol.
      open: () => webSocketTransport(url, { token, subprotocol: true }),
      name: "react-demo",
      onSessionUpdated: (changed) =>
        setSessions((all) => [changed, ...all.filter((other) => other.sessionId !== changed.sessionId)]),
      onSessionRemoved: (gone) => {
        setSessions((all) => all.filter((other) => other.sessionId !== gone));
        setSession((open) => (open?.id === gone ? null : open));
      },
    })
      .then(async (connected) => {
        setClient(connected);
        setSessions(await connected.listSessions({ loggedOnly: query.has("logged") }));
        const wanted = query.get("session");
        if (wanted) setSession(await connected.session(wanted));
      })
      .catch((error) => setProblem(String(error.message ?? error)));
  }, []);

  if (problem) return <p className="demo-problem">Could not connect: {problem}</p>;
  if (!client) return <p>Connecting…</p>;
  return (
    <div className="demo">
      <ul className="demo-sessions">
        {sessions.map((summary) => (
          <li key={summary.sessionId}>
            <button type="button" onClick={() => void client.session(summary.sessionId).then(setSession)}>
              {summary.title ?? summary.sessionId.slice(0, 8)}
              <small> {summary.agentId} · {summary.state ?? (summary.logged ? "" : "in the harness only")}{summary.openElsewhere ? " · open elsewhere" : ""}</small>
            </button>
          </li>
        ))}
      </ul>
      {session ? <Chat key={session.id} session={session} onFork={setSession} /> : <p>Choose a session.</p>}
    </div>
  );
}

createRoot(document.getElementById("root")!).render(<App />);
