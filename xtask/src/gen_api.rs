//! `gen-api` writes the committed Flick OpenAPI contract and UI TypeScript bindings.

use std::{fs, path::Path, process::Command};

use anyhow::{Context, Result, bail};

/// Regenerates `crates/flick-api/openapi.json` and, when available, `ui/src/api/schema.d.ts`.
pub fn run() -> Result<()> {
    let out = Path::new("crates/flick-api/openapi.json");
    let json = flick_api::openapi_json_pretty().context("serialize OpenAPI")?;
    fs::write(out, format!("{json}\n")).with_context(|| format!("write {}", out.display()))?;
    generate_typescript(out)?;
    Ok(())
}

fn generate_typescript(openapi: &Path) -> Result<()> {
    let generator = Path::new("ui/node_modules/.bin/openapi-typescript");
    let out = Path::new("ui/src/api/schema.d.ts");
    if !generator.exists() {
        println!(
            "hint: {} not found; run UI dependency install to generate {}",
            generator.display(),
            out.display()
        );
        return Ok(());
    }

    if let Some(parent) = out.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    let status = Command::new(generator)
        .arg(openapi)
        .arg("-o")
        .arg(out)
        .status()
        .with_context(|| format!("run {}", generator.display()))?;
    if !status.success() {
        bail!("{} failed with status {status}", generator.display());
    }
    Ok(())
}
