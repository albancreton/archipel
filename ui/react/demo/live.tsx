// A page that follows the daemon's sessions live, in a browser.
//
//   bridged --ws 127.0.0.1:7890 --ws-origin http://localhost:5173
//   pnpm demo:live                      (in ui/react; serves http://localhost:5173)
//   open "http://localhost:5173/?ws=ws://127.0.0.1:7890#TOKEN"
//
// The token is after the `#`, so the browser does not send it to the page's server.

import { BridgeClient, webSocketTransport, type Bridge, type Session, type SessionSummary } from "@harness-bridge/client";
import { useEffect, useState } from "react";
import { createRoot } from "react-dom/client";
import { ChatView, SessionList, useSession, withSession, withoutSession } from "../src/index.ts";

function Chat({ session, onFork }: { session: Session; onFork: (copy: Session) => void }) {
  const { conversation, actions, openElsewhere, error } = useSession(session, { onFork });
  return <ChatView conversation={conversation} sessionId={session.id} actions={actions} openElsewhere={openElsewhere} error={error} />;
}

function App() {
  const query = new URLSearchParams(location.search);
  const [client, setClient] = useState<BridgeClient | null>(null);
  const [sessions, setSessions] = useState<SessionSummary[]>([]);
  const [agents, setAgents] = useState<Bridge.AgentInfo[]>([]);
  const [session, setSession] = useState<Session | null>(null);
  const [problem, setProblem] = useState("");

  useEffect(() => {
    const url = query.get("ws") ?? "ws://127.0.0.1:7890";
    const token = location.hash.slice(1);
    BridgeClient.connect({
      // A page cannot set the Authorization header, so the token goes as a subprotocol.
      open: () => webSocketTransport(url, { token, subprotocol: true }),
      name: "react-demo",
      onSessionUpdated: (changed) => setSessions((all) => withSession(all, changed)),
      onSessionRemoved: (gone) => {
        setSessions((all) => withoutSession(all, gone));
        setSession((open) => (open?.id === gone ? null : open));
      },
    })
      .then(async (connected) => {
        setClient(connected);
        setSessions(await connected.listSessions({ loggedOnly: query.has("logged") }));
        // Listing started the agents, so they now say what they can do.
        setAgents(await connected.agents());
        const wanted = query.get("session");
        if (wanted) setSession(await connected.session(wanted));
      })
      .catch((error) => setProblem(String(error.message ?? error)));
  }, []);

  if (problem) return <p className="demo-problem">Could not connect: {problem}</p>;
  if (!client) return <p>Connecting…</p>;
  return (
    <div className="demo">
      <div className="demo-sessions">
        <SessionList
          sessions={sessions}
          agents={agents}
          selected={session?.id}
          onOpen={(id) => void client.session(id).then(setSession)}
          onRename={(id, title) => client.setTitle(id, title)}
          onDelete={(id) => client.deleteSession(id)}
        />
      </div>
      {session ? <Chat key={session.id} session={session} onFork={setSession} /> : <p>Choose a session.</p>}
    </div>
  );
}

createRoot(document.getElementById("root")!).render(<App />);
