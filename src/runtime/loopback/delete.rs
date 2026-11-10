//! The Ollama delete over the loopback client (model-delete): one `DELETE` to
//! [`DELETE_PATH`] on `127.0.0.1`, the name in a JSON body and never in the
//! URL. `request_to` refuses a `DELETE` with a body, on purpose, so this has
//! a gate of its own, beside the pull's.

use std::{net::SocketAddr, time::Duration};

use tokio::{net::TcpStream, time::timeout};

use super::{Reply, exchange};

/// The only path a delete may go to.
pub const DELETE_PATH: &str = "/api/delete";

pub const DELETE_REFUSED: &str = "refused: a delete goes to the loopback Ollama delete path only";
pub const DELETE_CANNOT_CONNECT: &str = "cannot connect to Ollama";
pub const DELETE_NO_ANSWER: &str = "no answer from Ollama";
pub const DELETE_NOT_FOUND: &str = "Ollama has no model with that name";
pub const DELETE_INVALID: &str = "invalid reply from Ollama";

const CONNECT: Duration = Duration::from_secs(2);
/// Removing a big model's layers can take a while on a slow disk.
const TOTAL: Duration = Duration::from_secs(60);

/// Deletes model `name` from the Ollama server on `host:port`; the reason on
/// failure.
pub async fn delete_with(host: &str, port: u16, path: &str, name: &str) -> Result<(), String> {
    // Only the literal loopback address and the one static path.
    if host != "127.0.0.1" || path != DELETE_PATH {
        return Err(DELETE_REFUSED.to_owned());
    }
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let Ok(Ok(stream)) = timeout(CONNECT, TcpStream::connect(addr)).await else {
        return Err(DELETE_CANNOT_CONNECT.to_owned());
    };
    let body = serde_json::json!({ "model": name }).to_string();
    let request = format!(
        "DELETE {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAccept: application/json\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    match timeout(TOTAL, exchange(stream, request.as_bytes())).await {
        Ok(Ok(Reply { status: 200, .. })) => Ok(()),
        Ok(Ok(Reply { status: 404, .. })) => Err(DELETE_NOT_FOUND.to_owned()),
        Ok(Ok(Reply { status, .. })) => Err(format!("Ollama answered HTTP {status}")),
        Ok(Err(_)) => Err(DELETE_INVALID.to_owned()),
        Err(_) => Err(DELETE_NO_ANSWER.to_owned()),
    }
}
