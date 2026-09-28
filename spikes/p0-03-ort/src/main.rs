use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use anyhow::{Context, Result, anyhow};
use clap::Parser;
use ort::{
    ep::{self, coreml::ModelFormat},
    session::{Session, SessionInputValue, builder::GraphOptimizationLevel},
    value::TensorRef,
};

#[derive(Parser, Debug)]
struct Args {
    #[arg(long, default_value = "../../models/cache")]
    models_dir: PathBuf,
    #[arg(short = 'n', long, default_value_t = 200)]
    iterations: usize,
    #[arg(long, default_value_t = 20)]
    warmup: usize,
    #[arg(long, default_value = "../../models/cache/coreml/p0-03")]
    coreml_cache_dir: PathBuf,
}

#[derive(Clone, Copy)]
struct InputSpec {
    name: &'static str,
    shape: &'static [usize],
}

#[derive(Clone, Copy)]
struct ModelSpec {
    id: &'static str,
    file: &'static str,
    inputs: &'static [InputSpec],
}

#[derive(Clone, Copy)]
enum Backend {
    Cpu,
    CoreMl(ModelFormat),
}

impl Backend {
    fn label(self) -> &'static str {
        match self {
            Self::Cpu => "CPU",
            Self::CoreMl(ModelFormat::MLProgram) => "CoreML-MLProgram",
            Self::CoreMl(ModelFormat::NeuralNetwork) => "CoreML-NeuralNetwork",
        }
    }
}

struct PreparedInput {
    name: &'static str,
    shape: Vec<i64>,
    data: Vec<f32>,
}

struct BenchResult {
    model: &'static str,
    backend: &'static str,
    load_ms: f64,
    p50_ms: f64,
    p95_ms: f64,
    cache_entries: Option<usize>,
}

const MODELS: &[ModelSpec] = &[
    ModelSpec {
        id: "palm_detection_full",
        file: "palm_detection_full.onnx",
        inputs: &[InputSpec {
            name: "input_1",
            shape: &[1, 192, 192, 3],
        }],
    },
    ModelSpec {
        id: "hand_landmark_full",
        file: "hand_landmark_full.onnx",
        inputs: &[InputSpec {
            name: "input_1",
            shape: &[1, 224, 224, 3],
        }],
    },
    ModelSpec {
        id: "face_detection_short",
        file: "face_detection_short.onnx",
        inputs: &[InputSpec {
            name: "input",
            shape: &[1, 128, 128, 3],
        }],
    },
    ModelSpec {
        id: "scene_embedder",
        file: "scene_embedder_dinov2_small_uint8.onnx",
        inputs: &[InputSpec {
            name: "pixel_values",
            shape: &[1, 3, 224, 224],
        }],
    },
    ModelSpec {
        id: "canned_gesture_classifier_qaihub_candidate",
        file: "canned_gesture_classifier_qaihub.onnx",
        inputs: &[
            InputSpec {
                name: "hand",
                shape: &[1, 64],
            },
            InputSpec {
                name: "mirrored_hand",
                shape: &[1, 64],
            },
        ],
    },
];

fn main() -> Result<()> {
    let args = Args::parse();
    let _ = ort::init().commit();

    println!("| model | backend | load_ms | p50_ms | p95_ms | coreml_cache_entries |");
    println!("|---|---:|---:|---:|---:|---:|");

    for model in MODELS {
        let model_path = args.models_dir.join(model.file);
        if !model_path.exists() {
            eprintln!("skip {}: missing {}", model.id, model_path.display());
            continue;
        }

        let prepared = prepare_inputs(model);
        for backend in [
            Backend::Cpu,
            Backend::CoreMl(ModelFormat::MLProgram),
            Backend::CoreMl(ModelFormat::NeuralNetwork),
        ] {
            match bench_one(model, &model_path, &prepared, backend, &args) {
                Ok(result) => {
                    println!(
                        "| {} | {} | {:.3} | {:.3} | {:.3} | {} |",
                        result.model,
                        result.backend,
                        result.load_ms,
                        result.p50_ms,
                        result.p95_ms,
                        result
                            .cache_entries
                            .map(|v| v.to_string())
                            .unwrap_or_else(|| "-".to_string())
                    );
                }
                Err(err) => {
                    println!(
                        "| {} | {} | error | error | error | {} |",
                        model.id,
                        backend.label(),
                        format!("{err:#}").replace('|', "\\|")
                    );
                }
            }
        }
    }

    Ok(())
}

fn prepare_inputs(model: &ModelSpec) -> Vec<PreparedInput> {
    model
        .inputs
        .iter()
        .map(|input| {
            let len = input.shape.iter().product();
            PreparedInput {
                name: input.name,
                shape: input.shape.iter().map(|d| *d as i64).collect(),
                data: vec![0.0; len],
            }
        })
        .collect()
}

fn bench_one(
    model: &ModelSpec,
    model_path: &Path,
    prepared: &[PreparedInput],
    backend: Backend,
    args: &Args,
) -> Result<BenchResult> {
    let builder = Session::builder().map_err(|err| anyhow!("{err}"))?;
    let builder = builder
        .with_optimization_level(GraphOptimizationLevel::Level3)
        .map_err(|err| anyhow!("{err}"))?;
    let builder = builder
        .with_intra_threads(2)
        .map_err(|err| anyhow!("{err}"))?;
    let mut builder = builder
        .with_inter_threads(1)
        .map_err(|err| anyhow!("{err}"))?;

    let cache_dir = match backend {
        Backend::Cpu => None,
        Backend::CoreMl(format) => {
            let dir = args.coreml_cache_dir.join(model.id).join(match format {
                ModelFormat::MLProgram => "mlprogram",
                ModelFormat::NeuralNetwork => "neuralnetwork",
            });
            std::fs::create_dir_all(&dir)?;
            builder = builder
                .with_execution_providers([ep::CoreML::default()
                    .with_model_format(format)
                    .with_model_cache_dir(dir.to_string_lossy())
                    .build()])
                .map_err(|err| anyhow!("{err}"))?;
            Some(dir)
        }
    };

    let load_start = Instant::now();
    let mut session = builder
        .commit_from_file(model_path)
        .map_err(|err| anyhow!("{err}"))
        .with_context(|| format!("loading {}", model_path.display()))?;
    let load_ms = elapsed_ms(load_start.elapsed());

    validate_inputs(model, &session)?;

    for _ in 0..args.warmup {
        run_once(&mut session, prepared)?;
    }

    let mut times = Vec::with_capacity(args.iterations);
    for _ in 0..args.iterations {
        let start = Instant::now();
        run_once(&mut session, prepared)?;
        times.push(start.elapsed());
    }
    times.sort_unstable();

    let p50 = percentile(&times, 0.50)?;
    let p95 = percentile(&times, 0.95)?;
    let cache_entries = cache_dir.as_deref().map(count_files).transpose()?;

    Ok(BenchResult {
        model: model.id,
        backend: backend.label(),
        load_ms,
        p50_ms: elapsed_ms(p50),
        p95_ms: elapsed_ms(p95),
        cache_entries,
    })
}

fn validate_inputs(model: &ModelSpec, session: &Session) -> Result<()> {
    for input in model.inputs {
        if !session.inputs().iter().any(|i| i.name() == input.name) {
            let names = session
                .inputs()
                .iter()
                .map(|i| i.name())
                .collect::<Vec<_>>()
                .join(", ");
            return Err(anyhow!(
                "{} expected input {}, session inputs were [{}]",
                model.id,
                input.name,
                names
            ));
        }
    }
    Ok(())
}

fn run_once(session: &mut Session, prepared: &[PreparedInput]) -> Result<()> {
    let mut values: Vec<(String, SessionInputValue<'_>)> = Vec::with_capacity(prepared.len());
    for input in prepared {
        let tensor = TensorRef::from_array_view((input.shape.clone(), input.data.as_slice()))?;
        values.push((input.name.to_string(), tensor.into()));
    }
    let _ = session.run(values)?;
    Ok(())
}

fn percentile(times: &[Duration], q: f64) -> Result<Duration> {
    if times.is_empty() {
        return Err(anyhow!("no timings"));
    }
    let idx = ((times.len() - 1) as f64 * q).round() as usize;
    Ok(times[idx])
}

fn elapsed_ms(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1_000.0
}

fn count_files(path: &Path) -> Result<usize> {
    if !path.exists() {
        return Ok(0);
    }
    let mut count = 0;
    for entry in std::fs::read_dir(path)? {
        let entry = entry?;
        if entry.file_type()?.is_file() {
            count += 1;
        } else if entry.file_type()?.is_dir() {
            count += count_files(&entry.path())?;
        }
    }
    Ok(count)
}
