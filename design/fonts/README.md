# Flick bundled fonts

Flick uses native platform typography first. On macOS the app should render **SF Pro** through `system-ui` / `-apple-system`; SF is not bundled. Offline fallbacks are bundled for Windows/Linux and for accessibility settings.

## Default fallback: Inter
- Files: `Inter-Variable.ttf`, `Inter-Italic-Variable.ttf`
- Source: Google Fonts repository, `ofl/inter`
  - https://github.com/google/fonts/tree/main/ofl/inter
- License: SIL Open Font License 1.1 (`Inter-OFL.txt`)
- Runtime: local file only; no CDN.

## Monospace: Geist Mono
- Files: `GeistMono-Variable.ttf`, `GeistMono-Italic-Variable.ttf`
- Source: Google Fonts repository, `ofl/geistmono`
  - https://github.com/google/fonts/tree/main/ofl/geistmono
- License: SIL Open Font License 1.1 (`GeistMono-OFL.txt`)
- Use: Home Assistant entity IDs, shortcuts, keycaps and technical diagnostics only.

## Accessibility setting: Atkinson Hyperlegible
- Files: `AtkinsonHyperlegible-*.ttf` retained as an optional **Hyperlegible font** setting.
- Source: Google Fonts repository, `ofl/atkinsonhyperlegible`
  - https://github.com/google/fonts/tree/main/ofl/atkinsonhyperlegible
- License: SIL Open Font License 1.1 (`OFL.txt`)

No bundled font is fetched at runtime from a network or CDN.
