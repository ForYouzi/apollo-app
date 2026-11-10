# Security and privacy

## Mods have your access

A mod is trusted JavaScript running inside Apollo. It can run programs with your environment, fetch URLs and read files.
The built-in file API limits writes to the app home, but a mod can run a program to write elsewhere. That is not a security boundary.
Mods can also run programs on connected machines.

There is no permission prompt for each program or network request. Read a mod before installing it, and treat it like a shell script you would run yourself.
Layouts can load mods. Check those references too.

## What stays on disk

The default app home is `~/.config/herdr-app`, including on macOS and Windows (under your user home).
`XDG_CONFIG_HOME` changes the config root. `HERDR_APP_HOME` selects an app home directly and takes precedence.

This folder holds your settings, keys, layouts, mods, generated agent guide and API types.
It also holds mod data, shared state, window positions, this screen's focus and zoom, update notes and cached media, and telemetry while enabled.
Mods may store sensitive data here. Apollo does not encrypt this folder.
The [installed guide's file map](../defaults/AGENTS.md#the-files) explains the files.

Spaces, tabs and splits are stored in each machine's core document, not in the app home.
Terminals and their output belong to [herdr-core](https://github.com/herdrdev/herdr-core).
Removing the app home does not stop the core or erase its data.

The app exposes a local control socket in its home so agents can inspect and drive the window.
Treat access to that home as access to the window and its commands.
Remote connections use SSH through the core.

## Updates and signing

Apollo never updates itself, in release builds or source builds. Download new versions from [Apollo's releases](https://github.com/ForYouzi/apollo-app/releases); each release lists SHA-256 checksums in `SHA256SUMS`.
A checksum published beside the download detects a damaged file; it is not an independent signature.

herdr-core keeps updating. Release builds check `https://releases.herdr.dev` for a newer core of the same generation, and Settings → Updates installs it with the core's own updater (`hc update`). A core a package manager owns is left to that package manager.

Apollo's published macOS builds are ad hoc signed and not notarized (the release workflow has no Developer ID certificate; `scripts/package` signs with one only when `APPLE_CERTIFICATE_P12` is set). Windows builds are not signed.
Do not assume a download is notarized or certificate-signed.

## Telemetry

Apollo does not collect or send usage data: no build sets `HERDR_APP_RELEASE`, so the app has no report endpoint, and without one it keeps no usage report at all.
Apollo shares its home with Herdr, and it never writes, reads or deletes the `telemetry` folder there; Herdr's own reports stay Herdr's.
**Settings → Privacy** says "Off: Apollo collects no usage data." whatever the setting says.
Only if you point `HERDR_TELEMETRY_URL` at a server does the app collect a small daily usage report, as below, and send it there.
Reports include app and core versions, OS and architecture, bucketed usage time and counts, known agent names, built-in layout names, mod counts, app command names, and fixed event counters.
Custom layout names become `custom`; custom mod names are not sent.
Terminal text, prompts, file contents, folder paths and free-text names are not included.

With `HERDR_TELEMETRY_URL` set, turn it off in **Settings → Privacy**, or set this field in your existing `settings.json`:

```json
{
  "telemetry": false
}
```

Turning it off deletes the local install ID and unsent reports.
For one run, set `DO_NOT_TRACK=1` or `HERDR_TELEMETRY=0`. CI environments disable collection too.

Telemetry being off does not stop update checks, mod network requests or feedback you choose to send.
Feedback is drafted for you to review and send, not sent silently.

[Back to Apollo](../README.md)
