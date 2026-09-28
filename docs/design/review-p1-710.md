# P1-710 accessibility and design-quality review

Date: 2026-09-28  
Branch: `task/E7-ui` after merging `main` at `8f3d1dc`

## Scope

Reviewed every Phase 1 UI route: Dashboard, Onboarding, Gestures, Gesture Studio, Devices, Teach, Places, Re-align, Mappings, Mapping editor, Cameras, Activity, Settings, Pro, and the HUD shell. P1-903 journeys were not started.

## Fixes completed

- Replaced emoji gesture language with `GestureGlyph`/design glyph rendering and plain-text mock labels.
- Removed duplicate Onboarding step-5 heading/actions by letting `OnboardingRoute` own the step controls and simplifying `OnboardingTeachDeviceStep`.
- Re-captured the four dashboard screenshots in `docs/design/screenshots/ui/`; the dashboard now uses an operate-scale header and balanced columns.
- Added accessible labels for gesture import and Settings form fields, labelled primary navigation, repaired the brand accessible name mismatch, and improved danger button contrast.
- Strengthened command palette semantics and ranked label matches ahead of description matches so keyboard search for “settings” reaches Settings first.

## Impeccable audit and critique

- Detector: `pnpm detect` / `node .../detect.mjs --json ui/src` returned `[]`.
- Audit evidence: route HTTP probes returned 200; Lighthouse accessibility was run separately for browser evidence.
- Critique summary: Flick has a strong operating-console world, clear HUD state visibility, and consistent point-then-gesture semantics. Remaining product-design opportunities are deferred rather than blockers: Home can become more task-led, onboarding can be further shortened, Teach copy can hide more engineering terminology, selection persistence/global suppression can be made more explicit, and HUD recovery states can add stronger next actions.
- `.impeccable/design.json` is stale and was intentionally not repaired, per instruction.

## Lighthouse accessibility

Tool used: `pnpm dlx lighthouse --only-categories=accessibility --preset=desktop` against the MSW dev server.

| Route | Score |
| --- | ---: |
| `/` | 100 |
| `/onboarding` | 100 |
| `/gestures` | 100 |
| `/gestures/new` | 100 |
| `/devices` | 100 |
| `/devices/teach` | 100 |
| `/devices/places` | 100 |
| `/devices/realign` | 100 |
| `/mappings` | 100 |
| `/mappings/new` | 100 |
| `/cameras` | 100 |
| `/activity` | 100 |
| `/settings` | 100 |
| `/pro` | 100 |
| `/hud.html?state=selected` | 95 |

## Keyboard-only walkthrough

Used Playwright/Chromium keyboard probes with the MSW dev server.

- Skip link, sidebar navigation, toolbar actions, and bottom-tab equivalent focus paths are reachable without pointer input.
- Command palette opens with Ctrl/⌘K, focuses the search field, ranks label matches first, and Enter activates the selected command.
- Main task routes expose route-local keyboard controls: onboarding steps/actions, gesture import/toggles, studio record/train controls, device teach/test actions, mappings tabs/editor chips/sensitive flow controls, camera actions, activity filters/export, and Settings controls.
- Static informational routes with no route-local controls (for example Pro/Places in their current empty-state sections) do not trap focus; focus cycles back to shell navigation normally.

## Validation

Passed: `pnpm lint && pnpm typecheck && pnpm test && pnpm build && pnpm detect`.
