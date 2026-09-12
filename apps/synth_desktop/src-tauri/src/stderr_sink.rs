//! Detached bounded stderr sink. Redaction happens before any disk write.
use std::io::{self, BufRead, Seek, SeekFrom, Write};

pub const MODE: &str = "--workshop-codex-stderr-sink";
const LOG_BYTES: usize = 1024 * 1024;
const LINE_BYTES: usize = 64 * 1024;

pub fn drain(
    mut input: impl BufRead,
    mut output: std::fs::File,
    redact: impl Fn(&str) -> String,
) -> io::Result<()> {
    let mut line = Vec::new();
    let mut dropping = false;
    let mut written = 0;
    let mut write_error = None;
    loop {
        let bytes = input.fill_buf()?;
        let eof = bytes.is_empty();
        let count = bytes
            .iter()
            .position(|byte| *byte == b'\n')
            .map(|n| n + 1)
            .unwrap_or(bytes.len());
        let end = eof || bytes.get(count.saturating_sub(1)) == Some(&b'\n');
        if !dropping && line.len() + count <= LINE_BYTES {
            line.extend_from_slice(&bytes[..count]);
        } else {
            dropping = true;
            line.clear();
        }
        input.consume(count);
        if end {
            let safe = if dropping {
                "[oversized stderr line omitted]\n".to_owned()
            } else {
                redact(&String::from_utf8_lossy(&line))
            };
            if safe.len() <= LOG_BYTES && write_error.is_none() {
                let result = (|| -> io::Result<()> {
                    if written + safe.len() > LOG_BYTES {
                        output.set_len(0)?;
                        output.seek(SeekFrom::Start(0))?;
                        written = 0;
                    }
                    output.write_all(safe.as_bytes())?;
                    output.flush()?;
                    written += safe.len();
                    Ok(())
                })();
                // A failed log volume must not close Codex's stderr pipe.
                // Drain to EOF, then report failure to the sink's parent.
                if let Err(error) = result {
                    write_error = Some(error);
                }
            }
            line.clear();
            dropping = false;
        }
        if eof {
            return write_error.map_or(Ok(()), Err);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn write_failure_still_drains_until_eof() {
        let path =
            std::env::temp_dir().join(format!("stderr-sink-readonly-{}", std::process::id()));
        std::fs::write(&path, "").unwrap();
        let output = std::fs::File::open(&path).unwrap();
        let mut input = io::Cursor::new(b"first\nsecond\nthird\n");
        assert!(drain(&mut input, output, str::to_owned).is_err());
        assert_eq!(input.position(), input.get_ref().len() as u64);
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn redacts_before_writing_and_bounds_disk_and_lines() {
        let path = std::env::temp_dir().join(format!("stderr-sink-{}", std::process::id()));
        let file = std::fs::File::create(&path).unwrap();
        let input = format!(
            "{}\n{}",
            "secret".repeat(LINE_BYTES),
            "secret\n".repeat(LOG_BYTES)
        );
        drain(io::Cursor::new(input), file, |text| {
            text.replace("secret", "[redacted]")
        })
        .unwrap();
        let stored = std::fs::read_to_string(&path).unwrap();
        assert!(stored.len() <= LOG_BYTES);
        assert!(!stored.contains("secret"));
        assert!(stored.contains("[redacted]"));
        std::fs::remove_file(path).unwrap();
    }
}
