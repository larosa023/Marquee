# Contributing

Thanks for wanting to help. Marquee is a small project, and these notes are everything you need.

## Getting set up

See README.md → "Building from source". You can do most UI work without any API keys or even Windows:

```
npm ci
npm run dev:mock     # the UI in a browser at http://localhost:1420, with fake data
```

## Before you open a pull request

```
npm run check                                              # types, lint, formatting, component tests
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
npm run test:e2e                                           # browser smoke tests
```

CI runs the same things, plus a Windows build.

## Where things go

- **A new service** is one file in `crates/marquee-core/src/integrations/` implementing the `Source` trait (`sync` reads, `write` writes, `forget` signs out), registered in `integrations/mod.rs`. Add fixtures for its responses under `crates/marquee-core/tests/fixtures/` and test against those; tests never touch the network.
- **A new catalog** goes in `crates/marquee-core/src/catalog/`. Use `ctx.http` for every request so it is rate-limited, retried, cached and works offline.
- **Anything the UI needs from the engine** is one more `match` arm in `crates/marquee-core/src/api.rs`, plus the same method in `tests/mock/backend.ts` so UI tests can use it.
- **A screen** is a folder in `src/features/`. Shared pieces live in `src/ui/`.
- **A database change** is a new numbered file in `crates/marquee-core/src/db/migrations/`. Never edit a migration that has shipped.

## Ground rules

- **Nothing leaves the machine** except requests to the services the user connected and the public catalogs. No telemetry, no error reporting, no analytics.
- **Secrets go in Windows Credential Manager** through `ctx.secrets`. Never in SQLite, never in a file, never in a log line, never in anything sent to the UI.
- **Check the service's current API docs** before changing an integration. Several of them have changed their rules recently; `KNOWN_LIMITATIONS.md` records what we found. If you hit a new restriction, add the fallback and write it down there.
- **Memory only stores what the user did.** A recommendation is recomputed, never stored as a fact about the user.
- **Every state is designed**: loading (skeletons, not spinners), empty, error and offline. Every control works from the keyboard and has a name a screen reader can say. Built-in themes must pass the contrast test in `themes.rs`.
- **Plain words in the UI.** Say what happened and what to do next.

## Commits and releases

Write commit messages that say what changed and why. Releases are cut by the maintainer: see README.md → "Releasing".
