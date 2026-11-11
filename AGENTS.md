# Apollo

Apollo is a fork of [herdr-app](https://github.com/herdrdev/herdr-app)
(Apache-2.0): the Herdr desktop app, a native GPUI client of
[herdr-core](https://github.com/herdrdev/herdr-core), renamed and with its
own releases. It lives at
[ForYouzi/apollo-app](https://github.com/ForYouzi/apollo-app), a GitHub
fork whose `main` builds on herdr-app's history. To take Herdr changes, add
the `upstream` remote (`git remote add upstream
https://github.com/herdrdev/herdr-app.git`) and merge `upstream/master` (or a
release tag) on a branch.

If AGENTS.local.md exists, read it too: the maintainer's local rules.

## What Apollo keeps from Herdr

The brand (name, bundle id, icon, executable) lives in `src/brand.rs`;
`scripts/brand-plist` renders the macOS Info.plist from
`packaging/Info.plist.in`. The macOS icon is `assets/icons/Apollo.icon` (Icon
Composer), compiled into the committed `assets/icons/Assets.car` by
`scripts/mac-icon`; its Dark look keeps the yellow. Rerun the script after
editing the icon. Internal herdr names stay on purpose: the
`herdr-app` command, the settings home `~/.config/herdr-app`, herdr-core,
and the `HERDR_*` environment variables, so Apollo shares a Herdr setup.

Apollo never updates itself (`src/update.rs`); new versions are on the
GitHub Releases page. Only herdr-core is checked and updated, and only in
release builds (`APOLLO_RELEASE` set at build time). Never set
`HERDR_APP_RELEASE`: it would turn on Herdr's telemetry and feedback
endpoints. The release workflow fails if a script sets it.

## Compatibility

Preserve compatibility with installed user data: Apollo reads and writes
the same `~/.config/herdr-app` as Herdr and talks to the same herdr-core.

## The app's core stays small

The app's core (the model, machine layout documents, commands, and shared
mod APIs) stays small; everything else is mods. Before adding to it, ask:

1. Does the app itself act on it (it changes what the app does, not only
   how something looks)?
2. Can only the app see it (e.g. a change that happens while no mod is
   watching)?
3. Can mods not do it correctly, even with care?

If all three are no, it belongs in mods.

`herdr-app --skill` prints the window manual in `src/SKILL.md`.
Keep it current when changing agent-facing behavior.

## The guide users' agents read

`defaults/AGENTS.md` is written into every user's `~/.config/herdr-app` on
every start: it's how their agents learn what the app can do. A change to
what users or agents can do (a command, a setting, a layout or mod
feature, a file) updates it in the same commit. `cargo test` checks that it
names every command and settings.json key; the rest is on you. The `.d.ts`
files are the full API.

## What ships in the box

`defaults/` is built into the app: mods, `layouts/*.json`, `keys.json`
(Linux, Windows) and `keys.macos.json` (Mac). Shipped layouts and mods are
replaced on start; edited versions are moved to personal copies. `keys.json`
is installed when missing; files matching an earlier shipped original
byte-for-byte are upgraded. Edited keys files are never replaced. Keep
release originals in `src/keys/shipped/` when changing defaults.

## Platform differences

Gate per OS only where the OS really behaves differently. A one-line
`cfg!(windows)` next to the quirk is fine, with a comment saying why; anything
bigger goes behind one small module per concern. Give the quirk a test on its
platform and keep the other platforms' behavior unchanged (and tested).

## Building and testing

Clone with submodules. Use Rust 1.95.0 and Zig 0.16.0 (the versions in
`.github/workflows/ci.yml`; set `ZIG` if `zig` is not on PATH).

- `just build`, `just test`, `just check`.
- In agent sessions run `cargo test --locked --bins` plus the non-screenshot
  `--test` targets. Never run `shot_pixels` (or a bare `cargo test`, which
  includes it) in an agent session: it opens windows.
- `bun test scripts/docs.test.ts` and `bun scripts/docs.ts check` check the
  docs; `just docs-generate` refreshes `docs/commands.md` from `src/commands.rs`.
- When dependencies or bundled assets change, run `just notices`.

Never launch the app in an agent session: no `cargo run`, no bare
`target/*/herdr-app`, no `open`. Subcommands and `--help`/`--version` only
talk; run them with `HERDR_APP_HOME` pointing at a scratch folder
(`HERDR_APP_HOME=$(mktemp -d) target/debug/herdr-app --version`). Never stop,
signal or restart the person's app or core.

`scripts/package` builds a release zip into `dist/` (macOS: ad-hoc signed
`Apollo.app` unless `APPLE_CERTIFICATE_P12` is set; Windows: an
`apollo-app` folder). `scripts/install-macos` writes `~/Applications/Apollo.app`;
don't run it in agent sessions.

## Contributing

Work on a branch and open a pull request into `main`; CI must pass.
Commit messages: lowercase conventional commits (`feat:`, `fix:`, `ci:`,
`build:`, `chore:`, `docs:`).

## Releases

Bump `version` in Cargo.toml, add its `## [x.y.z] - YYYY-MM-DD` section
(`### New` / `### Fixed`) at the top of CHANGELOG.md (a test checks the top
entry matches Cargo.toml; Apollo shows it as What's New), then push the tag
`v<version>` on a `main` commit whose ci passed. Only that tag push runs
`.github/workflows/release.yml`: it checks the tag matches Cargo.toml and is
on `main`, waits for that commit's ci, builds zips for macOS Apple silicon,
macOS Intel and Windows, and publishes them with `SHA256SUMS` to the GitHub
release (a tag with a `-`, like `v0.0.3-rc.1`, is a prerelease).
