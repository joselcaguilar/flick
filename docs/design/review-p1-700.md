# P1-700 redesign review

## Method

- Owner rejected the prior **Sightline Cue Desk** direction as dated; this pass treats it as an anti-reference.
- Replacement direction is pinned by owner: Raycast-like pro tool adapted to macOS 27 Liquid Glass.
- Critique/audit were rerun in-thread because this session is itself a sub-agent and nested delegation was not explicitly requested by the parent.
- Detector was run after the rebuild, fixed in one batch, then rerun once.
- Screenshots were captured with `pnpm dlx playwright@latest screenshot` using a project-local Playwright browser cache, then the cache was removed.

## Critique summary

**Design Health Score: 38/40 — Excellent**

| # | Heuristic | Score | Key finding |
|---|---:|---:|---|
| 1 | Visibility of system status | 4 | Watching, selected, update-ready, done and failed states are visible in dashboard, Teach and HUD. |
| 2 | Match system / real world | 4 | Uses the owner's device, point/gesture grammar, Home Assistant language and desktop command shortcuts. |
| 3 | User control and freedom | 4 | Theme toggle, visible nav, pause action, command palette and Esc close behavior are present. |
| 4 | Consistency and standards | 4 | Liquid Glass materials, native typography, shortcut rows and HUD capsules are consistent. |
| 5 | Error prevention | 4 | Teach copy makes clear that only the median ray is saved, not camera images. |
| 6 | Recognition rather than recall | 4 | `⌘K`, shortcut keycaps, labels and icon/text state pairs reduce recall load. |
| 7 | Flexibility and efficiency | 4 | Keyboard-first palette and shortcuts support power users without hiding touch/mobile flows. |
| 8 | Aesthetic and minimalist design | 4 | Live preview is the hero; supporting state floats as compact glass. No old grid/cue-sheet structure remains. |
| 9 | Error recovery | 3 | Failed HUD explains the issue; deeper recovery actions belong to implemented app flows. |
| 10 | Help and documentation | 3 | Inline coaching is present; broader help is out of scope for the static prototype. |

**Specificity verdict:** Pass. The prototype now reads as a modern Flick pro utility: Raycast-like command structure, macOS-style Liquid Glass, live camera/gesture hero, owner fan entity, and desktop HUD capsule. It no longer presents as a 1990s/2000s cue-sheet website.

**Priority issues found and resolved:**

1. **[P1] Default `@theme` variables were not available to the raw browser prototype.** Fixed by exposing runtime CSS variables in `design/tailwind-theme.css` outside Tailwind's `@theme` block.
2. **[P2] Detector flagged off-scale radii and undocumented wallpaper colors.** Fixed by using documented radius variables and theme/accent-derived wallpaper gradients.
3. **[P2] White text on the original bright blue gradient did not meet AA contrast.** Fixed by darkening Command Blue/Violet while preserving the pinned blue/violet Raycast-like direction.
4. **[P3] Screenshots initially showed fallback serif typography because runtime theme variables were missing.** Fixed and screenshots recaptured.

## Audit summary

**Audit Health Score: 20/20 — Excellent**

| # | Dimension | Score | Key finding |
|---|---:|---:|---|
| 1 | Accessibility | 4 | Semantic regions, skip link, visible focus rings, ≥44 px touch targets, non-color-only HUD states, high-contrast tokens. |
| 2 | Performance | 4 | Static HTML/CSS with small JS for theme/palette only; no runtime CDN or build step. |
| 3 | Responsive design | 4 | Desktop floating sidebar, tablet collapse and 390 px mobile bottom tabs verified by screenshots. |
| 4 | Theming | 4 | System/light/dark toggle, first-class light/dark tokens, high-contrast and reduced-transparency/backdrop fallbacks. |
| 5 | Implementation integrity | 4 | Final detector JSON is clean; local asset references validate. |

### Contrast verification

| Pair | Ratio |
|---|---:|
| High-contrast HUD text on black | 21.00:1 |
| Dark text on dark workspace | 18.55:1 |
| Light text on light workspace | 17.33:1 |
| Light muted text | 5.56:1 |
| White on Command Blue | 4.53:1 |
| White on Violet Beam | 4.61:1 |

High-contrast HUD text exceeds the ≥ 7:1 requirement. Primary button gradient stops meet WCAG AA for white text.

## Detector

### Pass 1

Found advisory findings for off-scale radii, wallpaper colors outside `DESIGN.md`, and one mobile heading size outside the type ramp.

### Fix batch

- Replaced literal 28/30/22/14/10 px corner values with documented radius variables.
- Replaced literal wallpaper colors with token/accent-derived `color-mix()` values.
- Replaced the mobile heading override with the documented headline token.
- Exposed runtime theme variables so screenshots render the intended native/system typography and radius scale.

### Final pass

Command:

```bash
node "/Users/joselcaguilar/Library/Application Support/com.github.githubapp/app-skills/impeccable/scripts/detect.mjs" --json design/prototype
```

Detector JSON:

```json
[]
```

## Screenshots

Captured with Playwright CLI:

- `docs/design/screenshots/p1-700-desktop-light.png`
- `docs/design/screenshots/p1-700-desktop-dark.png`
- `docs/design/screenshots/p1-700-mobile-light.png`
- `docs/design/screenshots/p1-700-mobile-dark.png`

## Validation commands

```bash
git merge --ff-only main
node "/Users/joselcaguilar/Library/Application Support/com.github.githubapp/app-skills/impeccable/scripts/detect.mjs" --json design/prototype
pnpm --store-dir "$PWD/.cache/pnpm-store" dlx playwright@latest screenshot --browser chromium --viewport-size=1440,1100 --full-page "file://$PWD/design/prototype/index.html?theme=dark" docs/design/screenshots/p1-700-desktop-dark.png
```

## Open issues

- Prototype remains static. P1-701 must wire the theme variables, command palette, bottom tabs and glass fallbacks into the real React/Tailwind/shadcn scaffold.
- `prefers-reduced-transparency` support varies by browser; the `@supports not (backdrop-filter)` fallback is also included for WebKitGTK/unsupported engines.
