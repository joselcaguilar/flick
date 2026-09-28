# Flick bundled fonts

Flick matches the GitHub Copilot app: **Mona Sans** for interface text and **Monaspace Neon** for code-like values. Both are bundled and loaded from local files; nothing is fetched from a network or CDN at runtime.

## Interface: Mona Sans (variable weight)
- Files: `MonaSans-Latin-Variable.woff2`, `MonaSans-Latin-Italic-Variable.woff2`, `MonaSans-LatinExt-Variable.woff2`, `MonaSans-LatinExt-Italic-Variable.woff2`
- Source: https://github.com/github/mona-sans (Fontsource `@fontsource-variable/mona-sans` 5.3.0, `wght` axis subsets)
- License: SIL Open Font License 1.1 (`MonaSans-OFL.txt`)

## Code and identifiers: Monaspace Neon
- Files: `MonaspaceNeon-Latin-400.woff2`, `-500`, `-600`
- Source: https://github.com/githubnext/monaspace (Fontsource `@fontsource/monaspace-neon` 5.3.0)
- License: SIL Open Font License 1.1 (`MonaspaceNeon-OFL.txt`)
- Use: Home Assistant entity IDs, keyboard shortcuts and diagnostics only.

## Accessibility setting: Atkinson Hyperlegible
- Files: `AtkinsonHyperlegible-*.ttf`, kept for an optional **Hyperlegible font** setting.
- Source: https://github.com/google/fonts/tree/main/ofl/atkinsonhyperlegible
- License: SIL Open Font License 1.1 (`OFL.txt`)
