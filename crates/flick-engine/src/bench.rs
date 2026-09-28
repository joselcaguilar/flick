//! Engine benchmark harness for `flick-engine bench`.

use std::{collections::BTreeMap, path::PathBuf, time::Instant};

use anyhow::{Context, bail};
use flick_core::SelectionState;
use flick_gestures::{GestureEngineConfig, ReplayRunner, read_jsonl_str};
use serde::Serialize;

/// Runs the benchmark over a landmark JSONL fixture and prints p50/p95 by stage.
pub fn run(fixture: Option<PathBuf>) -> anyhow::Result<()> {
    let fixture = fixture.context("bench requires --fixture <landmark-jsonl>")?;
    if !fixture.exists() {
        bail!("fixture does not exist: {}", fixture.display());
    }
    let content = std::fs::read_to_string(&fixture)
        .with_context(|| format!("failed to read {}", fixture.display()))?;
    let records = read_jsonl_str(&content)?;
    let mut stages: BTreeMap<&'static str, Vec<f64>> = BTreeMap::new();
    for record in &records {
        stages
            .entry("capture")
            .or_default()
            .push(f64::from(record.timings.capture_ms));
        stages
            .entry("palm")
            .or_default()
            .push(f64::from(record.timings.palm_ms));
        stages
            .entry("landmarks")
            .or_default()
            .push(f64::from(record.timings.landmarks_ms));
        stages
            .entry("tracking")
            .or_default()
            .push(f64::from(record.timings.tracking_ms));
        stages
            .entry("embedding")
            .or_default()
            .push(f64::from(record.timings.embedding_ms));
        stages
            .entry("targeting")
            .or_default()
            .push(f64::from(record.timings.targeting_ms));
        stages
            .entry("vision_total")
            .or_default()
            .push(f64::from(record.timings.total_ms));
    }

    let mut runner = ReplayRunner::new(GestureEngineConfig::default());
    let started = Instant::now();
    let outcome = runner.run_records(&records, &SelectionState::Idle);
    let elapsed = started.elapsed().as_secs_f64() * 1_000.0;
    if !records.is_empty() {
        stages
            .entry("recognize_dispatch_stub")
            .or_default()
            .push(elapsed / records.len() as f64);
    }

    let summary = BenchSummary {
        fixture: fixture.display().to_string(),
        frames: records.len(),
        events: outcome.events.len(),
        stages: stages
            .into_iter()
            .map(|(stage, mut values)| StageSummary {
                stage: stage.to_owned(),
                p50_ms: percentile(&mut values, 0.50),
                p95_ms: percentile(&mut values, 0.95),
            })
            .collect(),
    };
    println!("{}", serde_json::to_string_pretty(&summary)?);
    Ok(())
}

#[derive(Debug, Serialize)]
struct BenchSummary {
    fixture: String,
    frames: usize,
    events: usize,
    stages: Vec<StageSummary>,
}

#[derive(Debug, Serialize)]
struct StageSummary {
    stage: String,
    p50_ms: f64,
    p95_ms: f64,
}

fn percentile(values: &mut [f64], q: f64) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    values.sort_by(f64::total_cmp);
    let rank = ((values.len().saturating_sub(1)) as f64 * q).round() as usize;
    values[rank.min(values.len() - 1)]
}
