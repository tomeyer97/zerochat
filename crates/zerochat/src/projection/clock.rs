//! When ledger events happened. Zeroshot 16.1 stamps few events; the rest get the time of their
//! neighbours.

use serde_json::Value;

use crate::source::Event;

/// The time an event carries itself, if any: a log line's `timestamp`, else the `at` zerochat
/// fills in from Zeroshot's `recorded_at_ms`.
pub(super) fn event_time(event: &Event) -> Option<u64> {
    log_time(&event.body).or_else(|| event.body["at"].as_u64())
}

/// A log line's own time (`timestamp`, Unix ms). The run's first and last activity count these
/// only.
pub(super) fn log_time(body: &Value) -> Option<u64> {
    body["timestamp"].as_u64()
}

/// Wall-clock estimates for events that carry no timestamp: the last known time at or before
/// the event (falling back to the next known time).
pub(super) struct Clock {
    seqs: Vec<u64>,
    before: Vec<Option<u64>>,
    after: Vec<Option<u64>>,
}

impl Clock {
    pub(super) fn new(events: &[Event]) -> Self {
        let own: Vec<Option<u64>> = events.iter().map(event_time).collect();
        let mut before = Vec::with_capacity(own.len());
        let mut last = None;
        for t in &own {
            last = t.or(last);
            before.push(last);
        }
        let mut after = vec![None; own.len()];
        let mut next = None;
        for (i, t) in own.iter().enumerate().rev() {
            next = t.or(next);
            after[i] = next;
        }
        Self {
            seqs: events.iter().map(|e| e.seq).collect(),
            before,
            after,
        }
    }

    pub(super) fn at(&self, seq: u64) -> Option<u64> {
        let i = self.index(seq)?;
        self.before[i].or(self.after[i])
    }

    /// The next known time at or after the event.
    pub(super) fn next(&self, seq: u64) -> Option<u64> {
        let i = self.index(seq)?;
        self.after[i].or(self.before[i])
    }

    fn index(&self, seq: u64) -> Option<usize> {
        let i = self.seqs.partition_point(|s| *s < seq);
        (i < self.seqs.len() && self.seqs[i] == seq).then_some(i)
    }
}
