//! The stream halves an MSE connection hands back to `librqbit`.
//!
//! Two jobs, and the second is the one that is easy to get wrong.
//!
//! **Encrypt and decrypt.** RC4 is a stream cipher, so every byte read is
//! decrypted in arrival order and every byte written is encrypted in send
//! order. The cipher must advance by exactly the bytes that cross the socket
//! and no others, which is what [`EncryptedWrite`] is careful about.
//!
//! **Push back what the handshake over-read.** Neither MSE nor the plaintext
//! detection can frame what follows them, so both read past their own end. The
//! bytes past the end are the payload's first bytes and [`Prefixed`] serves
//! them before it touches the socket again.
//!
//! See `TODO/peers.md`, T-163.

use std::io::IoSliceMut;
use std::pin::Pin;
use std::task::{Context, Poll, ready};

use librqbit::AsyncReadVectored;
use tokio::io::{AsyncRead, AsyncWrite};

use super::rc4::Rc4;

/// Most a single `poll_write` encrypts before handing it to the socket.
///
/// A bitfield for a million piece torrent is 128 KiB in one `write_all`, and
/// buffering all of it per connection would be a per-peer cost that scales
/// with the torrent. `write_all` loops, so a cap costs nothing but syscalls.
const WRITE_CHUNK: usize = 32 * 1024;

/// The read half: an optional cipher, and bytes the handshake read too many
/// of.
pub struct Prefixed<R> {
    inner: R,
    /// Already decrypted, and served before `inner`.
    prefix: Vec<u8>,
    /// `None` for a plaintext connection, which still needs the pushback.
    decrypt: Option<Rc4>,
}

impl<R> Prefixed<R> {
    pub fn new(inner: R, prefix: Vec<u8>, decrypt: Option<Rc4>) -> Self {
        Self {
            inner,
            prefix,
            decrypt,
        }
    }

    /// Copy from the pushback into one slice, and report how much moved.
    fn drain_prefix(&mut self, out: &mut [u8]) -> usize {
        let n = out.len().min(self.prefix.len());
        out[..n].copy_from_slice(&self.prefix[..n]);
        self.prefix.drain(..n);
        n
    }
}

impl<R: AsyncRead + Unpin> AsyncRead for Prefixed<R> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let this = self.get_mut();
        if !this.prefix.is_empty() {
            let n = buf.remaining().min(this.prefix.len());
            buf.put_slice(&this.prefix[..n]);
            this.prefix.drain(..n);
            return Poll::Ready(Ok(()));
        }
        let start = buf.filled().len();
        ready!(Pin::new(&mut this.inner).poll_read(cx, buf))?;
        let end = buf.filled().len();
        if let Some(cipher) = this.decrypt.as_mut() {
            cipher.apply(&mut buf.filled_mut()[start..end]);
        }
        Poll::Ready(Ok(()))
    }
}

impl<R: AsyncReadVectored + Unpin> AsyncReadVectored for Prefixed<R> {
    fn poll_read_vectored(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        vec: &mut [IoSliceMut<'_>],
    ) -> Poll<std::io::Result<usize>> {
        let this = self.get_mut();
        if !this.prefix.is_empty() {
            let mut written = 0;
            for slice in vec.iter_mut() {
                if this.prefix.is_empty() {
                    break;
                }
                written += this.drain_prefix(slice);
            }
            if written > 0 {
                return Poll::Ready(Ok(written));
            }
        }
        let n = ready!(Pin::new(&mut this.inner).poll_read_vectored(cx, vec))?;
        if let Some(cipher) = this.decrypt.as_mut() {
            // The inner read filled the slices in order, so the keystream is
            // applied in that order too. Splitting a stream cipher across
            // buffers is only correct if the order is the wire's order.
            let mut left = n;
            for slice in vec.iter_mut() {
                if left == 0 {
                    break;
                }
                let take = slice.len().min(left);
                cipher.apply(&mut slice[..take]);
                left -= take;
            }
        }
        Poll::Ready(Ok(n))
    }
}

/// The write half.
///
/// `poll_write` does not report a byte consumed until its ciphertext has
/// reached the inner writer. Reporting earlier and draining later is legal for
/// `AsyncWrite` and is what a buffering writer normally does, and it is wrong
/// here: `librqbit` writes with `write_all` and never flushes, so bytes left in
/// a buffer would sit there until the next message while the peer waited for
/// them. The price is that this returns `Pending` under socket backpressure,
/// which is the correct thing for it to do anyway.
pub struct EncryptedWrite<W> {
    inner: W,
    encrypt: Rc4,
    /// Ciphertext waiting for the socket.
    pending: Vec<u8>,
    /// How much of `pending` has been accepted.
    sent: usize,
    /// How many plaintext bytes `pending` was made from.
    source_len: usize,
}

impl<W> EncryptedWrite<W> {
    pub fn new(inner: W, encrypt: Rc4) -> Self {
        Self {
            inner,
            encrypt,
            pending: Vec::new(),
            sent: 0,
            source_len: 0,
        }
    }
}

impl<W: AsyncWrite + Unpin> EncryptedWrite<W> {
    /// Push `pending` at the inner writer until it is empty.
    fn poll_drain(&mut self, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        while self.sent < self.pending.len() {
            let n = ready!(Pin::new(&mut self.inner).poll_write(cx, &self.pending[self.sent..]))?;
            if n == 0 {
                return Poll::Ready(Err(std::io::Error::new(
                    std::io::ErrorKind::WriteZero,
                    "peer stopped accepting encrypted bytes",
                )));
            }
            self.sent += n;
        }
        self.pending.clear();
        self.sent = 0;
        Poll::Ready(Ok(()))
    }
}

impl<W: AsyncWrite + Unpin> AsyncWrite for EncryptedWrite<W> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        let this = self.get_mut();
        if this.pending.is_empty() {
            if buf.is_empty() {
                return Poll::Ready(Ok(0));
            }
            let take = buf.len().min(WRITE_CHUNK);
            this.pending.extend_from_slice(&buf[..take]);
            this.encrypt.apply(&mut this.pending);
            this.source_len = take;
            this.sent = 0;
        }
        // Under `--trace handshake`, because this is what says whether a stall
        // on an encrypted connection is this wrapper's or the stream's below
        // it. T-233 is uTP under MSE not completing a transfer, and these three
        // lines are what took `EncryptedWrite` out of the suspect list: every
        // byte handed to it was accepted by the writer under it, in order, and
        // the transfer still did not complete. See `TODO/peers.md`, T-233.
        match this.poll_drain(cx) {
            Poll::Pending => {
                tracing::trace!(
                    target: "bit_cli::handshake",
                    pending = this.pending.len(),
                    sent = this.sent,
                    "encrypted write deferred by the stream below"
                );
                return Poll::Pending;
            }
            Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
            Poll::Ready(Ok(())) => {}
        }
        let written = this.source_len;
        this.source_len = 0;
        tracing::trace!(target: "bit_cli::handshake", written, "encrypted write accepted");
        Poll::Ready(Ok(written))
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        let this = self.get_mut();
        ready!(this.poll_drain(cx))?;
        Pin::new(&mut this.inner).poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        let this = self.get_mut();
        ready!(this.poll_drain(cx))?;
        Pin::new(&mut this.inner).poll_shutdown(cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    fn keys() -> (Rc4, Rc4) {
        (Rc4::new_mse(&[3u8; 20]), Rc4::new_mse(&[3u8; 20]))
    }

    /// What is written comes back, whatever the write sizes were. The cipher
    /// advancing by the wrong amount shows up here and nowhere else.
    #[tokio::test]
    async fn a_round_trip_survives_any_write_size() {
        for chunk in [1usize, 7, 4096, WRITE_CHUNK + 13] {
            let (client, server) = tokio::io::duplex(1024);
            let (enc, dec) = keys();
            let payload: Vec<u8> = (0..(WRITE_CHUNK * 2 + 37) as u32)
                .map(|i| (i % 253) as u8)
                .collect();
            let expect = payload.clone();

            let writer = tokio::spawn(async move {
                let mut w = EncryptedWrite::new(client, enc);
                for piece in payload.chunks(chunk) {
                    w.write_all(piece).await.unwrap();
                }
                w.flush().await.unwrap();
                w.shutdown().await.unwrap();
            });

            let mut r = Prefixed::new(server, Vec::new(), Some(dec));
            let mut got = Vec::new();
            r.read_to_end(&mut got).await.unwrap();
            writer.await.unwrap();
            assert_eq!(got, expect, "chunk {chunk}");
        }
    }

    /// A duplex narrower than one write forces the partial-write path, which
    /// is the one that can double-count or drop a byte.
    #[tokio::test]
    async fn a_narrow_socket_does_not_lose_or_repeat_a_byte() {
        let (client, server) = tokio::io::duplex(64);
        let (enc, dec) = keys();
        let payload: Vec<u8> = (0..5000u32).map(|i| (i % 251) as u8).collect();
        let expect = payload.clone();
        let writer = tokio::spawn(async move {
            let mut w = EncryptedWrite::new(client, enc);
            w.write_all(&payload).await.unwrap();
            w.shutdown().await.unwrap();
        });
        let mut r = Prefixed::new(server, Vec::new(), Some(dec));
        let mut got = Vec::new();
        r.read_to_end(&mut got).await.unwrap();
        writer.await.unwrap();
        assert_eq!(got.len(), expect.len());
        assert_eq!(got, expect);
    }

    /// The pushback is served first and exactly once, and the stream after it
    /// is continuous.
    #[tokio::test]
    async fn the_pushback_comes_first_and_only_once() {
        let (client, server) = tokio::io::duplex(4096);
        let (mut enc, dec) = keys();
        let mut tail = b"tail bytes over the wire".to_vec();
        enc.apply(&mut tail);
        tokio::spawn(async move {
            let mut client = client;
            client.write_all(&tail).await.unwrap();
            client.shutdown().await.unwrap();
        });
        let mut r = Prefixed::new(server, b"head bytes".to_vec(), Some(dec));
        let mut got = Vec::new();
        r.read_to_end(&mut got).await.unwrap();
        assert_eq!(got, b"head bytestail bytes over the wire");
    }

    /// A plaintext connection uses the same reader with no cipher, so the
    /// pushback path is shared and the bytes are untouched.
    #[tokio::test]
    async fn a_plaintext_reader_only_pushes_back() {
        let (client, server) = tokio::io::duplex(4096);
        tokio::spawn(async move {
            let mut client = client;
            client.write_all(b" and the rest").await.unwrap();
            client.shutdown().await.unwrap();
        });
        let mut r = Prefixed::new(server, b"\x13BitTorrent protocol".to_vec(), None);
        let mut got = Vec::new();
        r.read_to_end(&mut got).await.unwrap();
        assert_eq!(got, b"\x13BitTorrent protocol and the rest");
    }
}
