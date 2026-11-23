//! The WebSocket transport: one JSON message per text frame, opened with a bearer token.
//!
//! The daemon can run commands as the user, so a connection is accepted only when it carries a
//! known token in an `Authorization: Bearer` header. What a token allows is the caller's
//! business: the server asks a function, and hands its answer over with the link. Requests with an `Origin` header are refused: they
//! come from web pages, and any page a browser shows may try to connect to a local port.

use crate::{Link, QUEUE};
use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use std::{io, net::SocketAddr, sync::Arc};
use tokio::{
    io::{AsyncRead, AsyncWrite},
    net::{TcpListener, TcpStream},
    sync::mpsc,
};
use tokio_tungstenite::{
    WebSocketStream, accept_hdr_async, connect_async,
    tungstenite::{
        Message,
        client::IntoClientRequest,
        handshake::server::{ErrorResponse, Request, Response},
        http::{HeaderValue, StatusCode, header},
    },
};

/// A new random token: 32 bytes from the operating system, as hex.
///
/// # Errors
/// When the operating system has no randomness to give.
pub fn new_token() -> io::Result<String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(io::Error::other)?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

/// Connects to a daemon's WebSocket, such as `ws://127.0.0.1:7890`.
///
/// # Errors
/// When the address is unreachable or the daemon refuses the token.
pub async fn connect(url: &str, token: &str) -> io::Result<Link> {
    let mut request = url.into_client_request().map_err(io::Error::other)?;
    let bearer = HeaderValue::from_str(&format!("Bearer {token}")).map_err(io::Error::other)?;
    request.headers_mut().insert(header::AUTHORIZATION, bearer);
    let (stream, _) = connect_async(request).await.map_err(io::Error::other)?;
    Ok(link(stream))
}

/// Decides what a presented token allows. `None` refuses the connection.
pub type Authorize<T> = Arc<dyn Fn(&str) -> Option<T> + Send + Sync>;

/// A listening WebSocket server. `T` is what a token grants, for example a scope.
pub struct WsServer<T> {
    listener: TcpListener,
    authorize: Authorize<T>,
}

impl<T: Send + 'static> WsServer<T> {
    /// Starts listening on `addr`. Use a loopback address unless something in front adds TLS.
    ///
    /// `authorize` is called for every connection attempt, so tokens can be added and revoked
    /// while the server runs. Compare tokens with [`same`].
    ///
    /// # Errors
    /// When the address cannot be bound.
    pub async fn bind(addr: SocketAddr, authorize: Authorize<T>) -> io::Result<Self> {
        Ok(Self {
            listener: TcpListener::bind(addr).await?,
            authorize,
        })
    }

    /// The address in use, which matters when the port was chosen by the system.
    ///
    /// # Errors
    /// When the socket has no address.
    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.listener.local_addr()
    }

    /// Accepts clients for ever, sending each accepted link to `links` with what its token
    /// grants.
    ///
    /// Handshakes run on their own tasks, so a slow or refused client delays nobody.
    pub async fn run(self, links: mpsc::Sender<(Link, T)>) {
        while let Ok((stream, _)) = self.listener.accept().await {
            let (authorize, links) = (self.authorize.clone(), links.clone());
            tokio::spawn(async move {
                if let Ok(accepted) = handshake(stream, &authorize).await {
                    let _ = links.send(accepted).await;
                }
            });
        }
    }
}

async fn handshake<T>(stream: TcpStream, authorize: &Authorize<T>) -> io::Result<(Link, T)> {
    let mut granted = None;
    #[expect(
        clippy::result_large_err,
        reason = "the WebSocket library sets this signature"
    )]
    let check = |request: &Request, response: Response| -> Result<Response, ErrorResponse> {
        let refuse = |status: StatusCode, why: &str| {
            let mut refused = ErrorResponse::new(Some(why.to_owned()));
            *refused.status_mut() = status;
            refused
        };
        if request.headers().contains_key(header::ORIGIN) {
            return Err(refuse(StatusCode::FORBIDDEN, "browser origins are refused"));
        }
        let given = request
            .headers()
            .get(header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.strip_prefix("Bearer "))
            .unwrap_or_default();
        granted = authorize(given);
        if granted.is_some() {
            Ok(response)
        } else {
            Err(refuse(StatusCode::UNAUTHORIZED, "missing or wrong token"))
        }
    };
    let stream = accept_hdr_async(stream, check)
        .await
        .map_err(io::Error::other)?;
    let granted = granted.ok_or_else(|| io::Error::other("no grant"))?;
    Ok((link(stream), granted))
}

/// Compares two tokens without stopping at the first difference, so timing does not reveal
/// how much of a guess was right.
#[must_use]
pub fn same(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn link<S>(stream: WebSocketStream<S>) -> Link
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let (mut sink, mut source) = stream.split();
    let (in_tx, in_rx) = mpsc::channel(QUEUE);
    let (out_tx, mut out_rx) = mpsc::channel::<Value>(QUEUE);
    tokio::spawn(async move {
        while let Some(Ok(frame)) = source.next().await {
            let Message::Text(text) = frame else {
                continue;
            };
            if let Ok(message) = serde_json::from_str::<Value>(text.as_str())
                && in_tx.send(message).await.is_err()
            {
                break;
            }
        }
    });
    tokio::spawn(async move {
        while let Some(message) = out_rx.recv().await {
            if sink.send(Message::text(message.to_string())).await.is_err() {
                break;
            }
        }
        let _ = sink.close().await;
    });
    Link {
        tx: out_tx,
        rx: in_rx,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    async fn server() -> (String, mpsc::Receiver<(Link, &'static str)>) {
        let authorize: Authorize<&'static str> = Arc::new(|token| {
            [("secret", "owner"), ("peek", "guest")]
                .into_iter()
                .find_map(|(known, grant)| same(token, known).then_some(grant))
        });
        let server = WsServer::bind("127.0.0.1:0".parse().unwrap(), authorize)
            .await
            .unwrap();
        let url = format!("ws://{}", server.local_addr().unwrap());
        let (links_tx, links_rx) = mpsc::channel(4);
        tokio::spawn(server.run(links_tx));
        (url, links_rx)
    }

    #[tokio::test]
    async fn the_right_token_gets_a_link() {
        let (url, mut links) = server().await;
        let mut client = connect(&url, "secret").await.unwrap();
        let (mut served, grant) = links.recv().await.unwrap();
        assert_eq!(grant, "owner");
        client.tx.send(json!({"id": 1})).await.unwrap();
        assert_eq!(served.rx.recv().await, Some(json!({"id": 1})));
        served.tx.send(json!({"id": 2})).await.unwrap();
        assert_eq!(client.rx.recv().await, Some(json!({"id": 2})));

        let _guest = connect(&url, "peek").await.unwrap();
        assert_eq!(
            links.recv().await.unwrap().1,
            "guest",
            "each token has its grant"
        );
    }

    #[tokio::test]
    async fn a_wrong_token_and_a_browser_origin_are_refused() {
        let (url, _links) = server().await;
        assert!(connect(&url, "wrong").await.is_err());
        assert!(connect(&url, "").await.is_err());

        let mut request = url.as_str().into_client_request().unwrap();
        request.headers_mut().insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("Bearer secret"),
        );
        request.headers_mut().insert(
            header::ORIGIN,
            HeaderValue::from_static("https://example.com"),
        );
        assert!(connect_async(request).await.is_err());
    }

    #[test]
    fn tokens_are_long_and_differ() {
        let (a, b) = (new_token().unwrap(), new_token().unwrap());
        assert_eq!(a.len(), 64);
        assert_ne!(a, b);
    }
}
