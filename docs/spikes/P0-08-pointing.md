# P0-08 — Pointing accuracy & places

Status: **library spike complete on synthetic data; real MacBook dataset pending**.

## Findings

- The Phase 1 library path is viable: `flick-spatial` implements eye-rooted and finger-only rays, 2+ spot triangulation, Gaussian anchor scoring, the selection FSM, DINO-style place matching, and two-anchor Wahba/Kabsch re-align.
- Synthetic property tests recover eye-rooted rays within 1°, triangulate point anchors within 3 cm with ≤ 1° residual, and recover re-align rotations with ≤ 0.1° residual.
- The selector uses the spec defaults: 200 ms point-pose stability, 500 ms dwell, 4 s selected window, 10° base tolerance, 5° ambiguity margin, and 60°/s reselect lock.
- Place matching keeps the spec threshold at cosine ≥ 0.90 until DINOv2-small is measured on real rooms; two-anchor re-align accepts residual ≤ 5°.
- The owner's fan path is represented by synthetic replay fixtures: `point_fan_circle` expects `fan.turn_on {percentage: 1}` for `fan.ventilador_dormitorio`; `point_fan_stop` expects `fan.turn_off`.

## Decision

Proceed with the Phase 1 architecture: teach by pointing from two or more spots, default to eye-rooted rays when face keypoints are available, fall back to finger-only rays, and pause targeting when the place signature needs re-align.

## Still required before hardware go/no-go

- Record the normative dataset: ≥ 3 people × 5 targets × 3 positions on a MacBook camera, including the ceiling fan.
- Measure median angular error per ray model, face-visibility rate, selection accuracy at ≥ 20° and ≥ 15° separations, wrong-device rate, and same/moved/other-room scene-signature similarity.
- If eye-rooted selection is < 90% at 20° separation, update `09-device-targeting.md` with the fallback UX: larger-separation guidance plus HUD candidate confirmation.
