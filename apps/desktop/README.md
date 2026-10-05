# Flick desktop shell

## Bundled baseline models

`pnpm run prepare-build` stages verified baseline models into
`src-tauri/resources/models/` so Tauri bundles them at
`Contents/Resources/models/`.

Populate the gitignored root `models/cache/` before building:

```sh
cargo xtask fetch-models
# then generate the derived gesture models with Python 3.12 and the converter versions pinned
# in the `models` job of .github/workflows/ci.yml (tensorflow 2.21.0, tf2onnx 1.17.0, onnx 1.23.0):
python3.12 tools/training/convert.py fetch gesture_recognizer_task
python3.12 tools/training/convert.py convert-gesture
```

`convert-gesture` canonicalizes the tf2onnx output, so the same pinned `.task` always produces the
bytes pinned in the manifest.

`prepare-models` verifies SHA-256 for every bundled baseline entry in
`models/manifest.toml` and fails instead of bundling missing or unverified
files. DINOv2/scene embedding remains OTA-only and is intentionally excluded.

TODO: CI regenerates these artifacts in its `models` job; source them from the signed OTA baseline
pack once that pack is published.

## Release builds (CI)

`.github/workflows/ci.yml` runs when a GitHub release is published, or by hand from Actions → CI →
Run workflow. It builds unsigned bundles on native runners:

| Platform | Artifact |
|---|---|
| Windows x64, Windows arm64 | NSIS installer, `Flick_<version>_<arch>-setup.exe` |
| macOS Apple Silicon | `Flick_<version>_aarch64.dmg`, ad-hoc signed |
| Linux x64, Linux arm64 | `.AppImage` and `.deb` |

A manual run keeps the bundles as workflow artifacts. A published release gets them attached once
every check passes.

Nothing is signed with a trusted identity yet, so first launch needs one extra step:

- **macOS:** open Flick once, then System Settings → Privacy & Security → Open Anyway.
- **Windows:** in the SmartScreen prompt, More info → Run anyway.

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
