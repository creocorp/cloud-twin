//! `aws-chunked` request body decoding (SigV4 streaming uploads).

use axum::http::HeaderMap;

pub struct Decoded {
    pub data: Vec<u8>,
    /// Trailing headers (`x-amz-checksum-*`), names lowercase.
    pub trailers: Vec<(String, String)>,
}

/// Whether the request body uses the `aws-chunked` framing.
pub fn is_aws_chunked(headers: &HeaderMap) -> bool {
    let sha = headers
        .get("x-amz-content-sha256")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let enc = headers
        .get("content-encoding")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    sha.starts_with("STREAMING-")
        || enc
            .split(',')
            .any(|e| e.trim().eq_ignore_ascii_case("aws-chunked"))
}

fn find_crlf(b: &[u8], from: usize) -> Option<usize> {
    b.get(from..)?
        .windows(2)
        .position(|w| w == b"\r\n")
        .map(|p| from + p)
}

/// Decode `<hex>[;chunk-signature=..]\r\n<data>\r\n ... 0\r\n<trailers>\r\n\r\n`.
pub fn decode(body: &[u8]) -> Result<Decoded, String> {
    let mut pos = 0;
    let mut data = Vec::with_capacity(body.len());
    loop {
        let end = find_crlf(body, pos).ok_or("missing chunk header terminator")?;
        let line = std::str::from_utf8(&body[pos..end]).map_err(|e| e.to_string())?;
        let size_hex = line.split(';').next().unwrap_or("").trim();
        let size = usize::from_str_radix(size_hex, 16)
            .map_err(|_| format!("bad chunk size {size_hex:?}"))?;
        pos = end + 2;
        if size == 0 {
            break;
        }
        let chunk = body.get(pos..pos + size).ok_or("truncated chunk")?;
        data.extend_from_slice(chunk);
        pos += size;
        if body.get(pos..pos + 2) == Some(b"\r\n") {
            pos += 2;
        }
    }
    let mut trailers = Vec::new();
    let rest = String::from_utf8_lossy(body.get(pos..).unwrap_or_default());
    for line in rest.split("\r\n") {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some((k, v)) = line.split_once(':') {
            let k = k.trim().to_ascii_lowercase();
            if k != "x-amz-trailer-signature" {
                trailers.push((k, v.trim().to_string()));
            }
        }
    }
    Ok(Decoded { data, trailers })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_signed_and_unsigned_chunks_with_trailers() {
        let body = b"5;chunk-signature=abc\r\nhello\r\n6;chunk-signature=def\r\n world\r\n0;chunk-signature=x\r\nx-amz-checksum-crc32:DUoRhQ==\r\nx-amz-trailer-signature:zzz\r\n\r\n";
        let d = decode(body).unwrap();
        assert_eq!(d.data, b"hello world");
        assert_eq!(
            d.trailers,
            vec![("x-amz-checksum-crc32".to_string(), "DUoRhQ==".to_string())]
        );

        let d = decode(b"3\r\nabc\r\n0\r\n\r\n").unwrap();
        assert_eq!(d.data, b"abc");
        assert!(d.trailers.is_empty());
    }
}
