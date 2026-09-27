# Contributing to Diktator

Thanks for helping. Bug reports, fixes, documentation and ideas are all welcome.

## Ways to contribute

- **Report a bug.** Open an issue with the bug template. Include your OS version, CPU, the models you use, and the `dictation: …` log line if the problem is about a dictation. Never paste private dictations into an issue.
- **Suggest a feature.** Open an issue with the feature template and describe the problem before the solution.
- **Improve the docs.** Small fixes can go straight to a pull request.
- **Send code.** For anything larger than a small fix, open an issue first so we can agree on the approach before you spend time on it.

Security problems should not go in public issues; see [SECURITY.md](SECURITY.md).

## Development setup

See [docs/development.md](docs/development.md) for prerequisites, running the app, tests and benchmarks, and [docs/architecture.md](docs/architecture.md) for how the pieces fit together.

## Pull requests

1. Fork the repository and create a branch from `main`.
2. Keep the change focused. One pull request should do one thing.
3. Add or update tests for logic you change. Pure logic (parsers, state machines, cleanup rules) should have unit tests.
4. Before pushing, run:

   ```bash
   cd src-tauri && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
   cd .. && npm run build
   ```

5. If you changed `cleanup.rs` or anything in `rewrite/`, run the rewrite eval (see [docs/benchmarks.md](docs/benchmarks.md)) and put the summary in the pull request. The answered count must stay 0.
6. If you changed hotkeys, audio, pasting or the UI, work through the relevant sections of [docs/manual-testing.md](docs/manual-testing.md) and say which ones you ran, and on which OS.
7. Add a line to the `Unreleased` section of [CHANGELOG.md](CHANGELOG.md) for user-visible changes.

## Commit messages

Use [Conventional Commits](https://www.conventionalcommits.org): `feat:`, `fix:`, `perf:`, `refactor:`, `docs:`, `test:`, `chore:`, with an optional scope, for example `fix(insert): keep the clipboard when the paste fails`.

## Ground rules

- **Privacy is a hard requirement.** No new network access outside `src-tauri/src/download.rs`, no telemetry, and never log or store dictated text. Pull requests that break this won't be merged.
- **Licenses.** Contributions are made under the project's [MIT License](LICENSE). Only add dependencies or models with licenses compatible with MIT distribution, and list new models in [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).
- **Don't copy code** from projects whose license you can't honor in this repository.

## Code of conduct

This project follows the [Contributor Covenant](CODE_OF_CONDUCT.md). By taking part, you agree to uphold it.
