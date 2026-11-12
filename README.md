# Apollo

Apollo is a desktop app for terminals and coding agents. Keep work in spaces, see which agents need you, and leave terminals running when you close the window.

Apollo is a fork of [herdr-app](https://github.com/herdrdev/herdr-app) (Apache-2.0). It keeps Herdr's settings home, `~/.config/herdr-app`, and runs terminals in [herdr-core](https://github.com/herdrdev/herdr-core), so it shares your Herdr setup. The command is still `herdr-app`.

## Install

Download a zip from [Apollo's releases](https://github.com/ForYouzi/apollo-app/releases):

- `apollo-app-<version>-macos-aarch64.zip`: macOS on Apple silicon.
- `apollo-app-<version>-macos-x86_64.zip`: macOS on Intel.
- `apollo-app-<version>-windows-x86_64.zip`: Windows on x64.

On macOS, unzip it and move `Apollo.app` to Applications.
The macOS build is signed ad hoc, not notarized, so the first launch is blocked.
Right-click Apollo and choose Open, or clear the quarantine flag once:

```sh
xattr -dr com.apple.quarantine /Applications/Apollo.app
```

On Windows, unzip the `apollo-app` folder anywhere; `apollo-app.exe` opens the app.
The folder also holds `herdr-app.exe`, the same program: adding the folder to your PATH gives you the `herdr-app` command the docs use.

Apollo installs [herdr-core](https://github.com/herdrdev/herdr-core) when it is missing, and can update a core it cannot use.

### Reveal files instead of opening them

Cmd+click (macOS) or Ctrl+click (Windows) on a file path in terminal text shows the file selected in its folder (Finder or Explorer) instead of opening it.

Links are found the way [SonicTerm](https://github.com/D0n9X1n/SonicTerm) finds them: URLs (`http`, `https`, `mailto`, `file`, plus `ftp`, `ssh`, `git`, `tel` and others), and paths that exist: absolute, `~/`, `./`, relative and bare names (in the terminal's folder), `$HOME/…`, Windows drive and `\\server\share` paths, paths with spaces or in quotes, `file.rs:12:3` and `file.rs:12-20`, and paths inside `Read(…)`, `[label](…)` or `key=…`. A URL a program breaks across rows still opens whole when brackets close it: `(https://…` / `…)`, and a markdown link in Claude Code's numbered diff or markdown view.

## Find your work

A **space** holds work on one machine, usually in one repo. Each space has **tabs**. Split a tab into **panes** to put terminals or mod panels beside each other.

An unnamed space follows its first terminal's folder. Name a space or tab when you want a label that stays.
New tabs start in the focused terminal's folder.

The agents sidebar brings agents from your spaces into one list, including workers started by another agent.
Agents that report status show as:

- **Working:** still busy.
- **Blocked:** waiting for you.
- **Done:** finished while you were looking elsewhere.
- **Idle:** not working, or a finished agent you have looked at.

Click an agent to see it. A worker without a pane opens in **peek**, over your tab. Keep it as a tab if you want it beside your other work.
Ctrl-click a live terminal address in terminal text to peek at it; use Cmd-click on macOS.
An address such as `t7` means a terminal on the same machine as the terminal showing it. `vps/t7` names one on another machine.

The window arranges terminals. The core runs them.
Closing the window leaves them running; closing a pane or tab ends its terminals.
After a reboot or core crash, kept panes get fresh shells in their last folders, not their old programs.

## Work on another machine

Add an SSH machine from Settings → Machines or the sidebar's Add machine action.
Pick a host from your SSH config or type how SSH reaches it.
Spaces on that machine run their terminals there, not on your computer.

Each machine keeps its spaces, tabs and splits in its core. Connected screens share that structure and the live terminals, but keep their own focus and zoom.
Remote work keeps running when your local window closes or your laptop sleeps, as long as the remote machine and core stay up.

## Make it yours

Open the command palette to find an action by name, or open Settings to change fonts, colors, sounds and other preferences.
The built-in layouts include Apollo, Terminals and Apollo 98.
A **layout** arranges the window. **Mods** are small JavaScript programs that draw its panels, menus and dialogs.

Duplicate a shipped layout or mod before changing its structure or code. Your copies stay yours; Apollo updates the shipped ones on start.
Save a personal layout, mod or key file and the window reloads it.
A broken edit keeps the last good version and reports the error.

Ctrl+Shift+P shows every command and its key (Cmd+Shift+P on macOS). Change any key in `keys.json`; see [the keys section](defaults/AGENTS.md#keys-keysjson) of the mod guide for the format, including JSON objects for named, typed command arguments.

[Layouts and mods](docs/mods.md) · [Commands](docs/commands.md)

## Updates and privacy

Apollo never updates itself. Download new versions from [Apollo's releases](https://github.com/ForYouzi/apollo-app/releases).
Release builds check for and install herdr-core updates; Settings → Updates shows what is available. Your terminals keep running in the core.
What's New opens after you install a new version; you can open it again from the command palette.

Apollo builds collect and send no usage data.
Mods are trusted code, not sandboxed extensions. Read [security and privacy](docs/security.md) before installing someone else's mod.

## For agents

```sh
herdr-app --skill
```

This prints the agent guide for driving the window. The herdr-core skill points agents here.
Use herdr-core for running and coordinating programs; use the app to change what you see.

```sh
herdr-app ls
herdr-app status
herdr-app commands
```

The first two inspect the open window. The command list also works without one.
See the [command reference](docs/commands.md) for arguments.

## Build from source

For bugs, mod API requests and code contributions, see [Contributing](CONTRIBUTING.md).

Clone this repo with its submodules. Use Rust 1.95.0 and Zig 0.16.0, the versions used in [CI](.github/workflows/ci.yml).
Linux needs the development packages listed in that workflow.

```sh
git clone --recurse-submodules https://github.com/ForYouzi/apollo-app.git
cd apollo-app
just build
just test
just install
```

Install [just](https://github.com/casey/just) to use these recipes.
The platform installer builds a release and adds the app to your desktop.
On Linux it links to this checkout, so keep the checkout in place.
Quit and reopen Apollo to use the new build.

Documentation checks also need [Bun](https://bun.sh): `just docs-check` checks generated pages and CLI examples; `just docs-generate` refreshes the reference pages.

To compare drawing changes on Windows, Linux or macOS, run the same fixed workloads against each build:

```sh
python scripts/bench.py --bin <app executable> --set spinners --seconds 20
```

The runner requires `hc` on PATH and uses the Python executable running the script. It prints a unique artifact folder with the run settings, counters and results, then stops its private app, terminals and core. Both sets use Python fixture programs; the default set’s TUI workload replaces historical `btop` measurements. `--only spinners` measures just the quiet native spinner scenario. Keep the window visible and the display awake, and compare the same window activity and display settings; inactive windows may draw at a lower rate. Windows records visibility, foreground activity and display Hz at each measurement boundary; other platforms mark these observations unavailable. Visibility does not establish occlusion. Draws are window render calls. Render-to-paint elapsed time stops before scene finalization, backend drawing, uploads and presentation; it is separate from OS process CPU time. Neither measures physical presentation or the FPS HUD’s derived MAX FPS.

`python scripts/bench_compare.py ghostty --root <artifact folder>` reuses a run’s fake agent for the macOS comparison. Benchmark checks: `python -m unittest discover -s scripts -p test_bench.py`.
