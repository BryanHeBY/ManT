//! Bounded stdio transport for the local MCP subprocess.

use std::{
    io,
    pin::Pin,
    task::{Context, Poll},
};

use rmcp::ServiceExt;
use tokio::io::{AsyncRead, ReadBuf};

use super::MantMcpServer;

/// Upper bound on one newline-delimited MCP request, in bytes.
pub(super) const MAX_MCP_LINE_BYTES: usize = 8 * 1024 * 1024;

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
}

impl<R> LineBoundedReader<R> {
    pub(super) fn new(inner: R, max_line: usize) -> Self {
        Self {
            inner,
            max_line,
            since_newline: 0,
            tripped: false,
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
        let poll = Pin::new(&mut self.inner).poll_read(cx, buf);
        if let Poll::Ready(Ok(())) = &poll {
            let new = &buf.filled()[start..];
            match new.iter().rposition(|&byte| byte == b'\n') {
                Some(last_newline) => self.since_newline = new.len() - last_newline - 1,
                None => self.since_newline += new.len(),
            }
            self.tripped = self.since_newline > self.max_line;
        }
        poll
    }
}
