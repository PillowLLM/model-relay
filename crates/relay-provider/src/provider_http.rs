//! HTTP/SSE 公共辅助。
use futures::Stream;
use futures::StreamExt;
use std::pin::Pin;

/// 把 reqwest 字节流按行切分（SSE：以 \n 分隔）。
pub fn sse_lines<S>(byte_stream: S) -> Pin<Box<dyn Stream<Item = Result<String, std::io::Error>> + Send>>
where
    S: Stream<Item = Result<bytes::Bytes, reqwest::Error>> + Unpin + Send + 'static,
{
    Box::pin(async_stream::stream! {
        let mut byte_stream = byte_stream;
        let mut buf: Vec<u8> = Vec::new();
        while let Some(chunk) = byte_stream.next().await {
            match chunk {
                Ok(bytes) => {
                    buf.extend_from_slice(&bytes);
                    while let Some(pos) = buf.iter().position(|&b| b == b'\n') {
                        let line: Vec<u8> = buf.drain(..=pos).collect();
                        let line = if line.last() == Some(&b'\r') { &line[..line.len() - 1] } else { &line[..] };
                        if let Ok(s) = std::str::from_utf8(line) {
                            yield Ok(s.to_string());
                        }
                    }
                }
                Err(e) => yield Err(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())),
            }
        }
    })
}
