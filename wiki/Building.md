# Building

For bugs, mod API requests and code contributions, see [Contributing](https://github.com/ForYouzi/apollo-app/blob/main/CONTRIBUTING.md).

Clone the repo with its submodules. Use Rust 1.95.0 and Zig 0.16.0, the versions used in [CI](https://github.com/ForYouzi/apollo-app/blob/main/.github/workflows/ci.yml).
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

## Documentation

The `wiki/` folder is the documentation. A push to `main` publishes it to the [GitHub wiki](https://github.com/ForYouzi/apollo-app/wiki).
Pages link to each other by bare page name, such as `[Usage](Usage)`.

Checks need [Bun](https://bun.sh): `just docs-check` checks the generated [Commands](Commands) page and every CLI example in `README.md` and `wiki/`; `just docs-generate` refreshes [Commands](Commands) from `src/commands.rs`.

## Benchmarks

To compare drawing changes on Windows, Linux or macOS, run the same fixed workloads against each build:

```sh
python scripts/bench.py --bin <app executable> --set spinners --seconds 20
```

The runner requires `hc` on PATH and uses the Python executable running the script. It prints a unique artifact folder with the run settings, counters and results, then stops its private app, terminals and core.

- Every set uses Python fixture programs; the default set's TUI workload replaces historical `btop` measurements.
- `--only spinners` measures just the quiet native spinner scenario.
- `--set titles` runs eight agents whose titles spin after a prefix (`π ⠹ task`) on the main layout.

Keep the window visible and the display awake, and compare the same window activity and display settings; inactive windows may draw at a lower rate. Windows records visibility, foreground activity and display Hz at each measurement boundary; other platforms mark these observations unavailable. Visibility does not establish occlusion.

Draws are window render calls. Render-to-paint elapsed time stops before scene finalization, backend drawing, uploads and presentation; it is separate from OS process CPU time. Neither measures physical presentation or the FPS HUD's derived MAX FPS.

`python scripts/bench_compare.py ghostty --root <artifact folder>` reuses a run's fake agent for the macOS comparison. Benchmark checks: `python -m unittest discover -s scripts -p test_bench.py`.

## Releases

See "Releases" in [AGENTS.md](https://github.com/ForYouzi/apollo-app/blob/main/AGENTS.md#releases): merge Herdr's latest `upstream/master` first, then bump `Cargo.toml` and `CHANGELOG.md` and push the `v<version>` tag.

[Home](Home)
