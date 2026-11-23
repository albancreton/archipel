//! How JSON-RPC messages travel: lines over a byte stream, a local socket, or a WebSocket.
//!
//! Every transport ends in a [`Link`]: a pair of queues of JSON messages. The daemon, its clients
//! and the adapter connections all work on links, so there is one codec.

pub mod local;
pub mod ws;

use serde_json::Value;
use tokio::{
    io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader},
    sync::mpsc,
};

/// How many messages may wait in each direction before the sender is told the queue is full.
pub const QUEUE: usize = 4096;

/// A connection to a peer, as two queues of JSON-RPC messages.
///
/// `rx` yields `None` once the peer is gone. Dropping `tx` closes our side.
#[derive(Debug)]
pub struct Link {
    pub tx: mpsc::Sender<Value>,
    pub rx: mpsc::Receiver<Value>,
}

impl Link {
    /// Two connected links in memory: what one sends, the other receives.
    #[must_use]
    pub fn pair() -> (Self, Self) {
        let (a_tx, b_rx) = mpsc::channel(QUEUE);
        let (b_tx, a_rx) = mpsc::channel(QUEUE);
        (Self { tx: a_tx, rx: a_rx }, Self { tx: b_tx, rx: b_rx })
    }
}

/// A link over a byte stream carrying one JSON message per line (what ACP uses on stdio).
///
/// Lines that are not JSON are skipped: adapters sometimes print other text.
/// Must be called inside a tokio runtime.
pub fn lines<R, W>(reader: R, mut writer: W) -> Link
where
    R: AsyncRead + Unpin + Send + 'static,
    W: AsyncWrite + Unpin + Send + 'static,
{
    let (in_tx, in_rx) = mpsc::channel(QUEUE);
    let (out_tx, mut out_rx) = mpsc::channel::<Value>(QUEUE);
    tokio::spawn(async move {
        let mut lines = BufReader::new(reader).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            if let Ok(message) = serde_json::from_str::<Value>(&line)
                && in_tx.send(message).await.is_err()
            {
                break;
            }
        }
    });
    tokio::spawn(async move {
        while let Some(message) = out_rx.recv().await {
            let mut line = message.to_string();
            line.push('\n');
            if writer.write_all(line.as_bytes()).await.is_err() || writer.flush().await.is_err() {
                break;
            }
        }
        let _ = writer.shutdown().await;
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

    #[tokio::test]
    async fn lines_carry_messages_both_ways_and_skip_text_that_is_not_json() {
        let (near, far) = tokio::io::duplex(1024);
        let (near_read, near_write) = tokio::io::split(near);
        let (far_read, mut far_write) = tokio::io::split(far);
        let mut link = lines(near_read, near_write);

        far_write
            .write_all(b"starting up...\n{\"id\":1}\n")
            .await
            .unwrap();
        assert_eq!(link.rx.recv().await, Some(json!({"id": 1})));

        link.tx.send(json!({"method": "x"})).await.unwrap();
        let mut far_lines = BufReader::new(far_read).lines();
        assert_eq!(
            far_lines.next_line().await.unwrap().as_deref(),
            Some(r#"{"method":"x"}"#)
        );

        drop(far_write);
        drop(far_lines);
        assert_eq!(link.rx.recv().await, None, "peer gone");
    }
}
