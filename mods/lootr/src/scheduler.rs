//! In-memory index of loaded sites only; the shard is authoritative.
use std::collections::{BTreeSet, HashMap};

pub(crate) type Position = [i32; 3];
pub(crate) const BUDGET: usize = 16;

#[derive(Default)]
pub(crate) struct Queue {
    due: BTreeSet<(i64, Position)>,
    deadlines: HashMap<Position, i64>,
    epochs: HashMap<Position, i64>,
    next_epoch: i64,
}

impl Queue {
    pub fn schedule(&mut self, pos: Position, deadline: Option<i64>) {
        if let Some(old) = self.deadlines.remove(&pos) {
            self.due.remove(&(old, pos));
        }
        if let Some(at) = deadline {
            self.deadlines.insert(pos, at);
            self.due.insert((at, pos));
        }
    }

    pub fn unload(&mut self, pos: Position) {
        self.schedule(pos, None);
        self.invalidate(pos);
    }

    pub fn ready(&mut self, now: i64) -> Vec<Position> {
        let mut ready = Vec::with_capacity(BUDGET);
        while ready.len() < BUDGET {
            let Some(&(at, pos)) = self.due.first() else {
                break;
            };
            if at > now {
                break;
            }
            self.schedule(pos, None);
            ready.push(pos);
        }
        ready
    }

    /// Never reuse a token after refresh, unload or replacement.
    pub fn epoch(&mut self, pos: Position) -> Option<i64> {
        if let Some(epoch) = self.epochs.get(&pos) {
            return Some(*epoch);
        }
        self.next_epoch = self.next_epoch.checked_add(1)?;
        self.epochs.insert(pos, self.next_epoch);
        Some(self.next_epoch)
    }

    pub fn valid(&self, pos: Position, epoch: i64) -> bool {
        self.epochs.get(&pos) == Some(&epoch)
    }

    pub fn invalidate(&mut self, pos: Position) {
        self.epochs.remove(&pos);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queue_deduplicates_bounds_work_and_never_reuses_epoch() {
        let mut q = Queue::default();
        for x in 0..40 {
            q.schedule([x, 0, 0], Some(10));
            q.schedule([x, 0, 0], Some(20));
        }
        assert!(q.ready(19).is_empty());
        assert_eq!(q.ready(20).len(), BUDGET);
        assert_eq!(q.ready(20).len(), BUDGET);
        assert_eq!(q.ready(20).len(), 8);
        assert!(q.ready(20).is_empty());
        let pos = [0, 0, 0];
        let epoch = q.epoch(pos).unwrap();
        assert_eq!(q.epoch(pos), Some(epoch));
        q.schedule(pos, Some(30));
        q.unload(pos);
        assert!(!q.valid(pos, epoch));
        assert!(q.ready(30).is_empty());
        let new = q.epoch(pos).unwrap();
        assert!(new > epoch);
        q.invalidate(pos);
        assert!(!q.valid(pos, new));
    }
}
