//! `gen-api` writes the committed Flick OpenAPI contract.

use std::{fs, path::Path};

use anyhow::{Context, Result};

/// Regenerates `crates/flick-api/openapi.json`.
pub fn run() -> Result<()> {
    let out = Path::new("crates/flick-api/openapi.json");
    let json = flick_api::openapi_json_pretty().context("serialize OpenAPI")?;
    fs::write(out, format!("{json}\n")).with_context(|| format!("write {}", out.display()))?;
    Ok(())
}
