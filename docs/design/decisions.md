# P1-700 design decisions

## 2026-09-28 — Initial world rejected by owner

The first P1-700 prototype committed **Sightline Cue Desk**: a stage-manager/cue-sheet metaphor with sightlines, gridded preview structure and Atkinson Hyperlegible as the default face. The owner rejected it: "I want something more modern and cool, it's like 1992 website."

That feedback is binding. The previous look is now an anti-reference, not a baseline to refine.

## 2026-09-28 — Replacement visual world: Liquid Command Remote

### Pinned owner direction

- Raycast-like pro tool, adapted to macOS 27 design styles.
- Crisp, dark-first, equally polished light theme.
- Subtle gradients, sharp modern native type, compact but airy density, keyboard-first behavior with `⌘K` and visible shortcuts.
- macOS Liquid Glass language: translucent layered materials, blur/saturation, specular edge highlights, floating sidebar/toolbar, concentric rounded corners, vibrancy and a floating glass HUD capsule.
- Opaque fallback for reduced transparency and unsupported `backdrop-filter`.
- Responsive from 360 px phone to large desktop; phone uses bottom tabs and sheets; HUD remains desktop-only.

### Product truth preserved

- Flick remains a local-first gesture remote for Home Assistant.
- The primary mechanism remains point → selected device → verb gesture → outcome.
- Copy stays short, human and spec-grounded.
- The owner's device remains `fan.ventilador_dormitorio` / "Ventilador dormitorio".
- HUD text remains ≥ 20 px and state is never conveyed by color alone.
- Fonts/icons remain bundled or platform-native; no runtime CDN.

### New commitments

- Creative north star: **Liquid Command Remote**.
- Default font stack: `system-ui, -apple-system` first so macOS renders SF Pro natively. SF is not bundled. Inter is bundled as the OFL fallback for Windows/Linux.
- Monospace: Geist Mono, bundled under OFL, for entity IDs and shortcuts only.
- Atkinson Hyperlegible remains bundled only as a future accessibility preference, not the default.
- Visual system: dark-first glass workspace, floating sidebar/toolbar, command palette, live-preview hero, bottom mobile tabs, glass HUD capsule, high-contrast opaque HUD.

### Rejected alternatives

- **Sightline Cue Desk / stage cue sheets:** explicitly rejected by the owner as dated.
- **Gridded raycast boards:** too close to the rejected look and detector-prone generated UI.
- **Home Assistant mimicry:** would hide Flick's point-then-gesture mechanism inside generic smart-home dashboard chrome.
- **Stock shadcn:** acceptable only for behavior/accessibility primitives; default theme is banned.
- **Neon gamer HUD:** visually loud and less trustworthy for accessibility and household use.
- **Bundled SF Pro:** prohibited by license; use native platform stack instead.

## 2026-09-28 — Owner approval

The owner approved the v2 "Liquid command UI" direction (Raycast-like pro tool adapted to macOS 27 Liquid Glass; light + dark first-class; responsive down to 360 px). This satisfies the P1-700 approval criterion; P1-701 and later UI tasks build on it.
