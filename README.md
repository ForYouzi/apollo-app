<div align="center">

<img src="assets/icons/apollo-ui-icon.png" alt="Apollo icon: a black A on a yellow rounded square" width="128" height="128"/>

# Apollo

**A desktop home for your terminals and coding agents.**

[![Version](https://img.shields.io/github/v/release/ForYouzi/apollo-app?label=version&color=f5b82e)](https://github.com/ForYouzi/apollo-app/releases/latest) [![CI](https://github.com/ForYouzi/apollo-app/actions/workflows/ci.yml/badge.svg)](https://github.com/ForYouzi/apollo-app/actions/workflows/ci.yml) [![License: Apache-2.0](https://img.shields.io/badge/license-Apache--2.0-5ab8ff.svg)](LICENSE)

**[Download](https://github.com/ForYouzi/apollo-app/releases/latest)** · **[Documentation](wiki/Home.md)**

<sub>macOS (Apple silicon and Intel) · Windows x64</sub>

</div>

<p align="center">
  <a href="assets/screenshots/hero.png">
    <img src="assets/screenshots/hero.png" alt="Apollo with a storefront space open: a dev server logging requests beside a shell showing git status and the project tree" width="100%"/>
  </a>
  <br/>
  <sub><b>One space per project.</b> Tabs and split panes inside; the sidebar keeps every space one click away.</sub>
</p>

## Why Apollo

- **Spaces, tabs and panes.** Keep each project in its own space, and split terminals side by side.
- **See which agent needs you.** Agents show as working, blocked or done, across every space.
- **Terminals outlive the window.** [herdr-core](https://github.com/herdrdev/herdr-core) runs them; close the window and come back later, even over SSH.
- **Clickable everything.** URLs and file paths in terminal output open or reveal with one click, even when a program wraps them.
- **Make it yours.** Layouts, themes and JavaScript mods change the whole window, not just its colors.

## A closer look

<table>
  <tr>
    <td width="50%"><a href="assets/screenshots/diff.png"><img src="assets/screenshots/diff.png" alt="A tests tab showing a colored git diff and passing bun tests" width="100%"/></a><br/><sub><b>Tabs</b> — a dev server in one, your diff and tests in the next.</sub></td>
    <td width="50%"><a href="assets/screenshots/changes.png"><img src="assets/screenshots/changes.png" alt="The Changes panel listing two modified files with lines added and removed" width="100%"/></a><br/><sub><b>Changes</b> — what Git sees, one click from your terminals.</sub></td>
  </tr>
  <tr>
    <td width="50%"><a href="assets/screenshots/palette.png"><img src="assets/screenshots/palette.png" alt="The command palette open over a split-pane tab" width="100%"/></a><br/><sub><b>Command palette</b> — every action, with its key.</sub></td>
    <td width="50%"><a href="assets/screenshots/settings.png"><img src="assets/screenshots/settings.png" alt="Settings showing layout and theme choices" width="100%"/></a><br/><sub><b>Layouts and themes</b> — pick a layout; each keeps its own light and dark theme.</sub></td>
  </tr>
  <tr>
    <td width="50%"><a href="assets/screenshots/files.png"><img src="assets/screenshots/files.png" alt="The Files panel showing the project folder tree" width="100%"/></a><br/><sub><b>Files</b> — the space's folder beside your work.</sub></td>
    <td width="50%"><a href="assets/screenshots/win98.png"><img src="assets/screenshots/win98.png" alt="The Apollo 98 layout: terminals as DOS Prompt windows on a teal desktop with a taskbar" width="100%"/></a><br/><sub><b>Apollo 98</b> — the same terminals, redrawn entirely by mods.</sub></td>
  </tr>
</table>

## Get started

Download the [latest release](https://github.com/ForYouzi/apollo-app/releases/latest) for your platform and open it. Apollo shares Herdr's settings in `~/.config/herdr-app`, so an existing Herdr setup just works. See [Install](wiki/Install.md) for first-launch steps.

## Documentation

[Install](wiki/Install.md) · [Usage](wiki/Usage.md) · [Customize](wiki/Customize.md) · [Layouts and Mods](wiki/Layouts-and-Mods.md) · [Commands](wiki/Commands.md) · [For Agents](wiki/For-Agents.md) · [Security and Privacy](wiki/Security-and-Privacy.md) · [Building](wiki/Building.md)

## Thanks, Herdr

Apollo is a fork of [herdr-app](https://github.com/herdrdev/herdr-app), the Herdr desktop app, and runs its terminals in [herdr-core](https://github.com/herdrdev/herdr-core). It takes Herdr's changes before every release.

## License

[Apache-2.0](LICENSE)
