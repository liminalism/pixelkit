//! Per-phase frame timing, printed as percentiles.
//!
//! Enabled by `PIXELKIT_FRAME_LOG=1`. The shell records tick, paint and
//! present durations for every frame and prints p50/p95/max every couple of
//! seconds — the number that decides whether a software renderer is enough is
//! not the mean but the tail.

use std::time::{Duration, Instant};

#[derive(Debug)]
pub struct FrameTimer {
    enabled: bool,
    tick: Vec<Duration>,
    paint: Vec<Duration>,
    present: Vec<Duration>,
    window_start: Instant,
    /// Print interval.
    every: Duration,
    /// Optional budget; p95 above it is flagged.
    budget: Option<Duration>,
}

impl FrameTimer {
    /// Reads `PIXELKIT_FRAME_LOG` (any value enables) and
    /// `PIXELKIT_FRAME_BUDGET_MS` (a number of milliseconds).
    pub fn from_env() -> FrameTimer {
        let enabled = std::env::var_os("PIXELKIT_FRAME_LOG").is_some();
        let budget = std::env::var("PIXELKIT_FRAME_BUDGET_MS")
            .ok()
            .and_then(|v| v.parse::<f64>().ok())
            .map(Duration::from_secs_f64)
            .map(|d| d / 1000);
        FrameTimer::new(enabled, budget)
    }

    pub fn new(enabled: bool, budget: Option<Duration>) -> FrameTimer {
        FrameTimer {
            enabled,
            tick: Vec::new(),
            paint: Vec::new(),
            present: Vec::new(),
            window_start: Instant::now(),
            every: Duration::from_secs(2),
            budget,
        }
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    pub fn record(&mut self, tick: Duration, paint: Duration, present: Duration) {
        if !self.enabled {
            return;
        }
        self.tick.push(tick);
        self.paint.push(paint);
        self.present.push(present);
        if self.window_start.elapsed() >= self.every {
            self.flush();
        }
    }

    /// Print and reset. Also useful at exit.
    pub fn flush(&mut self) {
        if !self.enabled || self.paint.is_empty() {
            return;
        }
        let frames = self.paint.len();
        let secs = self.window_start.elapsed().as_secs_f64().max(1e-9);
        let line = |name: &str, samples: &mut Vec<Duration>| {
            samples.sort();
            let pick = |q: f64| samples[((samples.len() - 1) as f64 * q).round() as usize];
            format!(
                "{name} p50 {:>6.2}ms p95 {:>6.2}ms max {:>6.2}ms",
                pick(0.5).as_secs_f64() * 1e3,
                pick(0.95).as_secs_f64() * 1e3,
                samples[samples.len() - 1].as_secs_f64() * 1e3
            )
        };
        let mut total: Vec<Duration> = self
            .tick
            .iter()
            .zip(&self.paint)
            .zip(&self.present)
            .map(|((t, p), q)| *t + *p + *q)
            .collect();
        let over = self.budget.map(|b| {
            let n = total.iter().filter(|d| **d > b).count();
            format!(" over-budget {n}/{frames}")
        });
        eprintln!(
            "[frames] {frames} in {secs:.1}s ({:.1}/s) | {} | {} | {} | {}{}",
            frames as f64 / secs,
            line("tick", &mut self.tick),
            line("paint", &mut self.paint),
            line("present", &mut self.present),
            line("total", &mut total),
            over.unwrap_or_default()
        );
        self.tick.clear();
        self.paint.clear();
        self.present.clear();
        self.window_start = Instant::now();
    }
}
