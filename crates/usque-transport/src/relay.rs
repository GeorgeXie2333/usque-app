use tokio::io::{AsyncRead, AsyncWrite};

/// Large enough to amortize task and userspace-stack wakeups without creating
/// an unbounded queue per connection.
pub(crate) const RELAY_BUFFER_SIZE: usize = 128 * 1024;

pub(crate) async fn copy_bidirectional<A, B>(
    left: &mut A,
    right: &mut B,
) -> std::io::Result<(u64, u64)>
where
    A: AsyncRead + AsyncWrite + Unpin,
    B: AsyncRead + AsyncWrite + Unpin,
{
    copy_bidirectional_with_buffer(left, right, RELAY_BUFFER_SIZE).await
}

pub(crate) async fn copy_bidirectional_with_buffer<A, B>(
    left: &mut A,
    right: &mut B,
    buffer_size: usize,
) -> std::io::Result<(u64, u64)>
where
    A: AsyncRead + AsyncWrite + Unpin,
    B: AsyncRead + AsyncWrite + Unpin,
{
    tokio::io::copy_bidirectional_with_sizes(left, right, buffer_size, buffer_size).await
}
