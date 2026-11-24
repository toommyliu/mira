use std::{
    collections::HashMap,
    fmt::Write as _,
    time::{Duration, Instant},
};

use serde_json::Value as Json;

const REPORT_EVERY: Duration = Duration::from_secs(60);
const SHOWN: usize = 10;

#[derive(Default)]
struct Tally {
    calls: u64,
    bytes: u64,
}

pub(super) struct Usage {
    since: Instant,
    pending: HashMap<u64, String>,
    tallies: HashMap<String, Tally>,
}

impl Usage {
    pub(super) fn new(now: Instant) -> Self {
        Self {
            since: now,
            pending: HashMap::new(),
            tallies: HashMap::new(),
        }
    }

    pub(super) fn asked(&mut self, id: u64, request: &Json) {
        self.pending.insert(id, label(request));
    }

    pub(super) fn answered(&mut self, id: u64, bytes: usize) {
        if let Some(label) = self.pending.remove(&id) {
            let tally = self.tallies.entry(label).or_default();
            tally.calls += 1;
            tally.bytes += bytes as u64;
        }
    }

    pub(super) fn due(&mut self, now: Instant) -> Option<String> {
        if now.duration_since(self.since) < REPORT_EVERY {
            return None;
        }
        self.report(now)
    }

    pub(super) fn report(&mut self, now: Instant) -> Option<String> {
        let elapsed = now.duration_since(self.since);
        self.since = now;
        if self.tallies.is_empty() {
            return None;
        }
        let mut tallies: Vec<_> = self.tallies.drain().collect();
        tallies.sort_unstable_by(|(a_label, a), (b_label, b)| {
            (b.bytes, b.calls, a_label).cmp(&(a.bytes, a.calls, b_label))
        });
        let calls: u64 = tallies.iter().map(|(_, tally)| tally.calls).sum();
        let bytes: u64 = tallies.iter().map(|(_, tally)| tally.bytes).sum();
        let mut report = format!(
            "Skua calls over {}s: {calls} calls, {} replied.",
            elapsed.as_secs(),
            size(bytes)
        );
        for (label, tally) in tallies.iter().take(SHOWN) {
            let _ = write!(report, " {label} ×{}, {};", tally.calls, size(tally.bytes));
        }
        if tallies.len() > SHOWN {
            let _ = write!(report, " {} more.", tallies.len() - SHOWN);
        }
        Some(report.trim_end_matches(';').to_owned())
    }
}

fn label(request: &Json) -> String {
    let field = |name| request.get(name).and_then(Json::as_str);
    match (field("domain"), field("op")) {
        (Some("skua"), Some("call")) => {
            let function = field("function").unwrap_or("?");
            match request
                .get("args")
                .and_then(|args| args.get(0))
                .and_then(Json::as_str)
            {
                Some(path) => format!("{function}({path})"),
                None => function.to_owned(),
            }
        }
        (Some(domain), Some(op)) => format!("{domain}.{op}"),
        (domain, _) => domain.unwrap_or("?").to_owned(),
    }
}

fn size(bytes: u64) -> String {
    match bytes {
        0..1_000 => format!("{bytes} B"),
        1_000..1_000_000 => format!("{:.1} KB", bytes as f64 / 1e3),
        _ => format!("{:.1} MB", bytes as f64 / 1e6),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn a_report_ranks_calls_by_bytes_replied_and_waits_for_its_interval() {
        let start = Instant::now();
        let mut usage = Usage::new(start);
        let calls = [
            (
                json!({ "domain": "skua", "op": "call", "function": "flash.getGameObject", "args": ["world.myAvatar.items"] }),
                40_000,
            ),
            (
                json!({ "domain": "skua", "op": "call", "function": "flash.getGameObject", "args": ["world.myAvatar.items"] }),
                40_000,
            ),
            (
                json!({ "domain": "combat", "op": "useSkill", "skill": 1 }),
                60,
            ),
            (
                json!({ "domain": "skua", "op": "call", "function": "skua.availableMonsters" }),
                900,
            ),
        ];
        for (id, (request, _)) in calls.iter().enumerate() {
            usage.asked(id as u64, request);
        }
        for (id, (_, bytes)) in calls.iter().enumerate() {
            usage.answered(id as u64, *bytes);
        }
        usage.answered(99, 1_000_000);

        assert_eq!(usage.due(start + Duration::from_secs(59)), None);
        assert_eq!(
            usage.due(start + Duration::from_secs(60)).as_deref(),
            Some(
                "Skua calls over 60s: 4 calls, 81.0 KB replied. \
                 flash.getGameObject(world.myAvatar.items) ×2, 80.0 KB; \
                 skua.availableMonsters ×1, 900 B; \
                 combat.useSkill ×1, 60 B"
            ),
        );
        assert_eq!(usage.report(start + Duration::from_secs(61)), None);
    }
}
