//! Mojang's logging config makes the game print XML events meant for a
//! launcher to read. This turns them back into lines a person can follow.

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LogLine {
    pub text: String,
    pub is_error: bool,
}

#[derive(Default)]
pub struct LogReader {
    event: Option<PartialEvent>,
}

#[derive(Default)]
struct PartialEvent {
    level: String,
    thread: String,
    body: String,
    in_cdata: bool,
}

const CDATA_START: &str = "<![CDATA[";
const CDATA_END: &str = "]]>";

fn attribute<'a>(tag: &'a str, name: &str) -> &'a str {
    let marker = format!("{name}=\"");
    tag.find(&marker)
        .map(|at| at + marker.len())
        .and_then(|from| tag[from..].find('"').map(|len| &tag[from..from + len]))
        .unwrap_or("")
}

impl LogReader {
    /// Takes one line of game output and returns any finished log lines.
    pub fn feed(&mut self, line: &str) -> Vec<LogLine> {
        let trimmed = line.trim_start();
        if self.event.is_none() {
            if trimmed.starts_with("<log4j:Event") {
                self.event = Some(PartialEvent {
                    level: attribute(trimmed, "level").to_string(),
                    thread: attribute(trimmed, "thread").to_string(),
                    ..PartialEvent::default()
                });
                return Vec::new();
            }
            return vec![LogLine { text: line.to_string(), is_error: false }];
        }

        let event = self.event.as_mut().expect("checked above");
        if event.in_cdata {
            match line.find(CDATA_END) {
                Some(end) => {
                    event.body.push_str(&line[..end]);
                    event.in_cdata = false;
                }
                None => {
                    event.body.push_str(line);
                    event.body.push('\n');
                }
            }
            return Vec::new();
        }
        if let Some(start) = line.find(CDATA_START) {
            let rest = &line[start + CDATA_START.len()..];
            if !event.body.is_empty() {
                event.body.push('\n');
            }
            match rest.find(CDATA_END) {
                Some(end) => event.body.push_str(&rest[..end]),
                None => {
                    event.body.push_str(rest);
                    event.body.push('\n');
                    event.in_cdata = true;
                }
            }
            return Vec::new();
        }
        if trimmed.starts_with("</log4j:Event") {
            let event = self.event.take().expect("checked above");
            let is_error = matches!(event.level.as_str(), "ERROR" | "FATAL");
            let prefix = format!("[{}/{}] ", event.thread, event.level);
            return event
                .body
                .lines()
                .map(|body| LogLine { text: format!("{prefix}{body}"), is_error })
                .collect();
        }
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(lines: &[&str]) -> Vec<LogLine> {
        let mut reader = LogReader::default();
        lines.iter().flat_map(|line| reader.feed(line)).collect()
    }

    #[test]
    fn turns_an_event_into_a_readable_line() {
        let found = read(&[
            r#"<log4j:Event logger="Client" timestamp="1" level="INFO" thread="main">"#,
            "  <log4j:Message><![CDATA[Setting user: Steve]]></log4j:Message>",
            "</log4j:Event>",
        ]);
        assert_eq!(found, [LogLine { text: "[main/INFO] Setting user: Steve".into(), is_error: false }]);
    }

    #[test]
    fn keeps_every_line_of_a_multi_line_message() {
        let found = read(&[
            r#"<log4j:Event logger="Fabric" level="INFO" thread="main">"#,
            "<log4j:Message><![CDATA[Loading 2 mods:",
            "  - sodium",
            "  - iris]]></log4j:Message>",
            "</log4j:Event>",
        ]);
        assert_eq!(found.len(), 3);
        assert!(found[2].text.ends_with("- iris"));
    }

    #[test]
    fn flags_errors_and_includes_their_stack_trace() {
        let found = read(&[
            r#"<log4j:Event logger="Client" level="ERROR" thread="Render">"#,
            "<log4j:Message><![CDATA[It broke]]></log4j:Message>",
            "<log4j:Throwable><![CDATA[java.lang.RuntimeException: boom",
            "  at net.minecraft.Foo]]></log4j:Throwable>",
            "</log4j:Event>",
        ]);
        assert!(found.iter().all(|l| l.is_error));
        assert!(found.iter().any(|l| l.text.contains("RuntimeException")));
    }

    #[test]
    fn plain_output_passes_straight_through() {
        assert_eq!(read(&["hello"])[0].text, "hello");
    }
}
