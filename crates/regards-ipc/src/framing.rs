//! Length-prefixed JSON frames.

use futures_util::{SinkExt, StreamExt};
use serde::Serialize;
use serde::de::DeserializeOwned;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_util::codec::{Framed, LengthDelimitedCodec};

/// Largest accepted frame. Mail bodies travel through the daemon later, so
/// the limit is generous, but bounded so a peer cannot exhaust memory.
pub const MAX_FRAME_BYTES: usize = 16 * 1024 * 1024;

/// Frame a byte stream: 4-byte big-endian length followed by the payload.
pub fn framed<T: AsyncRead + AsyncWrite>(io: T) -> Framed<T, LengthDelimitedCodec> {
    Framed::new(
        io,
        LengthDelimitedCodec::builder()
            .length_field_length(4)
            .big_endian()
            .max_frame_length(MAX_FRAME_BYTES)
            .new_codec(),
    )
}

/// Errors reading or writing frames.
#[derive(Debug, thiserror::Error)]
pub enum FrameError {
    #[error("connection error: {0}")]
    Io(#[from] std::io::Error),
    #[error("malformed message: {0}")]
    Json(#[from] serde_json::Error),
}

/// Serialize `value` and send it as one frame.
pub async fn send<T, M>(framed: &mut Framed<T, LengthDelimitedCodec>, value: &M) -> Result<(), FrameError>
where
    T: AsyncRead + AsyncWrite + Unpin,
    M: Serialize,
{
    let bytes = serde_json::to_vec(value)?;
    framed.send(bytes::Bytes::from(bytes)).await?;
    Ok(())
}

/// Receive one frame and deserialize it. `None` means the peer closed the connection.
pub async fn recv<T, M>(framed: &mut Framed<T, LengthDelimitedCodec>) -> Option<Result<M, FrameError>>
where
    T: AsyncRead + AsyncWrite + Unpin,
    M: DeserializeOwned,
{
    match framed.next().await? {
        Ok(bytes) => Some(serde_json::from_slice(&bytes).map_err(FrameError::from)),
        Err(error) => Some(Err(FrameError::Io(error))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    #[tokio::test]
    async fn round_trips_a_json_frame() {
        let (a, b) = tokio::io::duplex(1024);
        let mut left = framed(a);
        let mut right = framed(b);
        send(&mut left, &json!({"hello": "world"})).await.unwrap();
        let received: Value = recv(&mut right).await.unwrap().unwrap();
        assert_eq!(received, json!({"hello": "world"}));
    }

    #[tokio::test]
    async fn reports_closed_connection_as_none() {
        let (a, b) = tokio::io::duplex(64);
        drop(a);
        let mut right = framed(b);
        assert!(recv::<_, Value>(&mut right).await.is_none());
    }
}
