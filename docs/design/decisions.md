# P1-700 design decisions

## 2026-09-28 — Visual world chosen without a live interview

The Impeccable workflow would normally ask the owner a structured visual-world question. This session was explicitly non-interactive and the task asked not to wait for answers, so the choice was made from `PRODUCT.md`, `docs/spec/04-gesture-studio-and-ux.md`, `docs/spec/09-device-targeting.md` and ADR-018.

### Product truth used
- Flick is an operate-mode desktop tool and HUD, not a marketing surface.
- The decisive mechanism is **point → selected device → verb gesture → visible outcome**.
- It is used from a couch/bed at 0.5-3 m, often in dim rooms, with accessibility users as the design driver.
- The app must feel local, safe, calm and fast while still avoiding stock shadcn defaults.

### Grounded direction candidates
Impeccable `concept-seed.mjs --scope direction --mode operate` returned seed `dfb11cf8` and assigned grounded candidate **6**. The grounded list was:

1. Home Assistant wall dashboard expanded into a spatial control desk. Rejected as too close to generic smart-home panels.
2. Camera calibration slate with lens grids and fiducial targets. Strong for setup, too technical for household members.
3. macOS menu-bar utility with frosted glass and system panels. Familiar, but risks disappearing into platform chrome.
4. Physical remote-control legends and appliance embossing. Clear, but too literal and not enough live feedback for targeting.
5. Safety lockout panel / industrial interlock. Safe, but too alarmist for daily living-room use.
6. **Stage manager cue desk + sightline plot. Chosen.** A room is treated like a stage, the user casts a ray, Flick locks onto a target, then a cue fires. It carries HUD states, countdowns, high contrast, sound cues and action feedback naturally.
7. Topographic room sketch / wayfinding map. Useful for devices list, but weak for gestures and HUD immediacy.

### Challenger weighing
The seed offered teletext, cyclorama, origami, CD-ROM chrome, parametric identity and datamatics challengers. Teletext and datamatics have excellent high-contrast discipline, but they over-index on bitmap/dense data and make the main window harder to scan. Cyclorama lighting supports dark-to-light state, but it is more atmospheric than operational. CD-ROM chrome would conflict with modern accessibility and shadcn re-skinning. The assigned cue-desk direction wins on both audience identification and product clarity.

### Commitments
- Creative north star: **Sightline Cue Desk**.
- Palette strategy: restrained neutral surfaces with cyan ray/focus, amber waiting/attention, green done, red failed, plus a black/white high-contrast HUD mode.
- Typography: Atkinson Hyperlegible, bundled locally under OFL, selected for 3 m HUD legibility and accessible character distinction.
- Signature interaction: the live preview owns the screen as a stage sightline; HUD states are cue cards with icons/text, not color-only badges.
- Rejected: old coral placeholder, stock shadcn theme, pure Home Assistant mimicry, neon gamer HUD, skeuomorphic remote buttons, and marketing-style glass cards.
