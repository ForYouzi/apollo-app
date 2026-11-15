# Install

Download a zip from [Apollo's releases](https://github.com/ForYouzi/apollo-app/releases/latest):

| Zip | For |
|---|---|
| `apollo-app-<version>-macos-aarch64.zip` | macOS on Apple silicon |
| `apollo-app-<version>-macos-x86_64.zip` | macOS on Intel |
| `apollo-app-<version>-windows-x86_64.zip` | Windows on x64 |

Each release also has `SHA256SUMS` to check the download.

## macOS

Unzip it and move `Apollo.app` to Applications.
The build is signed ad hoc, not notarized, so the first launch is blocked.
Right-click Apollo and choose Open, or clear the quarantine flag once:

```sh
xattr -dr com.apple.quarantine /Applications/Apollo.app
```

## Windows

Unzip the `apollo-app` folder anywhere; `apollo-app.exe` opens the app.
The folder also holds `herdr-app.exe`, the same program: adding the folder to your PATH gives you the `herdr-app` command the docs use.

### Without a GPU

On a machine without a GPU (a VM, Remote Desktop, or the Microsoft Basic Render Driver), Apollo draws the cheaper way meant for a CPU:

- about 30 frames a second
- no antialiasing passes for shapes, and simple shadows
- an opaque window instead of `window_blur` (what a theme leaves transparent shows black)
- grayscale text with no contrast boost instead of ClearType, whose colored edges blur over Remote Desktop

Set `GPUI_SOFTWARE_RENDER` to `force` to draw that way on a GPU too, or `off` to never do it. The default, `auto`, decides from the graphics adapter.

## herdr-core

Apollo runs your terminals in [herdr-core](https://github.com/herdrdev/herdr-core). It installs the core when it is missing, and can update a core it cannot use.

Apollo shares Herdr's settings home, `~/.config/herdr-app`, and its core, so it uses your Herdr setup as is. The command is still `herdr-app`.

## Updates

Apollo never updates itself. Release builds check GitHub for a newer Apollo every hour. When one is out, "Apollo x.y.z is out" appears in the corner and in **Settings → Updates**; Download opens the release page.
Release builds keep herdr-core up to date; **Settings → Updates** shows what is available. Your terminals keep running in the core.
What's New opens after you install a new version; open it again from the command palette.

[Home](Home)
