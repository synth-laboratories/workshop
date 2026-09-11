//! Bounded byte framing only. Protocol adapters validate identity and commit
//! data before saving an event ID; parsing is never a durable acknowledgement.
#[derive(Debug, PartialEq, Eq)]
pub struct Frame {
    pub event: String,
    pub data: String,
    pub id: Option<String>,
}

pub struct Decoder {
    pending: Vec<u8>,
    data: String,
    event: String,
    id: Option<String>,
    retry_ms: Option<u64>,
    first_line: bool,
    skip_lf: bool,
    limit: usize,
}

impl Decoder {
    pub fn new(limit: usize) -> Self {
        Self {
            pending: Vec::new(),
            data: String::new(),
            event: String::new(),
            id: None,
            retry_ms: None,
            first_line: true,
            skip_lf: false,
            limit,
        }
    }

    pub fn retry_ms(&self) -> Option<u64> {
        self.retry_ms
    }

    /// Discard this decoder after any error; resume from the caller's committed checkpoint.
    pub fn push(&mut self, bytes: &[u8]) -> Result<Vec<Frame>, &'static str> {
        let mut frames = Vec::new();
        for byte in bytes {
            if self.skip_lf && *byte == b'\n' {
                self.skip_lf = false;
                continue;
            }
            self.skip_lf = false;
            if *byte == b'\n' || *byte == b'\r' {
                self.skip_lf = *byte == b'\r';
                let line = std::mem::take(&mut self.pending);
                let line = line.strip_suffix(b"\r").unwrap_or(&line);
                let line = std::str::from_utf8(line).map_err(|_| "invalid SSE UTF-8")?;
                let line = if self.first_line {
                    line.trim_start_matches('\u{feff}')
                } else {
                    line
                };
                self.first_line = false;
                if line.is_empty() {
                    if !self.data.is_empty() {
                        self.data.pop(); // strip the final data-field newline
                        frames.push(Frame {
                            event: if self.event.is_empty() {
                                "message".into()
                            } else {
                                std::mem::take(&mut self.event)
                            },
                            data: std::mem::take(&mut self.data),
                            id: self.id.clone(),
                        });
                    }
                    self.event.clear();
                    continue;
                }
                if line.starts_with(':') {
                    continue;
                }
                let (field, value) = line.split_once(':').unwrap_or((line, ""));
                let value = value.strip_prefix(' ').unwrap_or(value);
                match field {
                    "data" => {
                        if self
                            .data
                            .len()
                            .saturating_add(value.len())
                            .saturating_add(1)
                            > self.limit
                        {
                            return Err("SSE event exceeds limit");
                        }
                        self.data.push_str(value);
                        self.data.push('\n');
                    }
                    "event" => self.event = value.into(),
                    "id" if !value.contains('\0') => self.id = Some(value.into()),
                    "retry" if !value.is_empty() && value.bytes().all(|c| c.is_ascii_digit()) => {
                        if let Ok(retry) = value.parse() {
                            self.retry_ms = Some(retry);
                        }
                    }
                    _ => {}
                }
            } else {
                if self.pending.len() >= self.limit {
                    return Err("SSE line exceeds limit");
                }
                self.pending.push(*byte);
            }
        }
        Ok(frames)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fragmented_utf8_crlf_multiline_and_comments() {
        let input = "\u{feff}: heartbeat\r\nretry: 123\r\nid: opaque:1\r\nevent: transcript\r\ndata: λ\r\ndata: second\r\n\r\n";
        let mut decoder = Decoder::new(256);
        let frames: Vec<_> = input
            .as_bytes()
            .iter()
            .flat_map(|byte| decoder.push(&[*byte]).unwrap())
            .collect();
        assert_eq!(
            frames,
            vec![Frame {
                event: "transcript".into(),
                data: "λ\nsecond".into(),
                id: Some("opaque:1".into())
            }]
        );
        assert_eq!(decoder.retry_ms(), Some(123));
        assert!(decoder.push(b": heartbeat\n\n").unwrap().is_empty());
    }
    #[test]
    fn bounded_and_never_dispatches_partial_event() {
        assert!(Decoder::new(3).push(b"data").is_err());
        let mut decoder = Decoder::new(16);
        assert!(decoder.push(b"data: pending\n").unwrap().is_empty());
        assert!(decoder.push(b"data: more-data\n").is_err());
    }
    #[test]
    fn null_id_is_ignored_and_empty_id_resets() {
        let mut decoder = Decoder::new(128);
        let frames = decoder
            .push(b"id: first\ndata: 1\n\nid: bad\0id\ndata: 2\n\nid:\ndata: 3\n\n")
            .unwrap();
        assert_eq!(frames[1].id.as_deref(), Some("first"));
        assert_eq!(frames[2].id.as_deref(), Some(""));
    }
    #[test]
    fn lone_cr_and_split_crlf_dispatch_once() {
        let mut decoder = Decoder::new(128);
        assert!(decoder.push(b"data: a\r").unwrap().is_empty());
        assert_eq!(decoder.push(b"\n\r").unwrap().len(), 1);
        assert!(decoder.push(b"\n").unwrap().is_empty());
        assert_eq!(decoder.push(b"data: b\r\r").unwrap()[0].data, "b");
    }
}
