# For Agents

```sh
herdr-app --skill
```

This prints the agent guide for driving the window. The herdr-core skill points agents here.
Use herdr-core (`hc`) for running and coordinating programs; use the app to change what you see.

```sh
herdr-app ls
herdr-app status
herdr-app commands
```

The first two inspect the open window. The command list also works without one.
See the [command reference](Commands) for arguments.

Apollo writes `~/.config/herdr-app/AGENTS.md` on every start: the installed guide to settings, layouts, commands and mods, with `.d.ts` files beside it for the full API.

Herdr clients share which agents a person has seen, and which are pinned, through terminal labels; see [Labels](Labels).

[Home](Home)
