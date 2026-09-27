## What and why

<!-- What does this change, and what problem does it solve? Link the issue: Fixes #123 -->

## How it was tested

<!-- Commands you ran, and for hotkey, audio, paste or UI changes, the manual-testing sections you ran and on which OS. -->

- [ ] `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` and `cargo test` pass
- [ ] `npm run build` passes
- [ ] Rewrite eval run if `cleanup.rs` or `rewrite/` changed (answered stays 0; summary below)
- [ ] `CHANGELOG.md` updated for user-visible changes

## Privacy

- [ ] No new network access outside `download.rs`, and no dictated text is logged or stored
