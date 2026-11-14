//! A chat's stdout as NDJSON: one JSON object a line. Blank lines and lines that don't start with
//! `{` (`[SandboxDebug]` and the like) are skipped; a line that starts with `{` and doesn't parse,
//! or any line longer than `LINE_CAP`, ends the read with what went wrong, so the tab says so
//! instead of waiting on a process whose wire is lost.

use std::io::{BufRead, BufReader, Read};

use serde_json::Value;

/// The longest line read: a replayed image or an MCP result can be several MB, never this.
pub const LINE_CAP: usize = 64 * 1024 * 1024;

/// How a read ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum End {
    /// The process closed its stdout.
    Eof,
    /// A line grew past `LINE_CAP`: nothing more of it was held.
    TooLong,
    /// A line that starts with `{` doesn't parse.
    Broken,
    /// The pipe failed.
    Failed,
}

/// Reads `source` to its end, handing each object on to `frame`, with lines capped at `cap`
/// (`LINE_CAP`; smaller in tests). `frame` returns false to stop.
pub fn read(source: impl Read, cap: usize, mut frame: impl FnMut(Value) -> bool) -> End {
    let mut reader = BufReader::with_capacity(64 * 1024, source);
    let mut line: Vec<u8> = Vec::new();
    loop {
        let (done, used) = match reader.fill_buf() {
            Ok([]) => {
                // The last line, without its line break.
                return match take(&line, &mut frame) {
                    Some(false) => End::Broken,
                    _ => End::Eof,
                };
            }
            Ok(chunk) => match chunk.iter().position(|byte| *byte == b'\n') {
                Some(at) => {
                    line.extend_from_slice(&chunk[..at]);
                    (true, at + 1)
                }
                None => {
                    line.extend_from_slice(chunk);
                    (false, chunk.len())
                }
            },
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => return End::Failed,
        };
        reader.consume(used);
        if line.len() > cap {
            return End::TooLong;
        }
        if done {
            match take(&line, &mut frame) {
                Some(false) => return End::Broken,
                None => return End::Eof,
                Some(true) => {}
            }
            line.clear();
        }
    }
}

/// One line: skipped (`Some(true)`), handed on (`Some(true)`, or `None` when `frame` says stop)
/// or broken (`Some(false)`).
fn take(line: &[u8], frame: &mut impl FnMut(Value) -> bool) -> Option<bool> {
    let start = line.iter().position(|byte| !byte.is_ascii_whitespace());
    let Some(start) = start.filter(|&start| line[start] == b'{') else {
        return Some(true);
    };
    match serde_json::from_slice::<Value>(&line[start..]) {
        Ok(value) if value.is_object() => frame(value).then_some(true),
        _ => Some(false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test 6: a line over the cap ends the read with an error instead of growing; lines that
    /// aren't objects are skipped; a broken `{` line ends it with an error (a silent hang
    /// otherwise).
    #[test]
    fn a_wire_that_cant_be_read_ends_the_chat() {
        let mut seen = Vec::new();
        let input = "\n[SandboxDebug] hello\n  \n{\"type\":\"a\"}\nnot json\n{\"type\":\"b\"}";
        assert_eq!(read(input.as_bytes(), 1024, |value| { seen.push(value["type"].as_str().unwrap().to_string()); true }), End::Eof);
        assert_eq!(seen, ["a", "b"]);

        let mut seen = 0;
        assert_eq!(read("{\"type\":\"a\"}\n{\"type\": broken\n{\"type\":\"c\"}\n".as_bytes(), 1024, |_| { seen += 1; true }), End::Broken);
        assert_eq!(seen, 1, "nothing after the broken line");

        // A line that never ends, longer than the cap: the read stops, holding no more than the cap
        // and a chunk.
        struct Endless;
        impl Read for Endless {
            fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
                buf.fill(b'x');
                Ok(buf.len())
            }
        }
        let mut endless = std::io::Cursor::new(b"{\"type\":\"a\"}\n{\"type\":\"".to_vec()).chain(Endless);
        let mut seen = 0;
        assert_eq!(read(&mut endless, 1024 * 1024, |_| { seen += 1; true }), End::TooLong);
        assert_eq!(seen, 1);
    }
}
