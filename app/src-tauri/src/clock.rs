use chrono::Local;
use sprk_pipeline::host::{Clock, Stamp};

/// Local wall clock (Windows time zone rules via chrono).
pub struct LocalClock;

impl Clock for LocalClock {
    fn now(&self) -> Stamp {
        let n = Local::now();
        Stamp {
            date: n.format("%Y-%m-%d").to_string(),
            hhmm: n.format("%H%M").to_string(),
            iso: n.format("%Y-%m-%dT%H:%M:%S%:z").to_string(),
        }
    }
}
