//! The event stream's reader: the WHATWG format as far as home uses it (`id`, `event`, `data`,
//! comments), fed bytes in any chunking.

/// The most one event may hold: its data and the line not yet ended.
pub(crate) const CAP: usize = 16 * 1024 * 1024;

/// The most an `event` name or an `id` may hold: home's are short, and a longer one is held, and
/// sent back as `Last-Event-ID`, for nothing.
pub(crate) const NAME_CAP: usize = 256;

#[derive(Debug, PartialEq)]
pub(crate) struct Event {
    pub event: String,
    pub data: String,
    pub id: Option<String>,
}

/// An event past [`CAP`]: the stream is dropped and reopened.
#[derive(Debug, PartialEq)]
pub(crate) struct TooLarge;

#[derive(Default)]
pub(crate) struct Reader {
    /// The line being read, not yet ended.
    line: Vec<u8>,
    /// The last byte was a `\r`: a `\n` right after it belongs to the same line end.
    after_cr: bool,
    event: Option<String>,
    data: Option<String>,
    id: Option<String>,
}

impl Reader {
    /// The events `bytes` completes.
    pub(crate) fn push(&mut self, bytes: &[u8]) -> Result<Vec<Event>, TooLarge> {
        let mut events = Vec::new();
        for &byte in bytes {
            let was_cr = std::mem::take(&mut self.after_cr);
            match byte {
                b'\n' if was_cr => {}
                b'\n' | b'\r' => {
                    self.after_cr = byte == b'\r';
                    let line = std::mem::take(&mut self.line);
                    if let Some(event) = self.read(&line)? {
                        events.push(event);
                    }
                }
                _ => self.line.push(byte),
            }
            let held = self.line.len() + self.data.as_ref().map_or(0, String::len);
            if held > CAP {
                return Err(TooLarge);
            }
        }
        Ok(events)
    }

    /// One line; the event a blank one dispatches.
    fn read(&mut self, line: &[u8]) -> Result<Option<Event>, TooLarge> {
        if line.is_empty() {
            let event = self.event.take();
            let id = self.id.take();
            return Ok(self.data.take().map(|data| Event {
                event: event.unwrap_or_else(|| "message".into()),
                data,
                id,
            }));
        }
        if line[0] == b':' {
            return Ok(None);
        }
        let text = String::from_utf8_lossy(line);
        let (field, value) = text.split_once(':').unwrap_or((&text, ""));
        let value = value.strip_prefix(' ').unwrap_or(value);
        if matches!(field, "event" | "id") && value.len() > NAME_CAP {
            return Err(TooLarge);
        }
        match field {
            "event" => self.event = Some(value.into()),
            "data" => match &mut self.data {
                Some(data) => {
                    data.push('\n');
                    data.push_str(value);
                }
                None => self.data = Some(value.into()),
            },
            "id" if !value.contains('\0') => self.id = Some(value.into()),
            _ => {}
        }
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_are_read_across_any_chunking() {
        let text = b": ping\n\nid: a-1\nevent: block\ndata: {\"op\":\"put\"}\n\nid: a-2\r\nevent: frame\r\ndata: x\r\ndata: y\r\n\r\n";
        let wanted = vec![
            Event {
                event: "block".into(),
                data: "{\"op\":\"put\"}".into(),
                id: Some("a-1".into()),
            },
            Event {
                event: "frame".into(),
                data: "x\ny".into(),
                id: Some("a-2".into()),
            },
        ];
        let mut whole = Reader::default();
        assert_eq!(whole.push(text), Ok(wanted));

        let mut bytes = Reader::default();
        let mut got = Vec::new();
        for byte in text {
            got.extend(bytes.push(&[*byte]).unwrap());
        }
        assert_eq!(got.len(), 2);
        assert_eq!(got[1].data, "x\ny");
        assert_eq!(got[1].id.as_deref(), Some("a-2"));

        let mut empty = Reader::default();
        assert_eq!(
            empty.push(b"data\n\n"),
            Ok(vec![Event {
                event: "message".into(),
                data: String::new(),
                id: None
            }])
        );
        assert_eq!(empty.push(b"event: x\n\n"), Ok(vec![]), "no data, no event");
        assert_eq!(
            empty.push(b"id: a\0b\ndata: 1\n\n").unwrap()[0].id,
            None,
            "an id with a NUL is ignored"
        );
    }

    #[test]
    fn a_long_event_name_or_id_is_too_large() {
        for field in ["event", "id"] {
            let mut ok = Reader::default();
            let line = format!("{field}: {}\ndata: 1\n\n", "a".repeat(NAME_CAP));
            assert_eq!(ok.push(line.as_bytes()).unwrap().len(), 1, "{field}");
            let mut reader = Reader::default();
            let line = format!("{field}: {}\n", "a".repeat(NAME_CAP + 1));
            assert_eq!(reader.push(line.as_bytes()), Err(TooLarge), "{field}");
        }
    }

    #[test]
    fn an_event_past_the_cap_is_too_large() {
        let mut reader = Reader::default();
        reader.push(b"data: ").unwrap();
        let chunk = vec![b'x'; 1024 * 1024];
        let mut result = Ok(vec![]);
        for _ in 0..=CAP / chunk.len() {
            result = reader.push(&chunk);
            if result.is_err() {
                break;
            }
        }
        assert_eq!(result, Err(TooLarge));

        let mut fresh = Reader::default();
        let mut event = b"data: ".to_vec();
        event.extend(vec![b'y'; 1024 * 1024]);
        event.extend(b"\n\n");
        let mut count = 0;
        for _ in 0..10 {
            count += fresh.push(&event).unwrap().len();
        }
        assert_eq!(count, 10);
    }
}
