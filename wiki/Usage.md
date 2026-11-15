# Usage

## Spaces, tabs and panes

A **space** holds work on one machine, usually in one repo. Each space has **tabs**. Split a tab into **panes** to put terminals or mod panels beside each other.

An unnamed space follows its first terminal's folder. Name a space or tab when you want a label that stays.
New tabs start in the focused terminal's folder.

The window arranges terminals; [herdr-core](https://github.com/herdrdev/herdr-core) runs them.
Closing the window leaves them running; closing a pane or tab ends its terminals.
After a reboot or core crash, kept panes get fresh shells in their last folders, not their old programs.

## Agents

The agents sidebar brings agents from your spaces into one list, including workers started by another agent.
Agents that report status show as:

| Status | Means |
|---|---|
| **Working** | still busy |
| **Blocked** | waiting for you |
| **Done** | finished while you were looking elsewhere |
| **Idle** | not working, or a finished agent you have looked at |

Click an agent to see it. A worker without a pane opens in **peek**, over your tab. Keep it as a tab if you want it beside your other work.
Ctrl-click a live terminal address in terminal text to peek at it; use Cmd-click on macOS.
An address such as `t7` means a terminal on the same machine as the terminal showing it. `vps/t7` names one on another machine.

## Files and changes

**Files** shows the space's folder; **Changes** lists what Git sees as changed, with lines added and removed. Both open from the tab bar.

## Links and paths

Cmd+click (macOS) or Ctrl+click (Windows) a URL to open it, or a file path to show the file selected in its folder (Finder or Explorer) instead of opening it.

Links are found the way [SonicTerm](https://github.com/D0n9X1n/SonicTerm) finds them:

- URLs: `http`, `https`, `mailto`, `file`, plus `ftp`, `ssh`, `git`, `tel` and others.
- Paths that exist: absolute, `~/`, `./`, relative and bare names (in the terminal's folder), `$HOME/…`, Windows drive and `\\server\share` paths, paths with spaces or in quotes.
- `file.rs:12:3` and `file.rs:12-20`, and paths inside `Read(…)`, `[label](…)` or `key=…`.

A URL a program breaks across rows still opens whole when brackets close it: `(https://…` / `…)`, and a markdown link in Claude Code's numbered diff or markdown view.

## Selecting text

On macOS, Cmd+A selects the terminal's whole screen and scrollback, ready for Cmd+C.
In vim and other alternate-screen apps, it selects only that screen.
Linux and Windows keep Ctrl+A for the shell; bind `terminal.select_all` in `keys.json` if you want it.

## Other machines

Add an SSH machine from **Settings → Machines** or the sidebar's Add machine action.
Pick a host from your SSH config or type how SSH reaches it.
Spaces on that machine run their terminals there, not on your computer.

Each machine keeps its spaces, tabs and splits in its core. Connected screens share that structure and the live terminals, but keep their own focus and zoom.
Remote work keeps running when your local window closes or your laptop sleeps, as long as the remote machine and core stay up.

[Home](Home)
