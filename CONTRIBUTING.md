# Contributing to Flick

- Use Conventional Commits, for example `feat(core): add gesture ids`.
- Sign every commit with DCO (`git commit -s`).
- Branches are named `task/<ID>-<short-name>` and map to `docs/spec/08-roadmap-and-work-breakdown.md`.
- Follow the lean testing policy in `docs/spec/07-quality-security-licensing.md` §1.1: test hard logic, costly failures and cross-boundary contracts only; do not add coverage-driven or trivial tests.
- UI work follows the Impeccable definition of done from `docs/spec/04-gesture-studio-and-ux.md` §0: use `PRODUCT.md` and `DESIGN.md`, re-skin components, run critique/audit/detector, and provide required screenshots.
- Never commit secrets, Home Assistant tokens, RTSP credentials, footage without consent, model binaries or generated dependency caches.
- `pnpm-lock.yaml` must resolve from the public npm registry: CI installs with `--frozen-lockfile` from registry.npmjs.org and rejects tarball URLs from private registries or mirrors.
