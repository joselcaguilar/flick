//! `fetch-models` downloads selected manifest models into `models/cache/` and verifies SHA-256.

use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context, Result, bail};
use clap::Args as ClapArgs;

/// Arguments for `cargo xtask fetch-models`.
#[derive(Debug, Clone, ClapArgs)]
pub struct Args {
    /// Model manifest path.
    #[arg(long, default_value = "models/manifest.toml")]
    pub manifest: PathBuf,
    /// Also fetch entries that are not marked selected.
    #[arg(long)]
    pub all: bool,
}

#[derive(Debug, Clone, Default)]
struct ManifestModel {
    id: String,
    source_url: String,
    sha256: String,
    cache_path: PathBuf,
    conversion_status: Option<String>,
}

impl ManifestModel {
    fn selected(&self) -> bool {
        self.conversion_status
            .as_deref()
            .is_none_or(|status| status.starts_with("selected"))
    }
}

/// Runs model fetching and verification.
pub fn run(args: Args) -> Result<()> {
    let manifest_text = fs::read_to_string(&args.manifest)
        .with_context(|| format!("reading {}", args.manifest.display()))?;
    let models = parse_manifest_models(&manifest_text);
    if models.is_empty() {
        bail!("no [[models]] entries found in {}", args.manifest.display());
    }
    for model in models.iter().filter(|model| args.all || model.selected()) {
        fetch_one(model)?;
    }
    Ok(())
}

fn fetch_one(model: &ManifestModel) -> Result<()> {
    if model.id.is_empty()
        || model.source_url.is_empty()
        || model.sha256.is_empty()
        || model.cache_path.as_os_str().is_empty()
    {
        bail!("manifest entry is missing id/source_url/sha256/cache_path: {model:?}");
    }
    if model.cache_path.exists() {
        verify_sha256(&model.cache_path, &model.sha256)
            .with_context(|| format!("verifying existing {}", model.cache_path.display()))?;
        println!("{} already verified", model.id);
        return Ok(());
    }
    if let Some(parent) = model.cache_path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    let partial = model.cache_path.with_extension("download");
    if let Some(path) = model.source_url.strip_prefix("file://") {
        fs::copy(path, &partial).with_context(|| format!("copying {path}"))?;
    } else {
        let status = Command::new("curl")
            .arg("--fail")
            .arg("--location")
            .arg("--show-error")
            .arg("--output")
            .arg(&partial)
            .arg(&model.source_url)
            .status()
            .with_context(|| format!("starting curl for {}", model.id))?;
        if !status.success() {
            let _ = fs::remove_file(&partial);
            bail!("curl failed for {} from {}", model.id, model.source_url);
        }
    }
    verify_sha256(&partial, &model.sha256)
        .with_context(|| format!("verifying downloaded {}", model.id))?;
    fs::rename(&partial, &model.cache_path)
        .with_context(|| format!("moving {} into place", model.cache_path.display()))?;
    println!("fetched {} -> {}", model.id, model.cache_path.display());
    Ok(())
}

fn parse_manifest_models(text: &str) -> Vec<ManifestModel> {
    let mut models = Vec::new();
    let mut current: Option<ManifestModel> = None;
    for line in text.lines() {
        let line = line.trim();
        if line == "[[models]]" {
            if let Some(model) = current.take() {
                models.push(model);
            }
            current = Some(ManifestModel::default());
            continue;
        }
        let Some(model) = current.as_mut() else {
            continue;
        };
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        let value = unquote(value.trim());
        match key {
            "id" => model.id = value,
            "source_url" => model.source_url = value,
            "sha256" => model.sha256 = value,
            "cache_path" => model.cache_path = PathBuf::from(value),
            "conversion_status" => model.conversion_status = Some(value),
            _ => {}
        }
    }
    if let Some(model) = current {
        models.push(model);
    }
    models
}

fn unquote(value: &str) -> String {
    value.trim_matches('"').trim_matches('\'').to_owned()
}

fn verify_sha256(path: &Path, expected: &str) -> Result<()> {
    let actual = sha256(path)?;
    if actual.eq_ignore_ascii_case(expected) {
        Ok(())
    } else {
        bail!(
            "sha256 mismatch for {}: expected {}, got {}",
            path.display(),
            expected,
            actual
        );
    }
}

fn sha256(path: &Path) -> Result<String> {
    let mut file = fs::File::open(path).with_context(|| format!("opening {}", path.display()))?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .with_context(|| format!("reading {}", path.display()))?;
    let output = Command::new("shasum")
        .arg("-a")
        .arg("256")
        .arg(path)
        .output()
        .with_context(|| format!("running shasum for {}", path.display()))?;
    if output.status.success() {
        let text = String::from_utf8_lossy(&output.stdout);
        if let Some(hash) = text.split_whitespace().next() {
            return Ok(hash.to_owned());
        }
    }
    bail!("shasum failed for {}", path.display());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_selected_manifest_entries() {
        let models = parse_manifest_models(
            r#"
[[models]]
id = "palm_detection_full"
source_url = "https://example.test/model.onnx"
sha256 = "abc"
cache_path = "models/cache/model.onnx"
conversion_status = "selected"

[[models]]
id = "blocked"
source_url = "https://example.test/blocked.task"
sha256 = "def"
cache_path = "models/cache/blocked.task"
conversion_status = "blocked"
"#,
        );
        assert_eq!(models.len(), 2);
        assert!(models[0].selected());
        assert!(!models[1].selected());
    }
}
