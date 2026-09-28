use std::error::Error;

use flick_vision::{EpKind, ModelSet, OrtSessionFactory};

#[test]
fn selected_model_contract_matches_manifest_when_cache_is_present() -> Result<(), Box<dyn Error>> {
    let models = match ModelSet::load("models/manifest.toml") {
        Ok(models) => models,
        Err(err) if err.to_string().contains("model unavailable") => return Ok(()),
        Err(err) => return Err(Box::new(err)),
    };
    let factory = OrtSessionFactory::new(models.root().join("models/cache"));
    for (id, path) in models.verified_paths() {
        let Some(entry) = models.model(id) else {
            continue;
        };
        if entry.format != "onnx" {
            continue;
        }
        let (session, _) = factory.build_session(path, EpKind::Cpu)?;
        for expected in &entry.inputs {
            assert!(
                session
                    .inputs()
                    .iter()
                    .any(|actual| actual.name() == expected.name),
                "model {id} is missing input {}",
                expected.name
            );
        }
        for expected in &entry.outputs {
            assert!(
                session
                    .outputs()
                    .iter()
                    .any(|actual| actual.name() == expected.name),
                "model {id} is missing output {}",
                expected.name
            );
        }
    }
    Ok(())
}
