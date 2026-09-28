---
name: Flick
description: Local-first gesture remote for Home Assistant, shaped as a sightline cue desk.
colors:
  light-bg: "#F4F7F2"
  light-surface: "#FFFFFF"
  light-surface-raised: "#E7EFE8"
  light-text: "#13201B"
  light-muted: "#4E625B"
  light-border: "#C6D4CC"
  dark-bg: "#07100E"
  dark-surface: "#0E1816"
  dark-surface-raised: "#172620"
  dark-text: "#EAF3EE"
  dark-muted: "#A8BBB2"
  dark-border: "#2E433B"
  cue-cyan: "#007A8C"
  cue-cyan-bright: "#66E2F2"
  cue-amber: "#B76B00"
  cue-amber-bright: "#FFC857"
  cue-green: "#176B46"
  cue-green-bright: "#72E59A"
  cue-red: "#A9282F"
  cue-red-bright: "#FF7A84"
  hc-bg: "#000000"
  hc-text: "#FFFFFF"
  hc-focus: "#FFFF00"
typography:
  display:
    fontFamily: "Atkinson Hyperlegible, ui-sans-serif, system-ui, sans-serif"
    fontSize: "clamp(2.25rem, 4vw, 4rem)"
    fontWeight: 700
    lineHeight: 0.98
    letterSpacing: "-0.03em"
  headline:
    fontFamily: "Atkinson Hyperlegible, ui-sans-serif, system-ui, sans-serif"
    fontSize: "clamp(1.5rem, 2.6vw, 2.5rem)"
    fontWeight: 700
    lineHeight: 1.08
    letterSpacing: "-0.02em"
  title:
    fontFamily: "Atkinson Hyperlegible, ui-sans-serif, system-ui, sans-serif"
    fontSize: "1.125rem"
    fontWeight: 700
    lineHeight: 1.25
  body:
    fontFamily: "Atkinson Hyperlegible, ui-sans-serif, system-ui, sans-serif"
    fontSize: "1rem"
    fontWeight: 400
    lineHeight: 1.55
  label:
    fontFamily: "Atkinson Hyperlegible, ui-sans-serif, system-ui, sans-serif"
    fontSize: "0.8125rem"
    fontWeight: 700
    lineHeight: 1.2
    letterSpacing: "0.04em"
  hud-title:
    fontFamily: "Atkinson Hyperlegible, ui-sans-serif, system-ui, sans-serif"
    fontSize: "1.375rem"
    fontWeight: 700
    lineHeight: 1.08
  hud-body:
    fontFamily: "Atkinson Hyperlegible, ui-sans-serif, system-ui, sans-serif"
    fontSize: "1.25rem"
    fontWeight: 400
    lineHeight: 1.25
  overlay-label:
    fontFamily: "Atkinson Hyperlegible, ui-sans-serif, system-ui, sans-serif"
    fontSize: "1.5rem"
    fontWeight: 700
    lineHeight: 1.15
rounded:
  xs: "6px"
  sm: "10px"
  md: "16px"
  lg: "24px"
  xl: "32px"
  pill: "999px"
spacing:
  1: "4px"
  2: "8px"
  3: "12px"
  4: "16px"
  5: "20px"
  6: "24px"
  8: "32px"
  10: "40px"
  12: "48px"
  16: "64px"
components:
  button-primary:
    backgroundColor: "{colors.cue-cyan}"
    textColor: "{colors.light-surface}"
    rounded: "{rounded.pill}"
    padding: "12px 20px"
    typography: "{typography.label}"
  button-secondary:
    backgroundColor: "{colors.light-surface-raised}"
    textColor: "{colors.light-text}"
    rounded: "{rounded.pill}"
    padding: "12px 18px"
    typography: "{typography.label}"
  card-surface:
    backgroundColor: "{colors.light-surface}"
    textColor: "{colors.light-text}"
    rounded: "{rounded.lg}"
    padding: "24px"
  hud-pill:
    backgroundColor: "{colors.dark-surface}"
    textColor: "{colors.dark-text}"
    rounded: "{rounded.xl}"
    width: "360px"
    height: "96px"
---

# Design System: Flick

## Overview

**Creative North Star: "Sightline Cue Desk"**

Flick treats the room as a small stage: the camera is the booth, the user's pointing hand casts a sightline, and every gesture is a cue that must visibly lock, fire and resolve. The visual world borrows from stage manager cue sheets, lighting plots and camera viewfinders rather than from generic smart-home cards. Lines, rings and tally chips show what Flick sees, what it selected and what happened.

The system is calm in the main window and unmistakable in the HUD. Most surfaces are neutral, gridded and low-glare; the live ray, countdown and result states carry the color. The design should feel like a reliable local instrument: precise, readable from the couch, and impossible to mistake for stock shadcn.

**Key Characteristics:**
- Sightlines and target rings are the signature visual primitives.
- Cyan means aiming/selection, amber means waiting/needs attention, green means done, red means failed; text and icons always repeat the state.
- Atkinson Hyperlegible is bundled and used everywhere for distance legibility.
- Surfaces are layered by tone, soft shadow and hairline grids, not decorative glass.
- Motion is a cue, not ornament: lock-on ring, countdown bar, sent pulse, then result.

## Colors

The palette is a restrained stage booth: soft neutral work surfaces, near-black HUD layers and a small set of cue colors reserved for state and sightline feedback.

### Primary
- **Cue Cyan**: selection, pointing rays, focused primary actions and active device chips. Use sparingly so selection reads immediately.
- **Cue Cyan Bright**: the dark-mode and HUD equivalent of Cue Cyan. It may draw lines, rings and focused states on dark surfaces.

### Secondary
- **Cue Amber**: waiting, update-ready banners, coaching and attention states that are not failures.
- **Cue Green**: successful completion and confirmed safe state.
- **Cue Red**: failed, blocked or unsafe actions. Never rely on red alone; include an icon and reason.

### Neutral
- **Light Booth**: the default light background, low glare and slightly green so camera panels do not float in sterile white.
- **Light Surface / Raised Surface**: cards, controls, step panels and preview chrome.
- **Night Booth / Night Surface**: dark mode and HUD foundations.
- **High-Contrast Black / White / Focus Yellow**: HUD and accessibility mode, built for ≥ 7:1 text contrast.

### Named Rules
**The Cue Rarity Rule.** At rest, no more than one cue color family should dominate a view; color marks live state, not decoration.

**The Red Needs Words Rule.** Any failed/unsafe state must include an icon plus a plain-language reason such as "Home Assistant unavailable".

## Typography

**Display Font:** Atkinson Hyperlegible (local TTF, OFL)
**Body Font:** Atkinson Hyperlegible
**Label/Mono Font:** Atkinson Hyperlegible with tabular numerals; system mono is only for code or raw identifiers.

**Character:** The face is accessible and plainspoken: distinctive letterforms for distance reading, enough warmth for household members, and no decorative tech costume.

### Hierarchy
- **Display** (700, clamp 2.25rem-4rem, 0.98): major product and flow headings only.
- **Headline** (700, clamp 1.5rem-2.5rem, 1.08): screen titles, step titles and dashboard group headings.
- **Title** (700, 1.125rem, 1.25): cards, row headings, HUD secondary labels.
- **Body** (400, 1rem, 1.55): explanatory copy and form help; keep line length to 65-75 characters.
- **Label** (700, 0.8125rem, 0.04em): control labels, status tags and compact metadata. Use uppercase only for very short cue labels.
- **HUD Title** (700, 1.375rem, 1.08) and **HUD Body** (400, 1.25rem, 1.25): always-on-top overlay text, never smaller.
- **Overlay Label** (700, 1.5rem, 1.15): live preview labels and ray annotations.

### Named Rules
**The Three-Meter Rule.** HUD primary text is never below 20 px; high-contrast HUD text uses black/white pairs that exceed 7:1.

**The Identifier Rule.** Raw Home Assistant entity IDs may appear as metadata, never as the primary name. Show "Ventilador dormitorio" first, then `fan.ventilador_dormitorio`.

## Layout

Flick uses an instrument-panel layout: a live preview or active teaching pane owns the largest area, while status, steps and activity sit in compact rails. Desktop screens use a 12-column grid with 24 px gutters and 32-48 px section spacing. Narrow layouts stack preview first, then primary actions, then diagnostics.

The HUD is its own layout: a 360×96 pill with a 56 px icon/ring zone, a text block and a countdown/result slot. It never carries update banners and never takes focus.

Preview panels use an internal sightline grid: hand skeleton, ray, selected label, confidence ring and pinned observations all align to the video bounds. If the camera feed is unavailable or intentionally omitted in a prototype, use a skeleton overlay placeholder rather than a fake room photo.

## Elevation & Depth

Depth is functional. Panels are separated by tonal layers, 1 px borders and soft offset shadows. Blur is allowed only when a HUD sits over a live wallpaper or camera view; it must improve legibility, not decorate a card.

### Shadow Vocabulary
- **Panel Rest** (`0 1px 2px rgba(19, 32, 27, 0.08), 0 10px 30px rgba(19, 32, 27, 0.08)`): cards and preview containers.
- **Panel Lift** (`0 10px 20px rgba(19, 32, 27, 0.10), 0 24px 60px rgba(19, 32, 27, 0.12)`): active teaching pane or open popover.
- **HUD Scrim** (`0 18px 60px rgba(0, 0, 0, 0.34)`): HUD pill over wallpapers.

### Named Rules
**The Useful Blur Rule.** Backdrop blur appears only behind the HUD or overlays on video/wallpaper, and must preserve contrast in high-contrast mode by switching to an opaque background.

## Shapes

Shapes come from lenses and cue keys: rounded enough to feel safe, precise enough to feel calibrated. Cards use 24 px corners, controls use pill shapes, small chips use 999 px pills, and preview panels use 32 px corners with clipped overlay content. Rings and arcs may be circular, but progress also needs text or a bar.

Glyphs and domain icons use 64×64 SVGs, rounded strokes, no filled emoji assets and `currentColor` so state colors and high-contrast tokens can own them. Gesture paths use motion trails, arrows and separation marks; domain icons use simplified appliance silhouettes.

## Components

### shadcn/ui Re-skinning
Use shadcn/ui only for behavior and accessibility. Replace the default CSS variables, radii, focus rings and shadows with Flick tokens before any component ships. No stock `slate`, `zinc`, default border radius, or generic `primary` palette may appear in app CSS.

### Buttons
- **Shape:** cue-key pills (999 px) with a minimum 44 px touch target.
- **Primary:** Cue Cyan background, light text, bold label, subtle downward press on active.
- **Secondary:** raised neutral surface with text color and a 1 px border.
- **Danger:** red outline or fill only when paired with action text that names the consequence.
- **Focus:** 3 px focus ring using Cue Amber / high-contrast Focus Yellow.

### Cards / Containers
Cards are not interchangeable tiles. Each card needs a named job: status, preview, step, activity, or banner. Status cards are compact and text-led; preview cards are large and visual; banners use amber with a restart/update action.

### Inputs / Fields
Fields sit on raised surfaces with a 1 px border, 10 px radius and visible focus ring. Search results use sentence labels and live state; raw HA IDs are secondary metadata.

### Chips and Status Tags
Chips carry device state, camera state, gesture type and place status. They must include icon/text, not color alone. Active chips use Cue Cyan; warning chips use Cue Amber; success/failure chips include ✓ or ✕.

### HUD
HUD states follow the grammar: selected = device + icon + "selected" + 4 s countdown; done = device + action + ✓; failed = device + reason + ✕. Text is ≥ 20 px and the pill uses an opaque high-contrast variant when requested.

### Gesture Glyphs
The gesture glyph set lives in `design/glyphs/`. Use it for point, circles, two-hand separate, thumbs up/down, pinch dial, swipes and open palm. Animate trails only as an enhancement; static glyphs are the source of truth under Reduce Motion.

### Domain Icons
The domain icon set lives in `design/icons/`. Use it for `light`, `fan`, `cover`, `media_player`, `climate`, `switch` and `lock`. Icons are line-based and theme-colored; do not mix in unrelated icon packs without a license review.

### Motion & Reduce Motion
Motion has four roles: ray acquisition, dwell countdown, gesture vote progress and result pulse. Standard transitions use 180 ms ease-out; selection/dwell progress may run 500 ms or 4 s because those durations match the product state. Under Reduce Motion, replace ring animation with a static progress bar, explicit seconds remaining and immediate state changes. Disable confetti entirely.

## Do's and Don'ts

### Do:
- **Do** make the live preview or HUD state immediately answer: what Flick sees, what it selected, what happened.
- **Do** keep HUD text at 20 px or larger and repeat state with icons/text.
- **Do** use local bundled fonts and SVG assets; the app must work offline.
- **Do** use token variables from `design/tailwind-theme.css` in the UI scaffold.
- **Do** keep main-window color calm and reserve saturated color for live state.

### Don't:
- **Don't** ship the stock shadcn theme, default gray palette or generic rounded card grid.
- **Don't** convey selected/done/failed by color alone.
- **Don't** fake camera imagery, user metrics, testimonials or Home Assistant state that the product has not observed.
- **Don't** load fonts, icons or images from a runtime CDN.
- **Don't** use the old coral placeholder accent unless a future design decision explicitly replaces this system.
