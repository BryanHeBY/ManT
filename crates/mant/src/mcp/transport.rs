//! Bounded stdio transport for the local MCP subprocess.

use std::{
    collections::VecDeque,
    io,
    pin::Pin,
    task::{Context, Poll},
};

use rmcp::ServiceExt;
use tokio::io::{AsyncRead, ReadBuf};

use super::MantMcpServer;

/// Upper bound on one newline-delimited MCP request, in bytes.
pub(super) const MAX_MCP_LINE_BYTES: usize = 256 * 1024;

/// Run the MCP server until the peer closes its standard-input stream.
pub(crate) async fn run_stdio() -> u8 {
    let transport = (
        LineBoundedReader::new(tokio::io::stdin(), MAX_MCP_LINE_BYTES),
        tokio::io::stdout(),
    );
    let Ok(service) = MantMcpServer::new().serve(transport).await else {
        return 1;
    };

    match service.waiting().await {
        Ok(_) => 0,
        Err(_) => 1,
    }
}

/// Wraps an [`AsyncRead`] and fails once a single line exceeds `max_line`.
pub(super) struct LineBoundedReader<R> {
    inner: R,
    max_line: usize,
    since_newline: usize,
    tripped: bool,
    pending: VecDeque<u8>,
}

impl<R> LineBoundedReader<R> {
    pub(super) fn new(inner: R, max_line: usize) -> Self {
        Self {
            inner,
            max_line,
            since_newline: 0,
            tripped: false,
            pending: VecDeque::new(),
        }
    }
}

impl<R: AsyncRead + Unpin> AsyncRead for LineBoundedReader<R> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        if self.tripped {
            return Poll::Ready(Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "MCP request line exceeded the maximum allowed length",
            )));
        }

        let start = buf.filled().len();
        while buf.remaining() > 0 {
            let Some(byte) = self.pending.pop_front() else {
                break;
            };
            buf.put_slice(&[byte]);
        }
        if buf.filled().len() > start || buf.remaining() == 0 {
            return Poll::Ready(Ok(()));
        }

        let mut storage = [0_u8; 8 * 1024];
        let mut staged = ReadBuf::new(&mut storage);
        match Pin::new(&mut self.inner).poll_read(cx, &mut staged) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Err(error)) => Poll::Ready(Err(error)),
            Poll::Ready(Ok(())) => {
                let new = staged.filled();
                for byte in new {
                    if *byte == b'\n' {
                        self.since_newline = 0;
                        continue;
                    }
                    self.since_newline = self.since_newline.saturating_add(1);
                    if self.since_newline > self.max_line {
                        self.tripped = true;
                        return Poll::Ready(Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "MCP request line exceeded the maximum allowed length",
                        )));
                    }
                }

                let delivered = buf.remaining().min(new.len());
                buf.put_slice(&new[..delivered]);
                self.pending.extend(&new[delivered..]);
                Poll::Ready(Ok(()))
            }
        }
    }
}
