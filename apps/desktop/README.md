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

## Local macOS bundle signing

Use the local bundle script for camera-test builds:

```sh
pnpm run bundle:local
```

It builds `ui/`, runs `tauri build --debug --bundles app`, then ad-hoc signs
both `Contents/MacOS/flick-desktop` and `Contents/MacOS/flick-engine` with
`--options runtime`, `src-tauri/entitlements.plist`, and identifier
`app.flick.desktop` before running `codesign --verify --deep --strict`.

The sidecar needs the same `com.apple.security.device.camera` entitlement as
the app because it opens the camera under hardened runtime. Ad-hoc signatures
change their cdhash on every rebuild, so macOS may prompt for camera access
again after each local bundle rebuild. Developer ID signing and notarization
provide the stable signing identity needed to avoid those repeated prompts.
