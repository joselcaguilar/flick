# Flick desktop shell

## Bundled baseline models

`pnpm run prepare-build` stages verified baseline models into
`src-tauri/resources/models/` so Tauri bundles them at
`Contents/Resources/models/`.

Populate the gitignored root `models/cache/` before building:

```sh
cargo xtask fetch-models
# then add the derived gesture models, either by copying a verified build:
cp ~/Repos/personal/flick-wt/models/models/cache/gesture_embedder.onnx models/cache/
cp ~/Repos/personal/flick-wt/models/models/cache/canned_gesture_classifier.onnx models/cache/
# or by regenerating them with Python 3.12:
python3.12 tools/training/convert.py convert-gesture
```

`prepare-models` verifies SHA-256 for every bundled baseline entry in
`models/manifest.toml` and fails instead of bundling missing or unverified
files. DINOv2/scene embedding remains OTA-only and is intentionally excluded.

TODO: CI should source these artifacts from the signed OTA baseline pack once
that pack is published.
