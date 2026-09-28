---
name: Flick
description: GitHub Primer operations console for local Home Assistant gesture control.
colors:
  bg: "var(--flick-bg)"
  bg-subtle: "var(--flick-bg-subtle)"
  bg-inset: "var(--flick-bg-inset)"
  sidebar: "var(--flick-sidebar)"
  surface: "var(--flick-surface)"
  surface-elevated: "var(--flick-surface-elevated)"
  text: "var(--flick-text)"
  muted: "var(--flick-muted)"
  vibrant-text: "var(--flick-vibrant-text)"
  border: "var(--flick-border)"
  border-muted: "var(--flick-border-muted)"
  border-strong: "var(--flick-border-strong)"
  button-bg: "var(--flick-btn-bg)"
  button-hover: "var(--flick-btn-hover)"
  button-active: "var(--flick-btn-active)"
  neutral-muted: "var(--flick-neutral-muted)"
  neutral-subtle: "var(--flick-neutral-subtle)"
  accent-blue: "var(--flick-accent-blue)"
  accent-emphasis: "var(--flick-accent-emphasis)"
  accent-muted: "var(--flick-accent-muted)"
  accent-border: "var(--flick-accent-border)"
  on-accent: "var(--flick-on-accent)"
  success-emphasis: "var(--flick-success-emphasis)"
  success-muted: "var(--flick-success-muted)"
  attention-emphasis: "var(--flick-attention-emphasis)"
  attention-muted: "var(--flick-attention-muted)"
  danger-emphasis: "var(--flick-danger-emphasis)"
  danger-muted: "var(--flick-danger-muted)"
  done-muted: "var(--flick-done-muted)"
  accent-violet: "var(--flick-accent-violet)"
  focus: "var(--flick-focus)"
  stage-bg: "#0d1117"
  stage-text: "#f0f6fc"
  stage-muted: "#9198a1"
  emphasis-icon: "#ffffff"
typography:
  display:
    fontFamily: "Mona Sans, -apple-system, BlinkMacSystemFont, 'Segoe UI', 'Noto Sans', Helvetica, Arial, sans-serif"
    fontSize: "2rem"
    fontWeight: 600
    lineHeight: 1.25
    letterSpacing: "-0.01em"
  headline:
    fontFamily: "Mona Sans, -apple-system, BlinkMacSystemFont, 'Segoe UI', 'Noto Sans', Helvetica, Arial, sans-serif"
    fontSize: "1.75rem"
    fontWeight: 600
    lineHeight: 1.25
    letterSpacing: "-0.01em"
  title:
    fontFamily: "Mona Sans, -apple-system, BlinkMacSystemFont, 'Segoe UI', 'Noto Sans', Helvetica, Arial, sans-serif"
    fontSize: "1rem"
    fontWeight: 600
    lineHeight: 1.5
    letterSpacing: "normal"
  body:
    fontFamily: "Mona Sans, -apple-system, BlinkMacSystemFont, 'Segoe UI', 'Noto Sans', Helvetica, Arial, sans-serif"
    fontSize: "0.875rem"
    fontWeight: 400
    lineHeight: 1.5
  label:
    fontFamily: "Mona Sans, -apple-system, BlinkMacSystemFont, 'Segoe UI', 'Noto Sans', Helvetica, Arial, sans-serif"
    fontSize: "0.75rem"
    fontWeight: 500
    lineHeight: 1
  mono:
    fontFamily: "Monaspace Neon, ui-monospace, SFMono-Regular, 'SF Mono', Menlo, Consolas, monospace"
    fontSize: "0.8125rem"
    fontWeight: 500
    lineHeight: 1.3
  hud-title:
    fontFamily: "Mona Sans, -apple-system, BlinkMacSystemFont, 'Segoe UI', 'Noto Sans', Helvetica, Arial, sans-serif"
    fontSize: "1.25rem"
    fontWeight: 600
    lineHeight: 1.2
  hud-body:
    fontFamily: "Mona Sans, -apple-system, BlinkMacSystemFont, 'Segoe UI', 'Noto Sans', Helvetica, Arial, sans-serif"
    fontSize: "0.9375rem"
    fontWeight: 400
    lineHeight: 1.5
rounded:
  xs: "4px"
  control: "6px"
  sm: "6px"
  md: "6px"
  lg: "8px"
  xl: "12px"
  2xl: "12px"
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
    backgroundColor: "{colors.accent-emphasis}"
    textColor: "{colors.on-accent}"
    rounded: "{rounded.control}"
    padding: "0 12px"
    height: "32px"
    typography: "{typography.body}"
  button-secondary:
    backgroundColor: "{colors.button-bg}"
    textColor: "{colors.text}"
    rounded: "{rounded.control}"
    padding: "0 12px"
    height: "32px"
    typography: "{typography.body}"
  button-ghost:
    backgroundColor: "transparent"
    textColor: "{colors.text}"
    rounded: "{rounded.control}"
    padding: "0 12px"
    height: "32px"
    typography: "{typography.body}"
  badge:
    backgroundColor: "transparent"
    textColor: "{colors.muted}"
    rounded: "{rounded.pill}"
    padding: "0 7px"
    height: "20px"
    typography: "{typography.label}"
  box:
    backgroundColor: "{colors.surface}"
    textColor: "{colors.text}"
    rounded: "{rounded.xl}"
    padding: "16px"
  input:
    backgroundColor: "{colors.bg}"
    textColor: "{colors.text}"
    rounded: "{rounded.control}"
    padding: "0 12px"
    height: "32px"
    typography: "{typography.body}"
  camera-stage:
    backgroundColor: "{colors.stage-bg}"
    textColor: "{colors.stage-text}"
    rounded: "{rounded.lg}"
  hud-capsule:
    backgroundColor: "{colors.surface-elevated}"
    textColor: "{colors.text}"
    rounded: "{rounded.xl}"
    padding: "14px 16px"
    width: "380px"
---

# Design System: Flick

## Overview

**Creative North Star: "Primer Operations Console"**

Flick's shipped interface is a GitHub-grade operations console for the home. The chrome is calm, flat, and Primer-native; the live camera stage is the only area allowed to feel vivid because it is the evidence surface. The app refuses consumer smart-home tiles, decorative gradients, blur, and Liquid Glass.

The system is professional before it is expressive: Mona Sans, Monaspace Neon, GitHub's six selectable themes, opaque Boxes, divided lists, one action blue, and semantic status colors. Screens tell the same story every time: status at a glance, what Flick sees and did, then one primary action.

**Key Characteristics:**
- Mona Sans for interface text and Monaspace Neon for entity IDs, shortcuts, and diagnostics.
- Six GitHub themes selected by `data-theme`, with independent light/dark choices and system sync.
- 1px borders, flat opaque fills, 6px controls, 12px Boxes, and no nested-card stacks.
- Accent-emphasis blue is the primary action color; semantic state colors are text-redundant.
- The camera preview stage stays dark in every theme so video and overlays read consistently.

## Colors

Flick uses GitHub Primer roles, not a brand gradient palette. Each theme remaps the same semantic tokens through `[data-theme]`; components consume the tokens, not hard-coded theme values.

### Primary
- **Accent Emphasis** (`accent-emphasis`): the only primary action fill, used for primary buttons, active tabs, switch-on tracks, progress bars, selected device rays, and active choice outlines. It is intentionally blue rather than GitHub green.
- **Accent Blue** (`accent-blue`): link text, low-emphasis selected states, badge text, and informational affordances.
- **On Accent** (`on-accent`): foreground for emphasis fills. `#ffffff` on primary emphasis fills and HUD success/danger icons is intentional in the shipped light/dark themes; do not "soften" it to a muted neutral.

### Semantic
- **Success** (`success-emphasis`, `success-muted`): online, completed, and OK states. The default themes render success as green; the light colorblind theme maps success to blue.
- **Attention** (`attention-emphasis`, `attention-muted`): waiting, update-ready, paused, and coaching states.
- **Danger** (`danger-emphasis`, `danger-muted`): failures, destructive actions, blocked camera states, and risky-device copy. The light colorblind theme maps danger to orange.
- **Done / Violet** (`done-muted`, `accent-violet`): available for completion-adjacent Primer roles, never as a competing primary action color.

### Neutral
- **Background / Sidebar / Surface** (`bg`, `sidebar`, `surface`): opaque theme fills. Light defaults are white and near-white; dark defaults are GitHub dark canvas values.
- **Surface Elevated** (`surface-elevated`): popovers, dialogs, sheets, toasts, command palette, and HUD capsule.
- **Text / Muted / Border** (`text`, `muted`, `border`, `border-muted`, `border-strong`): all structure is carried by readable text and 1px dividers.
- **Stage** (`stage-bg`, `stage-text`, `stage-muted`): the live camera preview is always the same dark stage, independent of the active theme.

### Named Rules
**The Blue Is the Action Rule.** Primary actions use `accent-emphasis`; do not switch primary buttons to success green or introduce a second brand accent.

**The Camera Owns Vividness Rule.** Keep surrounding chrome quiet and tokenized; visual energy belongs to the live preview, state overlays, and semantic feedback.

**The Colorblind Semantics Rule.** State must be carried by label, icon, and placement as well as color; colorblind themes deliberately remap success and danger.

## Typography

**Display Font:** Mona Sans with the system and Segoe/Noto/Helvetica fallbacks.
**Body Font:** Mona Sans using the same stack.
**Label/Mono Font:** Monaspace Neon for code-like values, Home Assistant entity IDs, keyboard shortcuts, and diagnostics only.
**Accessibility Font:** Atkinson Hyperlegible is bundled for the optional Hyperlegible font setting and should not replace Mona Sans by default.

**Character:** Compact, plain-spoken, and operational. Weight carries hierarchy more than large type; body copy stays at app density, and labels remain readable without shouting.

### Hierarchy
- **Display** (600, 2rem, 1.25): rare high-level framing; most screens should not need it.
- **Headline** (600, 1.75rem, 1.25): page titles and first-viewport headings.
- **Title** (600, 1rem, 1.5): Box headings, section headings, row titles, and panel titles.
- **Body** (400, 0.875rem, 1.5): operational copy, table rows, settings descriptions, and buttons.
- **Label** (500, 0.75rem, 1): badges, metadata, keyboard hints, captions, and compact status labels.
- **HUD Title / Body** (600 1.25rem / 400 0.9375rem): desktop overlay feedback, tuned for glanceability without becoming a consumer toast.
- **Mono** (500, 0.8125rem, 1.3): identifiers, version strings, entity IDs, diagnostics, and shortcuts only.

### Named Rules
**The Mona-First Rule.** Mona Sans is the product voice; do not fall back to Inter, Geist, or decorative display fonts for new surfaces.

**The Mono Means Machine Rule.** Monaspace Neon is reserved for values that behave like identifiers or shortcuts, never for headings or flavor.

## Layout

The app shell is a two-column operations workspace: a 15rem sidebar, a 3rem flat header bar, and a scrolling content column capped at 90rem. The first viewport puts the title and one-sentence context above a preview Box that takes roughly two thirds of the working width, with status, updates, activity, or quick actions in the side column.

Boxes are the primary layout primitive. A Box is one opaque surface with a 1px border, 12px radius, and internal divided rows. Prefer one Box with sections over multiple nested cards. Settings use a sticky 12rem section nav beside a 52rem content column; activity and studio routes use two-column operate layouts that collapse to one column.

At 980px and below the sidebar disappears and the toolbar adapts for mobile menu access. At 720px and below the app gains fixed bottom tabs, stacked content, full-width controls, and static preview overlays. Mobile keeps the same Primer chrome instead of becoming a separate consumer dashboard.

### Named Rules
**The Box-Then-Row Rule.** Use one bordered Box with divided rows before creating new cards; nesting cards inside cards is off-world.

**The First Viewport Rule.** Lead with status, then the live preview, then one primary action; do not open with marketing tiles or feature cards.

## Elevation & Depth

Flick is flat by default. Depth is conveyed through opaque tonal surfaces, 1px borders, divided rows, sticky chrome, and semantic fills. Shadows exist only for floating layers such as select menus, popovers, tooltips, dialogs, sheets, command palette, toasts, skip link, and HUD; ordinary Boxes, panels, cards, and previews rest flat.

### Shadow Vocabulary
- **Small Control Shadow** (`--flick-shadow-small`): subtle button/input control relief in themes that need it; dark themes may collapse it to transparent.
- **Overlay Shadow** (`--flick-shadow-overlay`): the only elevated shadow vocabulary for floating layers.
- **Preview Depth** (`none`): the camera preview earns focus through the dark stage and content, not through drop shadows.

### Named Rules
**The Flat Until Floating Rule.** If a surface is part of the page, it is flat; if it floats above the page, it may use the overlay shadow.

## Shapes

Flick uses Primer-scale corners. Small details use 4px, controls use 6px, camera stages and compact cards use 8px, and Boxes, dialogs, sheets, HUD capsules, and theme swatches use 12px. Pills are reserved for badges, switches, slider tracks, camera labels, gesture chips, and progress bars.

Borders are normally 1px. High-contrast themes strengthen border color rather than increasing visual ornament. The room sketch is a special case: it uses two `linear-gradient` layers as a CSS grid-line blueprint pattern, not as a decorative gradient treatment.

### Named Rules
**The Six And Twelve Rule.** Controls are 6px; Boxes are 12px. Do not bring back 24px+ liquid rounding.

## Components

### Shell and Navigation
Sidebar links are 32px rows with 6px radius, quiet hover fills, active neutral-muted fill, and optional keyboard hints. The header is a 48px flat bar with engine/HA status and a command-search control. Mobile replaces the sidebar with bottom tabs and a sheet nav.

### Boxes and Divided Lists
Boxes use `surface`, 1px `border`, 12px radius, and 16px padding. Rows divide with `border-muted`, use 10-12px vertical rhythm, and keep title, helper text, metadata, and trailing actions in one scan line when space allows.

### Buttons
- **Shape:** 32px default height, 28px small, 40px large, 6px radius, 1px border.
- **Primary:** `accent-emphasis` fill, `on-accent` text, transparent border, no gradient.
- **Secondary:** `button-bg` fill, `button-border`, small control shadow, hover/active through button tokens.
- **Ghost:** transparent rest state, neutral-muted hover, no shadow.
- **Danger:** text-only danger by default; hover/active become danger-emphasis with white text.
- **Focus:** all variants rely on the global 2px `focus` outline.

### Badges, Keycaps, and Chips
Badges are 20px pills with 1px borders and 12px text. Accent, success, warning, and danger badges color their text and border. Keycaps use Monaspace Neon, 4px radius, bottom border emphasis, and subtle background.

### Inputs, Selects, Segmented Controls, Switches, and Sliders
Inputs and selects are 32px controls with 6px radius, 1px borders, opaque backgrounds, and inset neutral relief. Focus moves both border and outline to `focus`. Segmented controls sit inside a neutral-muted track with a 2px inset; checked items are opaque. Switch thumbs and slider thumbs are intentionally white against emphasis or neutral tracks.

### Floating Layers and Command Palette
Select menus, popovers, tooltips, dialogs, sheets, toasts, and command palette use `surface-elevated`, 12px radius for larger layers, 6px radius for tooltips/items, and the overlay shadow. Command rows are transparent until active, then use neutral-muted.

### Camera Preview and HUD
The preview stage is always dark, with stage text and muted stage text for placeholders and overlays. Video uses `object-fit: contain`; overlays on the stage may use a dark translucent plate because they sit on video, not page chrome. The HUD is a desktop overlay Box with state-colored icon blocks, progress bars, and redundant text for aiming, candidate, selected, armed, done, failed, camera moved, confirm, ambiguous, and paused states.

### Theme Picker
Theme swatches are miniature two-column app previews using the same tokens as the app shell. Selected swatches use accent-emphasis border and a 1px accent ring; the radio uses the same accent color.

### Room Sketch Blueprint
The room sketch is a functional placement diagram: subtle grid lines, device rays, and labels use theme tokens. Its `linear-gradient` background is a grid-line technique and remains allowed even though decorative gradients are banned.

## Do's and Don'ts

### Do:
- **Do** build new screens from Primer Boxes, divided rows, one primary action, and semantic status labels.
- **Do** consume `--flick-*` tokens so all six GitHub themes and system sync keep working.
- **Do** keep the camera stage dark across every theme.
- **Do** use accent-emphasis blue for primary actions and focus, not success green.
- **Do** pair status color with text, icon, tone, or placement for accessibility.
- **Do** preserve reduced-motion behavior by shortening animation to static state changes.

### Don't:
- **Don't** reintroduce Liquid Glass, backdrop blur, translucent panels, decorative gradients, or oversized rounded corners.
- **Don't** use consumer smart-home tiles, nested card decks, or marketing feature grids inside the app shell.
- **Don't** replace Mona Sans / Monaspace Neon with Inter, Geist, SF-only, or novelty fonts.
- **Don't** make ordinary page Boxes float with shadows; reserve overlay shadow for actual floating layers.
- **Don't** ban the room-sketch grid-line `linear-gradient`; it is a functional blueprint pattern, not decoration.
