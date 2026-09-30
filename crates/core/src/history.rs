//! Undo/redo of whole snapshots. The EDL is small (a few hundred cuts at most), so keeping
//! copies is simpler and safer than inverting every kind of edit.

use serde::Serialize;

/// Snapshots kept before the oldest ones are forgotten.
pub const DEFAULT_LIMIT: usize = 200;

struct Entry<T> {
    state: T,
    label: String,
}

pub struct History<T> {
    undo: Vec<Entry<T>>,
    redo: Vec<Entry<T>>,
    limit: usize,
}

/// What can be undone or redone next, for menus and for agents.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct HistoryStatus {
    pub undo: Option<String>,
    pub redo: Option<String>,
}

impl<T> Default for History<T> {
    fn default() -> Self {
        Self::new(DEFAULT_LIMIT)
    }
}

impl<T> History<T> {
    pub fn new(limit: usize) -> Self {
        Self {
            undo: Vec::new(),
            redo: Vec::new(),
            limit: limit.max(1),
        }
    }

    /// Records that `before` was replaced by the action `label`. A new action makes the
    /// undone ones unreachable, as in every editor.
    pub fn record(&mut self, before: T, label: impl Into<String>) {
        self.redo.clear();
        self.undo.push(Entry {
            state: before,
            label: label.into(),
        });
        if self.undo.len() > self.limit {
            self.undo.remove(0);
        }
    }

    /// Puts back the state before the last action. Returns that action's label.
    pub fn undo(&mut self, current: &mut T) -> Option<String> {
        let entry = self.undo.pop()?;
        let label = entry.label.clone();
        let after = std::mem::replace(current, entry.state);
        self.redo.push(Entry {
            state: after,
            label: entry.label,
        });
        Some(label)
    }

    /// Applies again the last undone action. Returns its label.
    pub fn redo(&mut self, current: &mut T) -> Option<String> {
        let entry = self.redo.pop()?;
        let label = entry.label.clone();
        let before = std::mem::replace(current, entry.state);
        self.undo.push(Entry {
            state: before,
            label: entry.label,
        });
        Some(label)
    }

    pub fn status(&self) -> HistoryStatus {
        HistoryStatus {
            undo: self.undo.last().map(|e| e.label.clone()),
            redo: self.redo.last().map(|e| e.label.clone()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn undo_then_redo_walks_back_and_forth() {
        let mut history = History::default();
        let mut value = 1;
        history.record(value, "set 2");
        value = 2;
        history.record(value, "set 3");
        value = 3;

        assert_eq!(history.undo(&mut value).as_deref(), Some("set 3"));
        assert_eq!(value, 2);
        assert_eq!(history.undo(&mut value).as_deref(), Some("set 2"));
        assert_eq!(value, 1);
        assert_eq!(history.undo(&mut value), None);
        assert_eq!(value, 1);

        assert_eq!(history.redo(&mut value).as_deref(), Some("set 2"));
        assert_eq!(value, 2);
        assert_eq!(history.redo(&mut value).as_deref(), Some("set 3"));
        assert_eq!(value, 3);
        assert_eq!(history.redo(&mut value), None);
    }

    #[test]
    fn a_new_action_drops_what_was_undone() {
        let mut history = History::default();
        let mut value = 1;
        history.record(value, "a");
        value = 2;
        history.undo(&mut value);
        history.record(value, "b");
        assert_eq!(
            history.status(),
            HistoryStatus {
                undo: Some("b".into()),
                redo: None
            }
        );
    }

    #[test]
    fn the_oldest_snapshots_are_forgotten_past_the_limit() {
        let mut history = History::new(2);
        let mut value = 0;
        for next in 1..=3 {
            history.record(value, format!("{next}"));
            value = next;
        }
        assert!(history.undo(&mut value).is_some());
        assert!(history.undo(&mut value).is_some());
        assert_eq!(history.undo(&mut value), None);
        assert_eq!(value, 1);
    }

    #[test]
    fn status_names_the_next_steps() {
        let mut history = History::default();
        assert_eq!(history.status(), HistoryStatus::default());
        let mut value = "x";
        history.record(value, "Delete clip 2");
        value = "y";
        history.undo(&mut value);
        assert_eq!(history.status().redo.as_deref(), Some("Delete clip 2"));
        assert_eq!(history.status().undo, None);
    }
}
