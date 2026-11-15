# Customize

Open the command palette to find an action by name. Ctrl+Shift+P shows every command and its key (Cmd+Shift+P on macOS).
Open **Settings** to change fonts, colors, sounds and other preferences.

**Settings → Look** also adjusts terminal line height, cell width and scroll speed. All default to 100%; layouts can override them with `terminal_line_height` (0.8–2), `terminal_cell_width` (0.8–2) and `terminal_scroll_speed` (0.1–5).

## Layouts and themes

A **layout** arranges the window. The built-in layouts are Apollo, Terminals and Apollo 98. Each layout keeps its own light and dark theme.

**Mods** are small JavaScript programs that draw the window's panels, menus and dialogs. See [Layouts and Mods](Layouts-and-Mods).

Duplicate a shipped layout or mod before changing its structure or code. Your copies stay yours; Apollo updates the shipped ones on start.
Save a personal layout, mod or key file and the window reloads it.
A broken edit keeps the last good version and reports the error.

## Keys

Change any key in `keys.json`; see [the keys section](https://github.com/ForYouzi/apollo-app/blob/main/defaults/AGENTS.md#keys-keysjson) of the mod guide for the format, including JSON objects for named, typed command arguments.
The [command reference](Commands) lists every command you can bind.

[Home](Home)
