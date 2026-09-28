---
name: Flick
description: Raycast-like local gesture control for Home Assistant, rendered in dark-first Liquid Glass.
colors:
  dark-bg: "#090B10"
  dark-bg-elevated: "#11141C"
  dark-glass: "rgba(22, 25, 34, 0.62)"
  dark-glass-strong: "rgba(26, 30, 42, 0.78)"
  dark-text: "#F6F8FF"
  dark-muted: "#AEB7CA"
  dark-border: "rgba(255, 255, 255, 0.14)"
  light-bg: "#F5F7FB"
  light-bg-elevated: "#FFFFFF"
  light-glass: "rgba(255, 255, 255, 0.68)"
  light-glass-strong: "rgba(255, 255, 255, 0.86)"
  light-text: "#10131A"
  light-muted: "#5B6475"
  light-border: "rgba(17, 24, 39, 0.12)"
  accent-blue: "#006DFF"
  accent-violet: "#6D5DF7"
  accent-mint: "#4EE6B8"
  accent-amber: "#FFB84D"
  accent-red: "#FF5D6C"
  hc-bg: "#000000"
  hc-text: "#FFFFFF"
  hc-focus: "#FFFF00"
typography:
  display:
    fontFamily: "system-ui, -apple-system, BlinkMacSystemFont, 'Inter', 'Segoe UI', sans-serif"
    fontSize: "clamp(2.5rem, 5vw, 4.75rem)"
    fontWeight: 700
    lineHeight: 0.94
    letterSpacing: "-0.06em"
  headline:
    fontFamily: "system-ui, -apple-system, BlinkMacSystemFont, 'Inter', 'Segoe UI', sans-serif"
    fontSize: "clamp(1.75rem, 3vw, 3rem)"
    fontWeight: 700
    lineHeight: 1
    letterSpacing: "-0.045em"
  title:
    fontFamily: "system-ui, -apple-system, BlinkMacSystemFont, 'Inter', 'Segoe UI', sans-serif"
    fontSize: "1.125rem"
    fontWeight: 650
    lineHeight: 1.18
    letterSpacing: "-0.02em"
  body:
    fontFamily: "system-ui, -apple-system, BlinkMacSystemFont, 'Inter', 'Segoe UI', sans-serif"
    fontSize: "1rem"
    fontWeight: 450
    lineHeight: 1.5
  label:
    fontFamily: "system-ui, -apple-system, BlinkMacSystemFont, 'Inter', 'Segoe UI', sans-serif"
    fontSize: "0.8125rem"
    fontWeight: 650
    lineHeight: 1.2
    letterSpacing: "0.01em"
  small:
    fontFamily: "system-ui, -apple-system, BlinkMacSystemFont, 'Inter', 'Segoe UI', sans-serif"
    fontSize: "0.875rem"
    fontWeight: 500
    lineHeight: 1.35
  hud-title:
    fontFamily: "system-ui, -apple-system, BlinkMacSystemFont, 'Inter', 'Segoe UI', sans-serif"
    fontSize: "1.375rem"
    fontWeight: 700
    lineHeight: 1.08
  hud-body:
    fontFamily: "system-ui, -apple-system, BlinkMacSystemFont, 'Inter', 'Segoe UI', sans-serif"
    fontSize: "1.25rem"
    fontWeight: 550
    lineHeight: 1.25
  mono:
    fontFamily: "ui-monospace, 'SF Mono', 'Geist Mono', 'Cascadia Mono', monospace"
    fontSize: "0.8125rem"
    fontWeight: 550
    lineHeight: 1.3
rounded:
  xs: "8px"
  sm: "12px"
  md: "18px"
  lg: "24px"
  xl: "32px"
  xxl: "40px"
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
    backgroundColor: "{colors.accent-blue}"
    textColor: "{colors.hc-text}"
    rounded: "{rounded.pill}"
    padding: "10px 16px"
    typography: "{typography.label}"
  button-ghost:
    backgroundColor: "{colors.dark-glass}"
    textColor: "{colors.dark-text}"
    rounded: "{rounded.pill}"
    padding: "10px 14px"
    typography: "{typography.label}"
  glass-panel:
    backgroundColor: "{colors.dark-glass}"
    textColor: "{colors.dark-text}"
    rounded: "{rounded.xl}"
    padding: "20px"
  hud-glass:
    backgroundColor: "{colors.dark-glass-strong}"
    textColor: "{colors.dark-text}"
    rounded: "{rounded.xxl}"
    width: "380px"
    height: "104px"
---

# Design System: Flick

## Overview

**Creative North Star: "Liquid Command Remote"**

Flick now feels like a Raycast-class pro utility for the home: fast, keyboard-first, compact, dark-first and unmistakably modern. The operating surface is a translucent macOS-style command deck where the live camera preview is the hero, device status floats in layered glass, and every action has a visible shortcut path.

The visual language follows a macOS 27 Liquid Glass interpretation: translucent materials, saturated backdrop blur, specular edge highlights, floating sidebars/toolbars, concentric rounded corners and a glass HUD capsule. The old cue-sheet/grid look is an anti-reference. Keep the product truth and copy, but remove anything that resembles 1990s/2000s web chrome, flat boxy cards, neon gamer HUDs or stock shadcn.

**Key Characteristics:**
- Dark-first glass workspace with equally polished light mode and system/light/dark toggles.
- Command palette and visible shortcuts make Flick feel like a pro desktop tool, not a smart-home dashboard.
- Live preview is the largest element; status floats around it as glass, not card grids.
- Hairline alpha borders, inner highlights and subtle gradients provide depth.
- HUD is a desktop-only floating glass capsule with opaque high-contrast and reduced-transparency fallbacks.

## Colors

The palette is cool, crisp and low-glare: near-black and platinum surfaces, blue/violet selection energy, mint success, amber attention and red failure. Saturated color is reserved for live state and commands.

### Primary
- **Command Blue**: selected device rings, primary buttons, focus halos and active command rows.
- **Violet Beam**: secondary gradient stop for raycast effects and premium polish; never used for status alone.

### Secondary
- **Mint Done**: successful Home Assistant confirmation and check states.
- **Amber Attention**: update-ready banners, coaching and waiting states.
- **Red Failure**: unavailable, failed or blocked states, always paired with ✕ and recovery text.

### Neutral
- **Dark Workspace / Dark Glass**: default environment for macOS desktop use.
- **Light Workspace / Light Glass**: first-class light theme, not an inverted afterthought.
- **Hairline Border**: alpha white/dark borders for liquid material separation.
- **High-Contrast Black / White / Focus Yellow**: HUD and accessibility mode; text pairs exceed 7:1.

### Named Rules
**The Glass Has a Job Rule.** Glass appears for persistent navigation, preview overlays, command surfaces and HUD only. If a panel does not float above content, use an opaque surface.

**The Blue Means Control Rule.** Blue/violet marks selection, focus and user command; success/warning/failure keep their own semantic colors and redundant text.

## Typography

**Display Font:** `system-ui, -apple-system` first so macOS renders SF Pro natively. Inter is the bundled OFL fallback for Windows/Linux.
**Body Font:** same native-first sans stack.
**Label/Mono Font:** `ui-monospace` / SF Mono first with bundled Geist Mono fallback; use only for entity IDs, shortcuts and diagnostic values.
**Accessibility Font:** Atkinson Hyperlegible remains bundled only for a future "Hyperlegible font" preference.

**Character:** Sharp, native, compact and airy. Use tight tracking for big headings and normal tracking for operational copy. Avoid decorative type and monospaced "tech" styling outside real identifiers and shortcuts.

### Hierarchy
- **Display** (700, clamp 2.5rem-4.75rem, 0.94): rare product-level statements and prototype framing.
- **Headline** (700, clamp 1.75rem-3rem, 1): screen and flow titles.
- **Title** (650, 1.125rem, 1.18): glass panel headings and command names.
- **Body** (450, 1rem, 1.5): explanatory copy and status details.
- **Label** (650, 0.8125rem, 1.2): nav labels, chips, toolbar controls.
- **Small** (500, 0.875rem, 1.35): secondary rows and helper text.
- **HUD Title / Body** (1.375rem / 1.25rem): desktop HUD text; never below 20 px.
- **Mono** (0.8125rem): entity IDs and keyboard shortcuts only.

### Named Rules
**The Native First Rule.** Never bundle SF; rely on the platform stack, with Inter and Geist Mono as local fallbacks.

**The Shortcut Is Visible Rule.** Primary desktop actions show their shortcut or command-palette equivalent when space allows.

## Layout

Desktop uses a floating glass sidebar and toolbar inside a spacious stage. The live preview is the hero, with status glass sitting around it and recent activity docked as a translucent rail. The sidebar floats rather than touching the viewport edge; toolbar controls float above the content with concentric rounding.

Tablet collapses the sidebar into a compact rail/menu while preserving the hero preview first. Phone (360 px and up) switches to a bottom tab bar and sheet-like panels; touch targets are at least 44 px. The HUD is desktop-only and omitted from mobile layout because it represents an always-on-top desktop overlay.

Command palette layout is centered, glassy and keyboard-first: search field, grouped commands, visible shortcuts and a selected row.

## Elevation & Depth

Depth comes from layered liquid materials: backdrop blur, saturation, hairline alpha borders, inner specular highlights and soft shadows. Every glass surface has an opaque fallback for `prefers-reduced-transparency` and for browsers without `backdrop-filter`.

### Shadow Vocabulary
- **Glass Float** (`0 24px 80px rgba(0,0,0,.34), inset 0 1px rgba(255,255,255,.18)`): sidebar, toolbar, command palette and HUD.
- **Preview Depth** (`0 32px 120px rgba(0,0,0,.38)`): live preview hero.
- **Light Float** (`0 24px 70px rgba(31,41,55,.16), inset 0 1px rgba(255,255,255,.82)`): light-theme glass.

### Named Rules
**The Opaque Fallback Rule.** Any use of blur/saturation must have an opaque background in `@supports not (backdrop-filter)` and `prefers-reduced-transparency`.

## Shapes

Shapes are concentric and modern. App windows use 36-40 px outer corners; inner glass panes step down to 28/24/18 px; controls and chips use pills. Preview overlays and HUD capsules use larger radii than their internal controls so the hierarchy feels nested and native.

Gesture glyphs and domain icons are SF Symbols-like: 24×24 viewBox, 1.5-2 px rounded strokes, no filled emoji assets, no mixed icon packs.

## Components

### shadcn/ui Re-skinning
Use shadcn/ui for behavior and accessibility only. Replace the stock theme with Flick tokens: native-first type, glass/opaque surfaces, concentric radii, alpha hairline borders, command-blue focus rings and platform-style controls. Stock slate/zinc variables, default card grids and default shadcn buttons are prohibited.

### Buttons
- **Shape:** compact pill controls, 44 px minimum target on touch.
- **Primary:** blue/violet gradient with white text and subtle inner highlight.
- **Ghost:** transparent glass with hairline border and hover fill.
- **Focus:** 3 px command-blue or high-contrast yellow ring.
- **Shortcuts:** desktop buttons may show keycaps like `⌘K` using the mono stack.

### Glass Panels
Glass panels use a material token (`--flick-material-*`), a hairline border, inner highlight and shadow. They must retain contrast over both light and dark wallpapers. Opaque fallbacks use `--flick-surface` without blur.

### Command Palette
Palette opens with `⌘K`, contains a real search field, selected row, grouped commands and visible shortcuts. It is a first-class component, not a modal afterthought.

### Preview Hero
The live preview owns the dashboard and Teach screens. Skeleton overlay, selected-device ring and ray are thin, bright and modern. Place status and update banners float as glass over or near the preview, never as flat cue-sheet cells.

### HUD
Desktop-only floating glass capsule. Selected state shows device, countdown and verb hints; Done and Failed show the outcome with ✓/✕ and text. High-contrast and reduced-transparency variants are opaque and maintain ≥ 7:1 text contrast.

### Motion & Reduce Motion
Motion is fast and native: 160-220 ms springs/ease-outs for panels, 500 ms dwell progress and a 4 s selected-device countdown. Under Reduce Motion, remove spring travel and continuous ring animation; preserve state with static bars and explicit text.

## Do's and Don'ts

### Do:
- **Do** make dark mode feel primary and light mode equally crafted.
- **Do** show `⌘K`, shortcuts and command-palette affordances on desktop.
- **Do** provide opaque fallbacks for glass materials.
- **Do** keep HUD text ≥ 20 px and state redundant through icon + text.
- **Do** use system-ui first, bundled Inter fallback, and Geist Mono only for technical/shortcut text.

### Don't:
- **Don't** revive the rejected Sightline Cue Desk, gridded cue boards or 1990s/2000s web references.
- **Don't** ship flat boxy cards, stock shadcn, Home Assistant mimicry or neon gamer HUDs.
- **Don't** bundle SF or load fonts/icons from a runtime CDN.
- **Don't** convey selected/done/failed by color alone.
- **Don't** let glass reduce accessibility; fall back to opaque surfaces when needed.
