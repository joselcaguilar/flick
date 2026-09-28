# P1-700 design review

## Method

- Impeccable context loaded once from the repo root.
- References read: `shape`, `new-work`, `document`, `craft-floor`, `critique`, `audit`.
- Critique/audit were performed in-thread because this session is itself a sub-agent and the parent instructions prohibit nested delegation unless explicitly requested. Detector evidence was kept deterministic and rerun after one batched fix pass.
- Screenshots skipped: `~/Library/Caches/ms-playwright` is absent and the `playwright` Node module is not installed. No global software was installed.

## Critique summary

**Design Health Score: 37/40 — Excellent**

| # | Heuristic | Score | Key finding |
|---|---:|---:|---|
| 1 | Visibility of system status | 4 | Dashboard, Teach and HUD all show watching/selected/done/failed states. |
| 2 | Match system / real world | 4 | Uses point, ray, device name, Home Assistant result and plain copy from the spec. |
| 3 | User control and freedom | 3 | Static prototype shows navigation and pause, but controls do not execute by design. |
| 4 | Consistency and standards | 4 | Sightline/cue tokens, glyphs, cards, chips and HUD states are consistent. |
| 5 | Error prevention | 4 | Teach copy stores rays/landmarks, not images; failed HUD includes reason. |
| 6 | Recognition rather than recall | 4 | Icon + text states, device name before entity ID, visible HUD hints. |
| 7 | Flexibility and efficiency | 3 | Main quick actions are present; keyboard shortcuts are not modeled in this static prototype. |
| 8 | Aesthetic and minimalist design | 4 | Product-specific visual world without stock shadcn or generic smart-home chrome. |
| 9 | Error recovery | 4 | Failed HUD and HA offline copy tell the user what happened. |
| 10 | Help and documentation | 3 | Contextual copy is present; deeper help belongs to later app surfaces. |

**Specificity verdict:** Pass. The prototype is recognizably Flick: sightline overlays, pointing rays, cue colors, local privacy language, the owner fan entity and HUD result grammar make it hard to reuse unchanged for another product.

**Priority issues found and fixed in the bounded pass:**

1. **[P2] Detector found decorative grid-background and off-ramp typography advisories.** Fixed by removing CSS grid-line backgrounds from the prototype chrome and documenting HUD/overlay type steps in `DESIGN.md` and token files.
2. **[P2] Static prototype cannot prove control behavior.** Accepted as a prototype limitation; P1-701 and later UI tasks will implement interaction against mocks/engine events.
3. **[P3] Screenshots unavailable in this host.** Recorded as an open issue; no global install was performed.

## Audit summary

**Audit Health Score: 19/20 — Excellent**

| # | Dimension | Score | Key finding |
|---|---:|---:|---|
| 1 | Accessibility | 4 | Semantic landmarks/headings, skip link, focus rings, non-color-only states, local high-contrast tokens. |
| 2 | Performance | 4 | Plain HTML/CSS, local TTF/SVG assets, no JS runtime and no CDN. |
| 3 | Responsive design | 3 | Responsive CSS covers desktop/tablet/mobile; headless screenshots were unavailable to visually verify. |
| 4 | Theming | 4 | `design/tailwind-theme.css` defines Tailwind 4 `@theme` variables plus light/dark/high-contrast scopes. |
| 5 | Implementation integrity | 4 | Detector final pass is clean; assets and references validate. |

### Contrast verification

| Pair | Ratio |
|---|---:|
| High-contrast HUD text on black | 21.00:1 |
| Dark HUD text on Night Surface | 15.98:1 |
| Light body text on Light Booth | 15.54:1 |
| Light muted text on Light Booth | 6.03:1 |
| Dark muted text on Night Booth | 9.56:1 |
| Light primary button text | 5.05:1 |
| Dark primary button text | 12.60:1 |

High-contrast HUD text exceeds the ≥ 7:1 requirement.

## Detector

### Pass 1

Found 11 advisory findings:

- `codex-grid-background` on the prototype CSS.
- `design-system-font-size` for h2, h3, hero copy, SVG/HUD labels.
- `design-system-radius` for the 18 px brand mark corner.

All were fixed in one batch.

### Final pass

Command:

```bash
node "/Users/joselcaguilar/Library/Application Support/com.github.githubapp/app-skills/impeccable/scripts/detect.mjs" --json design/prototype
```

Detector JSON:

```json
[]
```

## Validation commands

```bash
node "/Users/joselcaguilar/Library/Application Support/com.github.githubapp/app-skills/impeccable/scripts/detect.mjs" --json design/prototype
python3 - <<'PY'
# Validated JSON files, local references and contrast ratios.
PY
```

## Open issues

- No headless screenshots were captured because Playwright browsers/module are not available on this host.
- The prototype is intentionally static; P1-701 must wire these tokens into the actual Vite/Tailwind/shadcn scaffold and live events.
