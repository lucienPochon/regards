//! A TCP proxy for fault injection: it forwards to a real server until told
//! to go offline, then drops every open connection and refuses new ones,
//! the way a lost network looks to the daemon (closed connections, failed
//! connects). TLS passes through untouched.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::AbortHandle;

pub struct Proxy {
    pub port: u16,
    online: Arc<AtomicBool>,
    connections: Arc<Mutex<Vec<AbortHandle>>>,
    accept: AbortHandle,
}

impl Proxy {
    /// Listen on 127.0.0.1 and forward to `upstream` (host:port).
    pub async fn start(upstream: String) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let online = Arc::new(AtomicBool::new(true));
        let connections: Arc<Mutex<Vec<AbortHandle>>> = Arc::default();
        let accept = tokio::spawn({
            let (online, connections) = (Arc::clone(&online), Arc::clone(&connections));
            async move {
                loop {
                    let Ok((mut client, _)) = listener.accept().await else {
                        continue;
                    };
                    if !online.load(Ordering::SeqCst) {
                        drop(client); // Refused, as far as the client can tell.
                        continue;
                    }
                    let upstream = upstream.clone();
                    let forward = tokio::spawn(async move {
                        if let Ok(mut server) = TcpStream::connect(&upstream).await {
                            let _ = tokio::io::copy_bidirectional(&mut client, &mut server).await;
                        }
                    });
                    connections.lock().unwrap().push(forward.abort_handle());
                }
            }
        })
        .abort_handle();
        Self {
            port,
            online,
            connections,
            accept,
        }
    }

    /// Drop every connection and refuse new ones.
    pub fn go_offline(&self) {
        self.online.store(false, Ordering::SeqCst);
        for connection in self.connections.lock().unwrap().drain(..) {
            connection.abort();
        }
    }

    pub fn go_online(&self) {
        self.online.store(true, Ordering::SeqCst);
    }
}

impl Drop for Proxy {
    fn drop(&mut self) {
        self.go_offline();
        self.accept.abort();
    }
}
