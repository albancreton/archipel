//! The same-machine transport: a Unix socket only its owner can open.

use crate::{Link, lines};
use std::{
    io,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};
use tokio::net::{UnixListener, UnixStream};

/// Connects to a daemon's socket.
///
/// # Errors
/// When nothing listens on the path or it cannot be opened.
pub async fn connect(path: &Path) -> io::Result<Link> {
    let (reader, writer) = UnixStream::connect(path).await?.into_split();
    Ok(lines(reader, writer))
}

/// A listening socket. The file is removed when this is dropped.
#[derive(Debug)]
pub struct LocalServer {
    listener: UnixListener,
    path: PathBuf,
}

impl LocalServer {
    /// Starts listening. A socket file left by a daemon that is gone is replaced.
    ///
    /// The file gets owner-only permissions. Keep it in a directory only the owner can enter,
    /// since the permissions are set just after the file appears.
    ///
    /// # Errors
    /// When another daemon is listening on the path, or the socket cannot be created.
    pub fn bind(path: &Path) -> io::Result<Self> {
        if path.exists() {
            if std::os::unix::net::UnixStream::connect(path).is_ok() {
                return Err(io::Error::new(
                    io::ErrorKind::AddrInUse,
                    format!("a daemon is already listening on {}", path.display()),
                ));
            }
            std::fs::remove_file(path)?;
        }
        let listener = UnixListener::bind(path)?;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
        Ok(Self {
            listener,
            path: path.to_owned(),
        })
    }

    /// Waits for the next client.
    ///
    /// # Errors
    /// When accepting fails.
    pub async fn accept(&self) -> io::Result<Link> {
        let (stream, _) = self.listener.accept().await?;
        let (reader, writer) = stream.into_split();
        Ok(lines(reader, writer))
    }
}

impl Drop for LocalServer {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn a_client_and_the_server_exchange_messages_and_the_file_is_private() {
        let dir = std::env::temp_dir().join(format!("archipel-local-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("d.sock");
        let server = LocalServer::bind(&path).unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);

        let mut client = connect(&path).await.unwrap();
        let mut served = server.accept().await.unwrap();
        client.tx.send(json!({"id": 1})).await.unwrap();
        assert_eq!(served.rx.recv().await, Some(json!({"id": 1})));
        served.tx.send(json!({"id": 2})).await.unwrap();
        assert_eq!(client.rx.recv().await, Some(json!({"id": 2})));

        assert_eq!(
            LocalServer::bind(&path).unwrap_err().kind(),
            io::ErrorKind::AddrInUse,
            "a second daemon is refused"
        );
        drop(server);
        assert!(!path.exists(), "socket file removed");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
