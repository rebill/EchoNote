//! Server-sent events, fed in arbitrary chunks. Only `event` and `data` fields matter here.

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Event {
    pub event: Option<String>,
    pub data: String,
}

#[derive(Default)]
pub struct Parser {
    buffer: Vec<u8>,
    event: Option<String>,
    data: Vec<String>,
    has_data: bool,
}

impl Parser {
    /// Events completed by `chunk`. Lines end with LF, CRLF or CR; a line split across
    /// chunks waits for the rest.
    pub fn feed(&mut self, chunk: &[u8]) -> Result<Vec<Event>, &'static str> {
        self.buffer.extend_from_slice(chunk);
        let mut events = Vec::new();
        let mut start = 0;
        let mut index = 0;
        while index < self.buffer.len() {
            let byte = self.buffer[index];
            if byte != b'\n' && byte != b'\r' {
                index += 1;
                continue;
            }
            // A CR at the end of the buffer may be the first half of a CRLF.
            if byte == b'\r' && index + 1 == self.buffer.len() {
                break;
            }
            let line = std::str::from_utf8(&self.buffer[start..index])
                .map_err(|_| "the event stream is not UTF-8")?
                .to_owned();
            if let Some(event) = self.line(&line) {
                events.push(event);
            }
            index += if byte == b'\r' && self.buffer[index + 1] == b'\n' {
                2
            } else {
                1
            };
            start = index;
        }
        self.buffer.drain(..start);
        Ok(events)
    }

    fn line(&mut self, line: &str) -> Option<Event> {
        if line.is_empty() {
            if !self.has_data {
                self.event = None;
                return None;
            }
            self.has_data = false;
            return Some(Event {
                event: self.event.take(),
                data: std::mem::take(&mut self.data).join("\n"),
            });
        }
        if line.starts_with(':') {
            return None;
        }
        let (field, value) = line.split_once(':').unwrap_or((line, ""));
        let value = value.strip_prefix(' ').unwrap_or(value);
        match field {
            "data" => {
                self.data.push(value.to_owned());
                self.has_data = true;
            }
            "event" => self.event = Some(value.to_owned()),
            _ => {}
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data(events: &[Event]) -> Vec<&str> {
        events.iter().map(|event| event.data.as_str()).collect()
    }

    #[test]
    fn events_survive_any_chunking() {
        let stream =
            "event: a\r\ndata: 1\r\n\r\n: comment\ndata: 2\ndata: 3\n\ndata:4\r\rdata: 你好\n\n";
        let whole = Parser::default().feed(stream.as_bytes()).unwrap();
        assert_eq!(data(&whole), ["1", "2\n3", "4", "你好"]);
        assert_eq!(whole[0].event.as_deref(), Some("a"));
        // Byte by byte, splitting CRLF pairs and UTF-8 sequences.
        let mut parser = Parser::default();
        let mut events = Vec::new();
        for byte in stream.as_bytes() {
            events.extend(parser.feed(std::slice::from_ref(byte)).unwrap());
        }
        assert_eq!(events, whole);
    }

    #[test]
    fn an_unfinished_event_is_held() {
        let mut parser = Parser::default();
        assert!(parser.feed(b"data: partial").unwrap().is_empty());
        assert_eq!(data(&parser.feed(b"\n\n").unwrap()), ["partial"]);
        assert!(parser.feed(&[0xff, b'\n']).is_err());
    }
}
