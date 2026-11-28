//! Low-level Unix process-group signaling / liveness checks and bounded, UTF-8-lossy
//! line reading for managed Run commands and shell environment probes.

use std::future::Future;
use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};

const LOG_READER_BUF_CAPACITY: usize = 8 * 1024;
const TRUNCATED_LINE_SUFFIX: &str = " … [truncated]";

#[cfg(unix)]
unsafe extern "C" {
    fn kill(pid: i32, sig: i32) -> i32;
}

#[cfg(unix)]
pub(super) const SIGTERM: i32 = 15;
#[cfg(unix)]
pub(super) const SIGKILL: i32 = 9;
#[cfg(unix)]
const EPERM: i32 = 1;

/// Sends a POSIX signal to the entire process group `-pgid`.
#[cfg(unix)]
pub(super) fn signal_process_group(pgid: u32, sig: i32) {
    if let Ok(raw) = i32::try_from(pgid)
        && raw > 0
    {
        // SAFETY: `pgid` was created via `process_group(0)` for a child owned by this process.
        unsafe {
            kill(-raw, sig);
        }
    }
}

#[cfg(not(unix))]
pub(super) fn signal_process_group(_pgid: u32, _sig: i32) {}

/// Sends a POSIX signal directly to a single un-reaped child PID as belt-and-suspenders
/// alongside `signal_process_group` (e.g. if a child detached itself into a new process group
/// before exiting). Callers must ONLY call this before `child.wait()` has reaped `pid`.
#[cfg(unix)]
pub(super) fn signal_pid_direct(pid: u32, sig: i32) {
    if let Ok(raw) = i32::try_from(pid)
        && raw > 0
    {
        // SAFETY: Caller guarantees `pid` is an un-reaped child owned by this process.
        unsafe {
            kill(raw, sig);
        }
    }
}

#[cfg(not(unix))]
pub(super) fn signal_pid_direct(_pid: u32, _sig: i32) {}

/// Returns `true` if ANY process still belongs to process group `pgid`.
/// After the group leader has been reaped by `child.wait()`, this accurately detects
/// surviving grandchildren that outlived the shell or ignored `SIGTERM`.
#[cfg(unix)]
pub(super) fn is_process_group_alive(pgid: u32) -> bool {
    let Ok(raw) = i32::try_from(pgid) else {
        return false;
    };
    if raw <= 0 {
        return false;
    }
    // SAFETY: Signal 0 performs an existence check on process group `-raw` without delivering a signal.
    let rc = unsafe { kill(-raw, 0) };
    if rc == 0 {
        return true;
    }
    std::io::Error::last_os_error().raw_os_error() == Some(EPERM)
}

#[cfg(not(unix))]
pub(super) fn is_process_group_alive(_pgid: u32) -> bool {
    false
}

/// Immediately sends `SIGKILL` to the entire process group `-pgid`.
pub(super) fn terminate_process_group_hard(pgid: u32) {
    #[cfg(unix)]
    signal_process_group(pgid, SIGKILL);
    #[cfg(not(unix))]
    let _ = pgid;
}

/// RAII drop guard that force-kills a spawned process group if an async task is cancelled
/// or times out before the child exits cleanly.
pub(super) struct ProcessGroupKillGuard {
    pgid: Option<u32>,
}

impl ProcessGroupKillGuard {
    pub(super) fn new(pgid: Option<u32>) -> Self {
        Self { pgid }
    }

    pub(super) fn disarm(&mut self) {
        self.pgid = None;
    }
}

impl Drop for ProcessGroupKillGuard {
    fn drop(&mut self) {
        if let Some(pgid) = self.pgid.take() {
            terminate_process_group_hard(pgid);
        }
    }
}

/// Trims an incomplete valid multi-byte UTF-8 sequence at the tail of a byte buffer that was
/// cut at a hard byte limit. Truly invalid bytes (e.g. `0xFF`) are preserved so
/// `String::from_utf8_lossy` converts them to `U+FFFD`.
pub(super) fn trim_incomplete_trailing_utf8(bytes: &[u8]) -> &[u8] {
    if bytes.is_empty() {
        return bytes;
    }
    let min_start = bytes.len().saturating_sub(3);
    let mut idx = bytes.len() - 1;
    while idx > min_start && (bytes[idx] & 0b1100_0000) == 0b1000_0000 {
        idx -= 1;
    }
    match std::str::from_utf8(&bytes[idx..]) {
        Err(err) if err.valid_up_to() == 0 && err.error_len().is_none() => &bytes[..idx],
        _ => bytes,
    }
}

fn decode_lossy_line(raw: &[u8], truncated: bool) -> String {
    let slice = if truncated {
        trim_incomplete_trailing_utf8(raw)
    } else {
        raw
    };
    let mut decoded = String::from_utf8_lossy(slice).into_owned();
    if truncated {
        decoded.push_str(TRUNCATED_LINE_SUFFIX);
    }
    decoded
}

/// Defense-in-depth cap ensuring no single stored log line exceeds `max_bytes` of payload text.
pub(super) fn cap_log_text(text: String, max_bytes: usize) -> String {
    if max_bytes == 0 || text.len() <= max_bytes {
        return text;
    }
    if text.ends_with(TRUNCATED_LINE_SUFFIX)
        && text.len() <= max_bytes.saturating_add(TRUNCATED_LINE_SUFFIX.len())
    {
        return text;
    }
    let mut end = max_bytes;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    let mut out = String::with_capacity(end + TRUNCATED_LINE_SUFFIX.len());
    out.push_str(&text[..end]);
    out.push_str(TRUNCATED_LINE_SUFFIX);
    out
}

/// Reads `\n`-delimited lines from an arbitrary byte stream with strict O(1) memory bounds
/// (`<= max_line_bytes` buffered per line) and lossy UTF-8 decoding.
///
/// - Invalid UTF-8 bytes are replaced with `U+FFFD` (``) instead of terminating the stream.
/// - Lines longer than `max_line_bytes` emit the capped prefix with a truncation marker
///   immediately and discard the remainder up to the next `\n` without allocating.
/// - Trailing `\r` before `\n` is stripped.
pub(super) async fn read_bounded_lossy_lines<R, F, Fut>(
    reader: R,
    max_line_bytes: usize,
    mut on_line: F,
) where
    R: AsyncRead + Unpin,
    F: FnMut(String) -> Fut,
    Fut: Future<Output = bool>,
{
    let cap = max_line_bytes.max(1);
    let mut buf_reader = BufReader::with_capacity(LOG_READER_BUF_CAPACITY, reader);
    let mut line_buf: Vec<u8> = Vec::new();
    let mut discarding_until_newline = false;

    loop {
        let available = match buf_reader.fill_buf().await {
            Ok([]) => break,
            Ok(bytes) => bytes,
            Err(_) => break,
        };

        if discarding_until_newline {
            if let Some(nl_idx) = available.iter().position(|&b| b == b'\n') {
                buf_reader.consume(nl_idx + 1);
                discarding_until_newline = false;
            } else {
                let len = available.len();
                buf_reader.consume(len);
            }
            continue;
        }

        let (slice_end, found_newline) = match available.iter().position(|&b| b == b'\n') {
            Some(idx) => (idx, true),
            None => (available.len(), false),
        };
        let remaining_cap = cap.saturating_sub(line_buf.len());

        if slice_end <= remaining_cap {
            line_buf.extend_from_slice(&available[..slice_end]);
            buf_reader.consume(if found_newline {
                slice_end + 1
            } else {
                slice_end
            });
            if found_newline {
                if line_buf.last() == Some(&b'\r') {
                    line_buf.pop();
                }
                let line = decode_lossy_line(&line_buf, false);
                line_buf.clear();
                if !on_line(line).await {
                    return;
                }
            }
        } else {
            line_buf.extend_from_slice(&available[..remaining_cap]);
            let consume_len = if found_newline {
                slice_end + 1
            } else {
                available.len()
            };
            buf_reader.consume(consume_len);
            discarding_until_newline = !found_newline;

            let line = decode_lossy_line(&line_buf, true);
            line_buf.clear();
            if !on_line(line).await {
                return;
            }
        }
    }

    if !discarding_until_newline && !line_buf.is_empty() {
        if line_buf.last() == Some(&b'\r') {
            line_buf.pop();
        }
        let line = decode_lossy_line(&line_buf, false);
        let _ = on_line(line).await;
    }
}
