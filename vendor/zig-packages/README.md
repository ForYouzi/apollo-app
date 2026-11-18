# Zig packages kept here

Ghostty's build fetches these by URL. When their host is down, a fresh build
(release, or ci on a cache miss) can't start. `scripts/zig-seed` puts each
one in Zig's package cache first, under the hash Ghostty's `build.zig.zon`
pins, so Zig never asks the host. Zig checks that hash, so a changed file
fails the build.

| File | From | License |
|---|---|---|
| `translate_c-0.0.0-Q_BUWhVNBwDOEcIqub4VFPJPB6D9dgwzUMHTX5KWr8Xr.tar.gz` | `codeberg.org/vancluever/translate-c` 4e879eb (`vendor/ghostty/pkg/translate-c/build.zig.zon`) | MIT, Zig contributors (in the archive) |

When the Ghostty submodule moves to a new pin, replace or drop the file to
match its `build.zig.zon`. It's a build tool; nothing from it ships.
