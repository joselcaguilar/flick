# Flick UI

## Commands

- `pnpm dev` runs Vite against a real engine endpoint (`127.0.0.1:7871`, `dev-token` by default).
- `pnpm dev:mock` runs the same app with MSW REST handlers and the mock WS sequence.
- `pnpm gen:api` regenerates `src/api/schema.d.ts` from `../crates/flick-api/openapi.json`; pass `OPENAPI_PATH=/absolute/openapi.json` for another checkout.
- `pnpm lint`, `pnpm typecheck`, `pnpm test`, `pnpm build`, and `pnpm detect` are the CI gates.

## Folder conventions

- `src/routes/` owns route metadata and route stubs. Add a screen by adding a `RouteMeta` item in `routes.tsx`; navigation, mobile tabs, and the command palette are generated from it.
- `src/features/<feature>/` owns non-trivial feature logic and the only allowed unit tests: WS reducer, sentence-builder validation, and dial math.
- `src/api/` owns generated OpenAPI types, endpoint resolution, the fetch client, and TanStack Query hooks.
- `src/events/` owns the WebSocket client, normalization, Zustand store, and reducer.
- `src/mocks/` owns MSW REST handlers, fixtures, and the mock WS sequence. Add a handler whenever a generated route is consumed.
- `src/components/ui/` is the re-skinned primitive kit. `src/components/domain/` contains Flick-specific building blocks such as gesture glyphs and the preview overlay.
- `src/i18n/en.json` stores English strings by route/domain (`nav.*`, `routes.*`, `empty.*`, `hud.*`). Add keys before hard-coding repeated copy.

## Adding a screen

1. Add or update a route in `src/routes/routes.tsx` with title, description, shortcut, and empty/loading/error behavior.
2. Put feature-specific state, parsing, math, or validators under `src/features/<feature>/`.
3. Add API hooks in `src/api/hooks.ts` and MSW fixtures/handlers in `src/mocks/`.
4. Use primitives from `components/ui` and domain components from `components/domain`; do not import Radix directly from screens.
5. Verify with `pnpm lint && pnpm typecheck && pnpm test && pnpm build && pnpm detect`.
