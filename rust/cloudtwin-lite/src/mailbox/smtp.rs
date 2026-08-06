//! Minimal SMTP server for the mailbox feature.
//!
//! This is a small, async, hand-rolled SMTP state machine over `tokio` TCP — no
//! external SMTP crate and no TLS, since this is a local-development sink that
//! captures mail rather than relaying it. It implements just enough of RFC 5321
//! (EHLO/HELO, MAIL, RCPT, DATA, RSET, NOOP, QUIT) to accept messages from
//! common SMTP clients and test tooling.

use std::sync::Arc;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::tcp::OwnedReadHalf;
use tokio::net::{TcpListener, TcpStream};

use super::service;
use crate::AppState;

/// Hard cap on a single command line, to bound memory per connection.
const MAX_LINE: usize = 8 * 1024;
/// Hard cap on a single message body (25 MiB), to bound memory per connection.
const MAX_MESSAGE: usize = 25 * 1024 * 1024;

/// Bind and serve the SMTP listener forever. Intended to be `tokio::spawn`-ed.
pub async fn serve(state: Arc<AppState>, addr: String) {
    let listener = match TcpListener::bind(&addr).await {
        Ok(l) => l,
        Err(e) => {
            tracing::error!("Mailbox SMTP failed to bind {addr}: {e}");
            return;
        }
    };
    tracing::info!("Mailbox SMTP server listening on smtp://{addr}");

    loop {
        match listener.accept().await {
            Ok((sock, peer)) => {
                let st = state.clone();
                tokio::spawn(async move {
                    if let Err(e) = handle_session(st, sock).await {
                        tracing::debug!("SMTP session ({peer}) ended: {e}");
                    }
                });
            }
            Err(e) => tracing::warn!("SMTP accept error: {e}"),
        }
    }
}

/// Drive one client connection through the SMTP command loop.
async fn handle_session(state: Arc<AppState>, sock: TcpStream) -> std::io::Result<()> {
    let (read_half, mut write) = sock.into_split();
    let mut reader = BufReader::new(read_half);

    write.write_all(b"220 cloudtwin-lite ESMTP ready\r\n").await?;

    let mut line = String::new();
    let mut rcpts: Vec<String> = Vec::new();

    loop {
        line.clear();
        let n = read_line(&mut reader, &mut line).await?;
        if n == 0 {
            break; // client closed the connection
        }
        let trimmed = line.trim_end();
        let upper = trimmed.to_ascii_uppercase();

        if upper.starts_with("EHLO") || upper.starts_with("HELO") {
            write.write_all(b"250 cloudtwin-lite\r\n").await?;
        } else if upper.starts_with("MAIL FROM") {
            rcpts.clear();
            write.write_all(b"250 2.1.0 OK\r\n").await?;
        } else if upper.starts_with("RCPT TO") {
            rcpts.push(extract_angle(trimmed));
            write.write_all(b"250 2.1.5 OK\r\n").await?;
        } else if upper.starts_with("DATA") {
            if rcpts.is_empty() {
                write.write_all(b"503 5.5.1 RCPT first\r\n").await?;
                continue;
            }
            write
                .write_all(b"354 End data with <CR><LF>.<CR><LF>\r\n")
                .await?;
            match read_data(&mut reader).await? {
                Some(data) => match service::capture_raw(&state.db, &data, "smtp").await {
                    Ok(id) => {
                        write
                            .write_all(format!("250 2.0.0 OK: queued as {id}\r\n").as_bytes())
                            .await?;
                    }
                    Err(e) => {
                        tracing::warn!("mailbox capture failed: {e}");
                        write
                            .write_all(b"451 4.3.0 Failed to store message\r\n")
                            .await?;
                    }
                },
                None => {
                    write.write_all(b"552 5.3.4 Message too big\r\n").await?;
                }
            }
            rcpts.clear();
        } else if upper.starts_with("RSET") {
            rcpts.clear();
            write.write_all(b"250 2.0.0 OK\r\n").await?;
        } else if upper.starts_with("NOOP") {
            write.write_all(b"250 2.0.0 OK\r\n").await?;
        } else if upper.starts_with("QUIT") {
            write.write_all(b"221 2.0.0 Bye\r\n").await?;
            break;
        } else if upper.starts_with("VRFY") || upper.starts_with("EXPN") {
            write.write_all(b"252 2.5.2 Cannot verify\r\n").await?;
        } else {
            write.write_all(b"500 5.5.2 Command unrecognized\r\n").await?;
        }
    }

    Ok(())
}

/// Read a single CRLF-terminated command line, capped at [`MAX_LINE`] bytes.
async fn read_line(
    reader: &mut BufReader<OwnedReadHalf>,
    buf: &mut String,
) -> std::io::Result<usize> {
    let mut bytes: Vec<u8> = Vec::new();
    let n = reader.read_until(b'\n', &mut bytes).await?;
    if bytes.len() > MAX_LINE {
        bytes.truncate(MAX_LINE);
    }
    buf.push_str(&String::from_utf8_lossy(&bytes));
    Ok(n)
}

/// Read the DATA payload until the lone `.` terminator, performing dot
/// un-stuffing and enforcing [`MAX_MESSAGE`]. Returns `None` if the message
/// exceeds the cap.
async fn read_data(reader: &mut BufReader<OwnedReadHalf>) -> std::io::Result<Option<Vec<u8>>> {
    let mut data: Vec<u8> = Vec::new();
    let mut line: Vec<u8> = Vec::new();
    let mut overflow = false;

    loop {
        line.clear();
        let n = reader.read_until(b'\n', &mut line).await?;
        if n == 0 {
            break; // EOF mid-stream
        }
        if line == b".\r\n" || line == b".\n" || line == b"." {
            break; // end of DATA
        }
        // RFC 5321 dot-stuffing: a leading '.' is doubled on the wire.
        let content: &[u8] = if line.starts_with(b"..") {
            &line[1..]
        } else {
            &line
        };
        if !overflow {
            if data.len() + content.len() > MAX_MESSAGE {
                overflow = true;
            } else {
                data.extend_from_slice(content);
            }
        }
    }

    if overflow {
        Ok(None)
    } else {
        Ok(Some(data))
    }
}

/// Extract the address inside `<...>` from a `MAIL FROM`/`RCPT TO` command,
/// falling back to whatever follows the first `:`.
fn extract_angle(s: &str) -> String {
    if let (Some(a), Some(b)) = (s.find('<'), s.rfind('>')) {
        if b > a {
            return s[a + 1..b].to_string();
        }
    }
    s.split_once(':')
        .map(|(_, rest)| rest.trim().to_string())
        .unwrap_or_default()
}
