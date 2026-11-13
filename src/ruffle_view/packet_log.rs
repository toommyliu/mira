use std::{collections::VecDeque, time::SystemTime};

use gpui::SharedString;

use crate::{automation::packets::Captured, bridge::packets::Sender};

const MAX_LINES: usize = 5_000;
const MAX_BYTES: usize = 8 * 1024 * 1024;
const PREVIEW: usize = 300;

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Line {
    Packet(Packet),
    Lost { at: SystemTime },
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Packet {
    pub(crate) at: SystemTime,
    pub(crate) from: Sender,
    pub(crate) text: SharedString,
    pub(crate) preview: SharedString,
}

impl Line {
    fn bytes(&self) -> usize {
        match self {
            Line::Packet(packet) if packet.preview == packet.text => packet.text.len(),
            Line::Packet(packet) => packet.text.len() + packet.preview.len(),
            Line::Lost { .. } => 0,
        }
    }

    fn from(&self) -> Option<Sender> {
        match self {
            Line::Packet(packet) => Some(packet.from),
            Line::Lost { .. } => None,
        }
    }
}

#[derive(Default)]
pub(crate) struct PacketLog {
    lines: VecDeque<Line>,
    bytes: usize,
    dropped: u64,
    client: usize,
    server: usize,
}

impl PacketLog {
    pub(crate) fn extend(&mut self, batch: Vec<Captured>) {
        for captured in batch {
            let line = match captured {
                Captured::Packet { at, from, text } => {
                    let preview = preview(&text);
                    let text = SharedString::from(text);
                    Line::Packet(Packet {
                        at,
                        from,
                        preview: preview.unwrap_or_else(|| text.clone()),
                        text,
                    })
                }
                Captured::Lost { at } => Line::Lost { at },
            };
            self.bytes += line.bytes();
            if let Some(count) = self.count_of(&line) {
                *count += 1;
            }
            self.lines.push_back(line);
        }
        while self.lines.len() > 1 && (self.lines.len() > MAX_LINES || self.bytes > MAX_BYTES) {
            if let Some(line) = self.lines.pop_front() {
                self.bytes -= line.bytes();
                if let Some(count) = self.count_of(&line) {
                    *count -= 1;
                }
                self.dropped += 1;
            }
        }
    }

    pub(crate) fn clear(&mut self) {
        self.dropped += self.lines.len() as u64;
        self.lines.clear();
        self.bytes = 0;
        self.client = 0;
        self.server = 0;
    }

    pub(crate) fn len(&self) -> usize {
        self.lines.len()
    }

    pub(crate) fn get(&self, index: usize) -> Option<&Line> {
        self.lines.get(index)
    }

    #[cfg(test)]
    fn lines(&self) -> impl Iterator<Item = &Line> {
        self.lines.iter()
    }

    pub(crate) fn dropped(&self) -> u64 {
        self.dropped
    }

    pub(crate) fn count(&self, from: Sender) -> usize {
        match from {
            Sender::Client => self.client,
            Sender::Server => self.server,
        }
    }

    fn count_of(&mut self, line: &Line) -> Option<&mut usize> {
        Some(match line.from()? {
            Sender::Client => &mut self.client,
            Sender::Server => &mut self.server,
        })
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Filter {
    pub(crate) from: Option<Sender>,
    pub(crate) query: String,
}

impl Filter {
    fn shows(&self, line: &Line) -> bool {
        let query = self.query.trim().as_bytes();
        match line {
            Line::Lost { .. } => query.is_empty(),
            Line::Packet(packet) => {
                self.from.is_none_or(|from| from == packet.from)
                    && (query.is_empty()
                        || packet
                            .text
                            .as_bytes()
                            .windows(query.len())
                            .any(|window| window.eq_ignore_ascii_case(query)))
            }
        }
    }
}

#[derive(Default)]
pub(crate) struct Visible {
    filter: Filter,
    ids: VecDeque<u64>,
    scanned: u64,
    dropped: u64,
}

impl Visible {
    pub(crate) fn refresh(&mut self, log: &PacketLog, filter: &Filter) {
        if *filter != self.filter {
            self.filter = filter.clone();
            self.ids.clear();
            self.scanned = 0;
        }
        let first = log.dropped();
        while self.ids.front().is_some_and(|id| *id < first) {
            self.ids.pop_front();
            self.dropped += 1;
        }
        let end = first + log.len() as u64;
        for id in self.scanned.max(first)..end {
            if log
                .get((id - first) as usize)
                .is_some_and(|line| self.filter.shows(line))
            {
                self.ids.push_back(id);
            }
        }
        self.scanned = end;
    }

    pub(crate) fn len(&self) -> usize {
        self.ids.len()
    }

    pub(crate) fn id(&self, index: usize) -> Option<u64> {
        self.ids.get(index).copied()
    }

    pub(crate) fn ids(&self) -> impl Iterator<Item = u64> + '_ {
        self.ids.iter().copied()
    }

    pub(crate) fn index_of(&self, id: u64) -> Option<usize> {
        self.ids.binary_search(&id).ok()
    }

    pub(crate) fn dropped(&self) -> u64 {
        self.dropped
    }
}

impl PacketLog {
    pub(crate) fn line(&self, id: u64) -> Option<&Line> {
        let index = id.checked_sub(self.dropped)?;
        self.lines.get(usize::try_from(index).ok()?)
    }
}

fn preview(text: &str) -> Option<SharedString> {
    let cut = text
        .char_indices()
        .nth(PREVIEW)
        .map_or(text.len(), |(at, _)| at);
    let multiline = text[..cut].contains(['\n', '\r', '\t']);
    if cut == text.len() && !multiline {
        return None;
    }
    let mut preview: String = text[..cut]
        .chars()
        .map(|c| if c.is_whitespace() { ' ' } else { c })
        .collect();
    if cut < text.len() {
        preview.push('…');
    }
    Some(preview.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn packet(from: Sender, text: impl Into<String>) -> Captured {
        Captured::Packet {
            at: SystemTime::UNIX_EPOCH,
            from,
            text: text.into(),
        }
    }

    fn texts(log: &PacketLog) -> Vec<SharedString> {
        log.lines()
            .filter_map(|line| match line {
                Line::Packet(packet) => Some(packet.text.clone()),
                Line::Lost { .. } => None,
            })
            .collect()
    }

    #[test]
    fn the_log_keeps_the_newest_lines_within_its_line_limit() {
        let mut log = PacketLog::default();
        log.extend(
            (0..MAX_LINES + 3)
                .map(|n| packet(Sender::Client, format!("%xt%zm%n%1%{n}%")))
                .collect(),
        );
        log.extend(vec![packet(Sender::Server, "%xt%last%-1%")]);
        assert_eq!(log.len(), MAX_LINES);
        assert_eq!(log.dropped(), 4);
        assert_eq!(texts(&log)[0], "%xt%zm%n%1%4%");
        assert_eq!(texts(&log)[MAX_LINES - 1], "%xt%last%-1%");
        assert_eq!(
            (log.count(Sender::Client), log.count(Sender::Server)),
            (MAX_LINES - 1, 1)
        );
    }

    #[test]
    fn large_packets_drop_older_lines_once_the_log_holds_too_much_text() {
        let mut log = PacketLog::default();
        let big = |n: usize| packet(Sender::Server, format!("{n}{}", "x".repeat(MAX_BYTES / 3)));
        log.extend(vec![big(1), big(2), big(3)]);
        assert_eq!(log.len(), 2);
        log.extend(vec![packet(Sender::Server, "x".repeat(MAX_BYTES * 2))]);
        assert_eq!(log.len(), 1);
        assert_eq!(log.dropped(), 3);
    }

    #[test]
    fn previews_are_one_short_line_and_short_packets_are_their_own_preview() {
        let mut log = PacketLog::default();
        log.extend(vec![
            packet(Sender::Client, "%xt%zm%afk%1%true%"),
            packet(Sender::Server, "<msg>\n<body/>\n</msg>"),
            packet(Sender::Server, "é".repeat(PREVIEW + 1)),
        ]);
        let previews: Vec<_> = log
            .lines()
            .filter_map(|line| match line {
                Line::Packet(packet) => Some(packet.preview.to_string()),
                Line::Lost { .. } => None,
            })
            .collect();
        assert_eq!(previews[0], "%xt%zm%afk%1%true%");
        assert_eq!(previews[1], "<msg> <body/> </msg>");
        assert_eq!(previews[2], format!("{}…", "é".repeat(PREVIEW)));
    }

    #[test]
    fn clearing_keeps_counting_dropped_lines() {
        let mut log = PacketLog::default();
        log.extend(vec![
            packet(Sender::Client, "a"),
            Captured::Lost {
                at: SystemTime::UNIX_EPOCH,
            },
        ]);
        log.clear();
        assert_eq!(
            (log.len(), log.dropped(), log.count(Sender::Client)),
            (0, 2, 0)
        );
    }

    #[test]
    fn the_filtered_view_follows_a_growing_log_and_its_dropped_lines() {
        let mut log = PacketLog::default();
        let mut visible = Visible::default();
        let filter = Filter {
            from: Some(Sender::Server),
            query: "AFK".into(),
        };
        log.extend(vec![
            packet(Sender::Server, "%xt%afk%-1%"),
            packet(Sender::Client, "%xt%zm%afk%1%"),
            packet(Sender::Server, "%xt%ct%-1%"),
        ]);
        visible.refresh(&log, &filter);
        assert_eq!(visible.ids().collect::<Vec<_>>(), [0]);

        log.extend(
            (0..MAX_LINES)
                .map(|n| {
                    packet(
                        Sender::Server,
                        if n % 2 == 0 {
                            "%xt%afk%-1%"
                        } else {
                            "%xt%ct%-1%"
                        },
                    )
                })
                .collect(),
        );
        visible.refresh(&log, &filter);
        assert_eq!(visible.len(), MAX_LINES / 2);
        assert_eq!(visible.dropped(), 1);
        assert!(visible.ids().all(|id| matches!(
            log.line(id),
            Some(Line::Packet(packet)) if packet.text == "%xt%afk%-1%"
        )));

        visible.refresh(&log, &Filter::default());
        assert_eq!(visible.len(), MAX_LINES);
        assert_eq!(visible.index_of(log.dropped() + 1), Some(1));
    }
}
