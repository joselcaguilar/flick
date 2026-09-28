# Product

<!-- impeccable:product-schema 1 -->

## Platform

web

## Users
- **Home Assistant power users (primary).** They run HA on a Pi/NUC/VM, own a Mac (later a PC) and often Unifi Protect cameras. They want to control lights, fans and media from the couch or bed without a phone, and they tinker with setups on weekends.
- **Accessibility users (primary; the design driver).** People with limited mobility or speech, or who are Deaf/hard of hearing, for whom voice assistants fail. They need very reliable gestures that fit their own body, clear visual feedback and no false triggers.
- **Household members (secondary).** Non-technical people who use what the power user set up. It has to "just work", show clearly what happened, and be impossible to break.

## Product Purpose
Flick turns a camera the user already owns into a private, instant remote for Home Assistant. Users point at a real device (for example the bedroom ceiling fan) and make a gesture (draw a circle for speed 1, pull stacked hands apart for stop), or use a global gesture (thumbs-up toggles a light).

Success means:
- first successful gesture in under 3 minutes;
- new gestures and devices taught through the UI in about a minute;
- no false triggers in normal living-room activity.

## Positioning
Local-first, low-latency gesture control built specifically for Home Assistant:
- **Point-then-gesture:** the device is selected by pointing at the real object, not from a list.
- **Training in the UI:** users create gestures through the UI from a few takes, instead of collecting datasets.
- **Private:** video never leaves the user's machine, and only hand-landmark geometry is stored.

## Operating Context
- Runs on the user's Mac (macOS first; Windows/Linux later), on the same LAN as Home Assistant.
- Cameras: the MacBook/Continuity webcam in the same room first; Unifi Protect/RTSP cameras later.
- Used from a distance (0.5–3 m) while relaxing, often with the main window closed. Feedback comes from a small always-on-top HUD, sounds, and the device itself reacting.
- Setup happens in the main window: connecting HA, teaching devices by pointing, and recording gestures.

## Capabilities and Constraints
- **Recognition:**
  - Built-in gestures with zero training.
  - Custom static, motion and two-hand gestures recorded in the UI.
  - Pointing-based device selection with taught spatial anchors.
- **Home Assistant:** direct service calls over the HA WebSocket API only (no HA events, webhooks or add-on).
- **Updates:** the app, models and gesture catalog update over the air, signed.
- **Safety:** sensitive devices (locks, alarms, garage doors) are blocked unless the user explicitly allows them, and then need a confirmation gesture.
- **Cloud:** never in the recognition path. Optional cloud features must be opt-in.
- **Business model:** open-core (Apache-2.0 core plus paid Pro). The exact Core/Pro split and pricing are undecided.
- **Undecided:** final domain and bundle id; trademark clearance versus "Flic" (flic.io).

## Brand Commitments
- Name: **Flick**. The product speaks in short, human, jargon-free copy ("hand points", not "landmarks").
- No visual identity is committed yet. It will be established in DESIGN.md by a dedicated design task.

## Evidence on Hand
- Product and engineering spec: `docs/spec/` (README + 00–11).
- No users, testimonials, benchmarks, press or screenshots exist yet. Do not fabricate them.

## Product Principles
1. **Zero to first flick in minutes.** Every setup screen has one obvious next step.
2. **Always show what Flick sees and did:** live hand overlay, selected device, then sent → done/failed.
3. **Safe and calm by default.** Nothing fires by accident, and risky devices need explicit consent.
4. **Teach, don't configure.** Show Flick the gesture or the device instead of filling in forms.
5. **Private by construction.** Nothing leaves the machine unless the user opts in.

## Accessibility & Inclusion
- WCAG 2.2 AA for the main window.
- Fully keyboard-operable, with screen-reader labels.
- Respects Reduce Motion and contrast settings; sound and HUD feedback can be toggled independently.
- Gestures must be customizable to the user's body and range of motion (one hand, either hand, seated).
