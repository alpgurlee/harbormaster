# Contributing

Bug reports, fixes and new cleaner locations are welcome. For a larger change, open an issue first so
we can agree on the approach before you spend time on it. Security problems go through
[SECURITY.md](SECURITY.md), not public issues.

## Layout

- `crates/platform`: ports, processes and the files they hold, as the kernel tells them, and signals.
- `crates/engine`: dev servers from those facts: grouping, frameworks, projects, and the checks a stop
  runs before each signal.
- `crates/cleaner`: what a scan finds, how much room it takes, the space map, and removal.
- `crates/cli`: the `harbormaster` command.
- `app/src-tauri` and `app/src`: the menu bar app, Tauri in Rust and the windows in React.

## Before you open a pull request

Set up as in the README's Building section, then run what CI runs, from the repository's root:

```sh
(cd app && npm ci --ignore-scripts && npm audit --audit-level=moderate &&
  npx tsc --noEmit && npx vitest run && npx vite build)
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

Some tests start real processes, listen on ports and build small programs with `cc`, so run them on a
Mac, not in a sandbox that forbids those.

- A behaviour change comes with a test that fails without it.
- Anything that ends a process or removes a file keeps its checks: show first, check again right before
  acting, refuse when unsure.
- A new cleaner location needs a source, such as the tool's own documentation, for the path and for
  the fact that the tool rebuilds what is there. Put anything that may be the user's own work in the
  Trash instead of deleting it.
- Dependencies are pinned to exact versions, and lock files are committed.
- Commit messages follow [Conventional Commits](https://www.conventionalcommits.org) in English, one
  logical change per commit.
- New text in the windows needs both English and Turkish, in `app/src/messages.ts`.
