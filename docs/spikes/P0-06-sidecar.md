# P0-06 — Tauri sidecar spike

Date: 2026-09-28  
Lane: E8 Desktop shell (`task/E8-desktop`)  
Host: macOS 27, Apple M5, Command Line Tools only, node 26, pnpm 12.6

## Result

A Tauri 2.12 shell at `apps/desktop/src-tauri` now supervises `flick-engine` as a sidecar process:

1. Generate a per-launch 256-bit token.
2. Spawn `flick-engine --sidecar --port 0 --data-dir <app data>`.
3. Write the token as the first stdin line.
4. Parse the JSON ready line from stdout.
5. Expose `{ base_url, token }` through the `engine_endpoint` IPC command.

The engine sidecar stub binds loopback, requires `Authorization: Bearer <token>`, rejects non-loopback `Host`, and allows no `Origin` plus the dev/Tauri origins used by the shell. The full `flick-api` routes remain owned by the engine/API lanes.

## Sidecar bundling

Tauri config uses:

```json
"externalBin": ["binaries/flick-engine"]
```

Tauri expects target-triple files in `apps/desktop/src-tauri/binaries/` during bundling, for example `flick-engine-aarch64-apple-darwin`. Those generated binaries are ignored by git. `apps/desktop/scripts/prepare-sidecar.mjs` builds `flick-engine` and copies it to the correct target-triple filename. The Tauri `beforeDevCommand` and `beforeBuildCommand` run that script. `build.rs` creates a small ignored placeholder only so plain `cargo build -p flick-desktop` works without a prebuilt sidecar; Tauri dev/build overwrites it with a real engine binary.

## Measurements

Commands used the required Rust environment prefix.

| Measurement | Value |
|---|---:|
| Sidecar cold start to ready line | 241.13 ms |
| Sidecar RSS after ready | 7.42 MiB |
| Debug `.app` bundle size | 79,440 KiB (`du -sk`), 77 MiB (`du -sh`) |

Smoke evidence:

```text
engine sidecar ready: http://127.0.0.1:55330
```

`pnpm --dir apps/desktop tauri build --debug --bundles app` succeeded with Command Line Tools only. No full Xcode was needed for the debug `.app` bundle. Signing/notarization was not attempted because Developer ID and App Store Connect credentials do not exist yet.

## Entitlements and signing inputs

Configured macOS metadata:

- `NSCameraUsageDescription`
- `NSCameraUseContinuityCameraDeviceType`
- `NSLocalNetworkUsageDescription`
- `NSBonjourServices = _home-assistant._tcp`
- `LSApplicationCategoryType = public.app-category.utilities`
- Hardened runtime entitlement: `com.apple.security.device.camera`

The owner must provide:

- Apple Developer ID Application certificate (`APPLE_SIGNING_IDENTITY`).
- App Store Connect notarization credentials (`APPLE_API_KEY`, `APPLE_API_ISSUER`, `APPLE_API_KEY_PATH`).
- Tauri updater minisign private key/password for release artifacts, and the public key to replace the placeholder updater `pubkey` in `tauri.conf.json`.
- Stable Team ID / bundle identifier continuity to preserve TCC grants across updates.

## Notes and deferrals

- The shell implements restart backoff, `/health` monitoring, fatal crash-loop state after 5 crashes in 2 minutes, and graceful SIGTERM on quit (with hard-kill fallback).
- HUD is transparent, click-through, always-on-top, all Spaces, and uses `hud.html` placeholder content until the UI lane lands.
- The app updater is wired through `tauri-plugin-updater` and stores pending/rollback state plus DB snapshots. Signed channel-index rollout/revocation verification is kept interface-shaped here; the OTA lane owns the signed index producer and keys.
- TCC attribution and camera prompt wording require the real camera engine path from P0-04/P1-607; this shell has the Info.plist/entitlement wiring ready.
