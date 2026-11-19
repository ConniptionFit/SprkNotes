//! Calendar seam. Local only: no Graph, no network.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Meeting {
    pub id: String,
    pub start_epoch: i64,
    pub end_epoch: i64,
}

pub trait CalendarProvider: Send {
    /// Meetings overlapping `now_epoch`. Empty when the provider is unavailable.
    fn current_meetings(&self, now_epoch: i64) -> Vec<Meeting>;
}

pub struct FakeCalendar(pub Vec<Meeting>);
impl CalendarProvider for FakeCalendar {
    fn current_meetings(&self, now: i64) -> Vec<Meeting> {
        self.0
            .iter()
            .filter(|m| m.start_epoch <= now && now < m.end_epoch)
            .cloned()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn filters_to_current() {
        let c = FakeCalendar(vec![Meeting {
            id: "a".into(),
            start_epoch: 0,
            end_epoch: 10,
        }]);
        assert_eq!(c.current_meetings(5).len(), 1);
        assert!(c.current_meetings(10).is_empty());
    }
}
