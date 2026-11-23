//! Benchmark for the assistant (plan phase 0): load time, prompt speed, time to first text,
//! generation speed and peak memory, on a synthetic meeting. Prints numbers only, never text.
//!
//!   sprk-assistant-bench <model folder> [--threads N] [--runs N] [--background-load N]
//!
//! `--background-load N` keeps N threads busy to imitate transcription taking cores. Numbers from
//! the dev host or a VM are informational; only the real laptops count (SPEC 13.3).
mod engine;

use sprk_assistant::engine::{AssistantEngine, GenRequest, SharedControl};
use sprk_assistant::host::LoadParams;
use sprk_assistant::prompt::{data_block, PromptFormat};
use sprk_assistant::view::{render, Keep, Line};
use sprk_models::Manifest;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

const SENTENCES: [&str; 12] = [
    "We need to review the quarterly budget before Friday.",
    "Finance wants the vendor invoices reconciled first.",
    "I will send the reconciled invoices tomorrow morning.",
    "Does the budget include the new laptop purchase order?",
    "Yes, the laptop purchase order is in the second tab.",
    "The migration to the new ticketing system starts on the fifteenth.",
    "Can someone confirm the maintenance window with the vendor?",
    "I can do that, but I need the contract number from procurement.",
    "Let's move the training session to next Thursday afternoon.",
    "The security review found two items we should fix this month.",
    "Please add the backup test to the checklist before the audit.",
    "We agreed to revisit the staffing plan at the next meeting.",
];

fn meeting(minutes: u64) -> Vec<Line> {
    // About 6 lines a minute, alternating speakers.
    (0..minutes * 6)
        .map(|i| Line {
            id: i + 1,
            t_ms: i * 10_000,
            speaker: if i % 3 == 0 { "Me" } else { "Others" }.into(),
            text: SENTENCES[(i as usize * 7) % SENTENCES.len()].into(),
        })
        .collect()
}

#[cfg(target_os = "linux")]
fn peak_rss_mb() -> Option<u64> {
    let s = std::fs::read_to_string("/proc/self/status").ok()?;
    let kb: u64 = s
        .lines()
        .find(|l| l.starts_with("VmHWM:"))?
        .split_whitespace()
        .nth(1)?
        .parse()
        .ok()?;
    Some(kb / 1024)
}

#[cfg(windows)]
fn peak_rss_mb() -> Option<u64> {
    use windows_sys::Win32::System::ProcessStatus::{
        K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS,
    };
    use windows_sys::Win32::System::Threading::GetCurrentProcess;
    let mut c = PROCESS_MEMORY_COUNTERS {
        cb: std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
        PageFaultCount: 0,
        PeakWorkingSetSize: 0,
        WorkingSetSize: 0,
        QuotaPeakPagedPoolUsage: 0,
        QuotaPagedPoolUsage: 0,
        QuotaPeakNonPagedPoolUsage: 0,
        QuotaNonPagedPoolUsage: 0,
        PagefileUsage: 0,
        PeakPagefileUsage: 0,
    };
    // SAFETY: `c` is a properly sized, writable PROCESS_MEMORY_COUNTERS with `cb` set, and
    // GetCurrentProcess returns a pseudo handle that needs no closing.
    let ok = unsafe { K32GetProcessMemoryInfo(GetCurrentProcess(), &mut c, c.cb) };
    (ok != 0).then(|| (c.PeakWorkingSetSize / (1024 * 1024)) as u64)
}

#[cfg(not(any(target_os = "linux", windows)))]
fn peak_rss_mb() -> Option<u64> {
    None
}

struct Args {
    dir: String,
    threads: u32,
    runs: u32,
    background: u32,
}

fn parse() -> Option<Args> {
    let mut it = std::env::args().skip(1);
    let dir = it.next()?;
    let mut a = Args {
        dir,
        threads: 4,
        runs: 3,
        background: 0,
    };
    while let Some(flag) = it.next() {
        let v: u32 = it.next()?.parse().ok()?;
        match flag.as_str() {
            "--threads" => a.threads = v.max(1),
            "--runs" => a.runs = v.max(1),
            "--background-load" => a.background = v,
            _ => return None,
        }
    }
    Some(a)
}

fn main() {
    let Some(a) = parse() else {
        eprintln!("usage: sprk-assistant-bench <model folder> [--threads N] [--runs N] [--background-load N]");
        std::process::exit(2);
    };
    let Ok(json) = std::fs::read_to_string(std::path::Path::new(&a.dir).join("model.json")) else {
        eprintln!("no model.json in {}", a.dir);
        std::process::exit(2);
    };
    let Ok(manifest) = Manifest::from_json(&json) else {
        eprintln!("model.json is not a valid assistant pack");
        std::process::exit(2);
    };
    let Some(spec) = manifest.assistant.clone() else {
        eprintln!("not an assistant pack");
        std::process::exit(2);
    };
    let Some(format) = PromptFormat::from_id(&spec.prompt_format) else {
        eprintln!("unknown prompt format {}", spec.prompt_format);
        std::process::exit(2);
    };

    let stop = Arc::new(AtomicBool::new(false));
    let burners: Vec<_> = (0..a.background)
        .map(|_| {
            let stop = Arc::clone(&stop);
            std::thread::spawn(move || {
                let mut x = 1.0f64;
                while !stop.load(Ordering::Relaxed) {
                    for _ in 0..100_000 {
                        x = (x * 1.000_001).sin().abs() + 1.0;
                    }
                    std::hint::black_box(x);
                }
            })
        })
        .collect();

    let params = LoadParams {
        model_dir: a.dir.clone(),
        threads: a.threads,
        context_tokens: spec.context_tokens,
    };
    let Ok(mut eng) = engine::LlamaEngine::new(&params) else {
        eprintln!("could not read the pack");
        std::process::exit(1);
    };
    let t = Instant::now();
    if eng.load().is_err() {
        eprintln!("model failed to load");
        std::process::exit(1);
    }
    let load_ms = t.elapsed().as_millis();
    println!(
        "model={} tier={} threads={} background_load={} load_ms={} rss_after_load_mb={}",
        manifest.id,
        spec.tier,
        a.threads,
        a.background,
        load_ms,
        peak_rss_mb().map_or("n/a".into(), |m| m.to_string())
    );

    // Window sizes in minutes of meeting the question covers.
    let lines = meeting(60);
    let control = SharedControl::default();
    for (name, minutes) in [("title-3min", 3u64), ("recap-5min", 5), ("recap-15min", 15)] {
        let from = lines
            .last()
            .map_or(0, |l| l.t_ms)
            .saturating_sub(minutes * 60_000);
        let budget = (spec.context_tokens as usize).saturating_sub(300);
        let view = render(&lines, &[], (from, u64::MAX), budget, Keep::Tail);
        let user = format!(
            "{}\n\nSummarize what was said in three short bullets.",
            data_block("transcript", &view.text)
        );
        let req = GenRequest {
            prompt: format.build("You summarize meetings from a transcript.", &user),
            max_tokens: 120,
            temperature: 0.0,
            stop: format.stop(),
        };
        for run in 1..=a.runs {
            let t = Instant::now();
            let mut first: Option<u128> = None;
            let r = eng.generate(&req, &control, &mut |_| {
                first.get_or_insert(t.elapsed().as_millis());
            });
            let total = t.elapsed().as_millis();
            match r {
                Ok(s) => {
                    let ttft = first.unwrap_or(total);
                    let gen_ms = total.saturating_sub(ttft).max(1);
                    println!(
                        "{name} run={run} prompt_tokens={} out_tokens={} ttft_ms={ttft} total_ms={total} prompt_tok_s={:.0} gen_tok_s={:.1}",
                        s.prompt_tokens,
                        s.output_tokens,
                        f64::from(s.prompt_tokens) * 1000.0 / ttft.max(1) as f64,
                        f64::from(s.output_tokens.saturating_sub(1)) * 1000.0 / gen_ms as f64,
                    );
                }
                Err(e) => println!("{name} run={run} error={e}"),
            }
        }
    }
    println!(
        "peak_rss_mb={}",
        peak_rss_mb().map_or("n/a".into(), |m| m.to_string())
    );
    stop.store(true, Ordering::Relaxed);
    for b in burners {
        let _ = b.join();
    }
}
