//! `sprkctl sources-check [--seconds N]` (Windows): polls audio sessions the way a session does
//! and prints detected activities. For development with synthetic audio only. Titles can be
//! PHI-grade, so only the app name and the title length are printed, never the title.
use anyhow::Result;
use sprk_context::activity::{ActivityEvent, ActivityTracker, Exclusions, Timing};
use sprk_context::sources::{AudioSessionSource, NoTabs, SourcePoller};
use sprk_context::win::{WasapiSessions, Windows};
use sprk_context::WindowEnumerator;
use std::time::{Duration, Instant};

pub fn run(rest: &[&str]) -> Result<()> {
    let seconds: u64 = rest
        .iter()
        .position(|a| *a == "--seconds")
        .and_then(|i| rest.get(i + 1))
        .and_then(|s| s.parse().ok())
        .unwrap_or(10);
    let sessions = WasapiSessions::default();
    let mut poller = SourcePoller::new(ActivityTracker::new(
        Timing::default(),
        Exclusions::default(),
    ));
    let start = Instant::now();
    let mut seen_sessions = 0usize;
    while start.elapsed() < Duration::from_secs(seconds) {
        let t = start.elapsed().as_millis() as u64;
        let list = sessions.sessions();
        seen_sessions = seen_sessions.max(list.iter().filter(|s| !s.ignorable).count());
        let wins = Windows.windows();
        for e in poller.poll(t, &list, &[], &NoTabs, &wins) {
            match e {
                ActivityEvent::Started { app, title, at_ms } => {
                    println!(
                        "started app={app} title_len={} at={}ms",
                        title.chars().count(),
                        at_ms
                    )
                }
                ActivityEvent::Ended {
                    app,
                    title,
                    end_ms,
                    duration_ms,
                } => println!(
                    "ended app={app} title_len={} at={}ms duration={}ms",
                    title.chars().count(),
                    end_ms,
                    duration_ms
                ),
            }
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    for e in poller.pause(start.elapsed().as_millis() as u64) {
        if let ActivityEvent::Ended {
            app, duration_ms, ..
        } = e
        {
            println!("ended (stop) app={app} duration={duration_ms}ms");
        }
    }
    println!("non-ignorable sessions seen: {seen_sessions}");
    Ok(())
}
