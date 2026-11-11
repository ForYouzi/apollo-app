//! Apollo's identity. Internal herdr ids (herdr-core, ~/.config/herdr-app, HERDR_*) stay.

/// A literal: scripts/brand-plist reads it with sed.
pub const NAME: &str = "Apollo";
/// `NAME` as a literal, for `concat!` where a `&'static str` is required (a test keeps them equal).
macro_rules! name {
    () => {
        "Apollo"
    };
}
pub(crate) use name;
/// Read by scripts/brand-plist for the macOS Info.plist, not by the app.
#[allow(dead_code)]
pub const BUNDLE_ID: &str = "com.foryouzi.apollo";
pub const APP_ID: &str = "apollo-app";
pub const BUNDLE: &str = "Apollo.app";
pub const EXECUTABLE: &str = "Apollo";
pub const RELEASES_PAGE: &str = "https://github.com/ForYouzi/apollo-app/releases";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn brand_names() {
        assert_eq!((NAME, BUNDLE_ID, APP_ID, BUNDLE, EXECUTABLE), ("Apollo", "com.foryouzi.apollo", "apollo-app", "Apollo.app", "Apollo"));
        assert_eq!(name!(), NAME);
        assert!(crate::HELP.starts_with("herdr-app: the Apollo desktop app. With no arguments"), "{}", crate::HELP);
    }

    #[test]
    fn icons_are_valid() {
        assert!(image::load_from_memory(include_bytes!("../assets/icons/apollo-ui-icon.png")).is_ok());
        assert!(include_bytes!("../assets/icons/Apollo.icns").starts_with(b"icns"));
        assert!(include_bytes!("../assets/icons/apollo-app.ico").starts_with(&[0, 0, 1, 0]));
    }

    /// welcome, whatsnew and workspace-chrome show `frame/welcome/herdr.png` (the name
    /// stays so user mods keep working): it is the Apollo icon at 160x160, not the ram.
    #[test]
    fn shipped_welcome_image_is_the_apollo_icon() {
        let bytes = include_bytes!("../defaults/frame/welcome/herdr.png");
        assert_ne!(crate::update::sha256(bytes), "a9e6710a7c41bc2d9cd16b08ae44e0e79fc99fd3ee4494dfa1b3d80c0867f167", "still the Herdr ram");
        let shipped = image::load_from_memory(bytes).unwrap().to_rgba8();
        assert_eq!(shipped.dimensions(), (160, 160));
        let icon = image::load_from_memory(include_bytes!("../assets/icons/apollo-ui-icon.png")).unwrap();
        let icon = icon.resize_exact(160, 160, image::imageops::FilterType::Triangle).to_rgba8();
        let diff: u64 = shipped.as_raw().iter().zip(icon.as_raw()).map(|(a, b)| a.abs_diff(*b) as u64).sum();
        let mean = diff as f64 / shipped.as_raw().len() as f64;
        assert!(mean < 6.0, "not the Apollo icon: mean channel difference {mean:.1}");
    }

    /// Apollo 98's About box, splash and Start / Update icons show the Apollo mark
    /// (art/generate.ts, from assets/icons/apollo.svg), not Herdr's ram.
    #[test]
    fn win98_art_shows_the_apollo_mark() {
        // The ram versions: Herdr's (before the rebrand) and the first "Apollo 98" text over the ram.
        let ram = [
            ("img/about.png", &include_bytes!("../defaults/mods/win98/img/about.png")[..], &["e9e9d3a23366d609b1dc02d6415dd575b53cb95f54e4a7b32a22ccb93e77c3b7", "1a349d434cb39773ffc48cdef1b03d4f1445452451580140f381c1c7258c8076"][..]),
            ("img/splash.png", include_bytes!("../defaults/mods/win98/img/splash.png"), &["04a0ca740f93e28551c59c9d77aa244d5908a5b8d5051f3a402abf325c9597d5", "a18166048bd97ac711253a226e53e63f687547c4ec05fc4fb5e1e79c6f647f03"]),
            ("icons/start.svg", include_bytes!("../defaults/mods/win98/icons/start.svg"), &["0aa338734e548f5d293ef15e29aae41560b054e219c536c37d13892c69304913"]),
            ("icons/start-sm.svg", include_bytes!("../defaults/mods/win98/icons/start-sm.svg"), &["a9c675871f2711f88da0f8a30860cd5dcc06cd7ed0e7253a5c4efdd7d6e471fb"]),
            ("icons/update.svg", include_bytes!("../defaults/mods/win98/icons/update.svg"), &["b16a9ce0f4c19994209402099eec0eafc1ba73bf4cb7aae3bae2b2ff4727b606"]),
            ("icons/update-sm.svg", include_bytes!("../defaults/mods/win98/icons/update-sm.svg"), &["df4d6ca573ee5fa8ad6a8c92cb0be8db0bd34b6781666a02df6070ba80c622eb"]),
        ];
        for (file, bytes, old) in ram {
            assert!(!old.contains(&crate::update::sha256(bytes).as_str()), "win98 {file} is still the ram");
            // The mark's amber tile (#f0a91c..#fbc94a), which the ram art never had.
            let amber = if file.ends_with(".png") {
                let img = image::load_from_memory(bytes).unwrap().to_rgba8();
                img.pixels().filter(|p| p[3] > 200 && p[0] > 220 && (150..215).contains(&p[1]) && p[2] < 100).count()
            } else {
                std::str::from_utf8(bytes).unwrap().matches("fill=\"#f5b933\"").count()
            };
            assert!(amber > 0, "win98 {file} has no Apollo amber");
        }
    }

    #[cfg(unix)]
    #[test]
    fn info_plist_renders_from_brand() {
        let out = std::process::Command::new("bash").args(["scripts/brand-plist", "9.8.7"]).current_dir(env!("CARGO_MANIFEST_DIR")).output().unwrap();
        let plist = String::from_utf8(out.stdout).unwrap();
        for want in [BUNDLE_ID, "<string>Apollo</string>", "<string>9.8.7</string>"] {
            assert!(plist.contains(want), "{want}");
        }
        assert!(!plist.contains('@') && !plist.contains("Herdr") && !plist.contains("dev.herdr"));
        // scripts/package and install-macos name the bundle from these.
        for (name, want) in [("NAME", NAME), ("BUNDLE_ID", BUNDLE_ID), ("EXECUTABLE", EXECUTABLE), ("BUNDLE", BUNDLE), ("APP_ID", APP_ID)] {
            let out = std::process::Command::new("bash").args(["scripts/brand-plist", "--const", name]).current_dir(env!("CARGO_MANIFEST_DIR")).output().unwrap();
            assert_eq!(String::from_utf8(out.stdout).unwrap().trim_end(), want, "brand-plist --const {name}");
        }
        for key in ["CFBundleName", "CFBundleDisplayName"] {
            assert!(plist.contains(&format!("<key>{key}</key><string>Apollo</string>")), "{key}: {plist}");
        }
    }

    /// With macOS's Dark icon style, an icon without its own dark look is darkened to black.
    /// `Apollo.icon` keeps the yellow in Dark; `scripts/mac-icon` compiles it into `Assets.car`,
    /// which the bundle names by `CFBundleIconName` (`Apollo.icns` stays for older macOS).
    #[test]
    fn mac_icon_stays_yellow_in_dark() {
        let icon: serde_json::Value = serde_json::from_str(include_str!("../assets/icons/Apollo.icon/icon.json")).unwrap();
        let fills = icon["fill-specializations"].as_array().unwrap();
        let fill = |appearance: Option<&str>| fills.iter().find(|f| f["appearance"].as_str() == appearance).map(|f| &f["value"]);
        let light = fill(None).expect("no default fill");
        assert_eq!(light["linear-gradient"][0], "srgb:0.98431,0.78824,0.29020,1.00000", "not apollo.svg's #fbc94a");
        assert_eq!(fill(Some("dark")), Some(light), "Dark must keep the yellow fill");
        assert!(include_bytes!("../assets/icons/Assets.car").starts_with(b"BOMStore"));
        let plist = include_str!("../packaging/Info.plist.in");
        assert!(plist.contains("<key>CFBundleIconName</key><string>Apollo</string>") && plist.contains("<key>CFBundleIconFile</key><string>Apollo</string>"));
        for (script, src) in [("package", include_str!("../scripts/package")), ("install-macos", include_str!("../scripts/install-macos"))] {
            assert!(src.contains("cp assets/icons/Assets.car \"$app/Contents/Resources/Assets.car\""), "{script} doesn't ship Assets.car");
        }
    }

    /// The window's app id (Wayland app_id, X11 WM_CLASS) is `APP_ID`: the launcher entry
    /// must be named after it and claim it, or the window shows up as a separate, iconless app.
    #[test]
    fn linux_launcher_entry_matches_the_app_id() {
        // A Windows checkout has CRLF line endings: normalize, then check both forms.
        let src = include_str!("../scripts/install-linux").replace("\r\n", "\n");
        for src in [src.clone(), src.replace('\n', "\r\n")] {
            let src = src.replace("\r\n", "\n");
            for want in ["id=$(scripts/brand-plist --const APP_ID)", "name=$(scripts/brand-plist --const NAME)", "\"$data/applications/$id.desktop\"", "Name=$name\n", "Icon=$id\n", "StartupWMClass=$id\n"] {
                assert!(src.contains(want), "install-linux lacks {want}");
            }
        }
    }

    /// The Windows zip ships `apollo-app.exe` (the brand) and `herdr-app.exe`, the same
    /// binary, so the documented `herdr-app …` commands work with the folder on PATH.
    #[test]
    fn windows_zip_ships_both_command_names() {
        let package = include_str!("../scripts/package").replace("\r\n", "\n");
        let start = package.find("if [[ $os == windows ]]; then\n").expect("scripts/package has no Windows branch");
        let windows = &package[start..start + package[start..].find("\nelse\n").unwrap()];
        for want in ["cp \"$bin/herdr-app.exe\" \"$dir/apollo-app.exe\"", "cp \"$bin/herdr-app.exe\" \"$dir/herdr-app.exe\"", "cmp \"$dir/apollo-app.exe\" \"$dir/herdr-app.exe\""] {
            assert!(windows.contains(want), "scripts/package's Windows branch lacks {want}:\n{windows}");
        }
        let release = include_str!("../.github/workflows/release.yml").replace("\r\n", "\n");
        let check = &release[release.find("- name: Check the Windows zip").unwrap()..];
        let check = &check[..check.find("- uses:").unwrap()];
        for exe in ["apollo-app.exe", "herdr-app.exe"] {
            let want = format!("HERDR_APP_HOME=\"$x/home\" \"$x/apollo-app/{exe}\" --version | grep -q \"^herdr-app \"");
            assert!(check.contains(&want), "release.yml doesn't run {exe}:\n{check}");
        }
        let readme = include_str!("../README.md").replace("\r\n", "\n");
        assert!(readme.contains("`apollo-app.exe` opens the app") && readme.contains("gives you the `herdr-app` command"), "README's Windows install text");
    }

    /// On a tag, nothing builds before ci passed on the commit; a workflow_dispatch dry run
    /// (where `ci-passed` is skipped) still builds.
    #[test]
    fn release_builds_wait_for_ci_on_tags() {
        let release = include_str!("../.github/workflows/release.yml").replace("\r\n", "\n");
        let build = &release[release.find("\n  build:\n").unwrap()..];
        let build = &build[..build.find("\n    strategy:").unwrap() + 1];
        assert!(build.contains("\n    needs: [validate, ci-passed]\n"), "{build}");
        let gate = "\n    if: ${{ !cancelled() && needs.validate.result == 'success' && (needs.ci-passed.result == 'success' || needs.ci-passed.result == 'skipped') }}\n";
        assert!(build.contains(gate), "{build}");
        assert!(release.contains("  publish:\n    needs: [build, ci-passed]\n    if: github.event_name == 'push' && github.ref_type == 'tag'\n"));
    }

    #[test]
    fn user_facing_strings_use_the_brand() {
        for (file, src) in [("main.rs", include_str!("main.rs")), ("setup.rs", include_str!("setup.rs")), ("macos.rs", include_str!("macos.rs"))] {
            for old in ["title: Some(\"Herdr\"", "Menu::new(\"Herdr\")", "\"Hide Herdr\"", "\"Quit Herdr\"", "\"Herdr needs", "\"herdr-app\".into()", "releases.herdr.dev/app"] {
                assert!(!src.contains(old), "{file} still has {old}");
            }
        }
    }

    /// The string literals in `src`'s code, skipping comments, char literals and
    /// `#[cfg(test)]` modules anywhere in the file. Nothing else is skipped (a test-only fn,
    /// a `cfg(test)` parameter): a wrong guess can only report too much, never hide code.
    fn string_literals(src: &str) -> Vec<String> {
        let b = src.as_bytes();
        let (mut i, mut out) = (0, Vec::new());
        // Brace depth; a pending `#[cfg(test)]` item; the depth its body closes at;
        // the last byte of code (not whitespace or a comment): items follow `{`, `}`, `;` or `]`.
        let (mut depth, mut pending, mut skip, mut last) = (0usize, false, None::<usize>, b';');
        while i < b.len() {
            match b[i] {
                b'/' if b.get(i + 1) == Some(&b'/') => i = src[i..].find('\n').map_or(b.len(), |n| i + n),
                b'/' if b.get(i + 1) == Some(&b'*') => i = src[i + 2..].find("*/").map_or(b.len(), |n| i + n + 4),
                b'#' if src[i..].starts_with("#[cfg(test)]") && matches!(last, b'{' | b'}' | b';' | b']') && mod_follows(&src[i + "#[cfg(test)]".len()..]) => {
                    pending = skip.is_none();
                    i += "#[cfg(test)]".len();
                    last = b']';
                }
                b';' if pending => {
                    (pending, last) = (false, b';');
                    i += 1;
                }
                b'{' => {
                    if pending {
                        (pending, skip) = (false, Some(depth));
                    }
                    depth += 1;
                    (i, last) = (i + 1, b'{');
                }
                b'}' => {
                    depth = depth.saturating_sub(1);
                    if skip == Some(depth) {
                        skip = None;
                    }
                    (i, last) = (i + 1, b'}');
                }
                b'\'' if b.get(i + 2) == Some(&b'\'') || (b.get(i + 1) == Some(&b'\\') && b.get(i + 3) == Some(&b'\'')) => {
                    i += if b[i + 1] == b'\\' { 4 } else { 3 };
                    last = b'\'';
                }
                b'r' if (i == 0 || !(b[i - 1].is_ascii_alphanumeric() || b[i - 1] == b'_')) && matches!(b.get(i + 1), Some(b'"' | b'#')) => {
                    let hashes = b[i + 1..].iter().take_while(|&&c| c == b'#').count();
                    let start = i + 1 + hashes;
                    if b.get(start) != Some(&b'"') {
                        (i, last) = (i + 1, b'r');
                        continue;
                    }
                    let close = format!("\"{}", "#".repeat(hashes));
                    let end = src[start + 1..].find(&close).map_or(b.len(), |n| start + 1 + n);
                    if skip.is_none() && !pending {
                        out.push(src[start + 1..end.min(b.len())].to_owned());
                    }
                    (i, last) = (end + close.len(), b'"');
                }
                b'"' => {
                    let mut j = i + 1;
                    while j < b.len() && b[j] != b'"' {
                        j += if b[j] == b'\\' { 2 } else { 1 };
                    }
                    if skip.is_none() && !pending {
                        out.push(src[i + 1..j.min(b.len())].to_owned());
                    }
                    (i, last) = (j + 1, b'"');
                }
                c => {
                    if !c.is_ascii_whitespace() {
                        last = c;
                    }
                    i += 1;
                }
            }
        }
        out
    }

    /// After `#[cfg(test)]`: does a `mod` follow, past more attributes, comments and `pub(…)`?
    fn mod_follows(mut s: &str) -> bool {
        loop {
            s = s.trim_start();
            if s.starts_with("//") {
                s = s.find('\n').map_or("", |n| &s[n..]);
            } else if s.starts_with("/*") {
                s = s.find("*/").map_or("", |n| &s[n + 2..]);
            } else if s.starts_with("#[") {
                let mut open = 0;
                let end = s.char_indices().find(|&(_, c)| {
                    open += (c == '[') as i32 - (c == ']') as i32;
                    c == ']' && open == 0
                });
                s = end.map_or("", |(k, _)| &s[k + 1..]);
            } else {
                let word = &s[..s.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).unwrap_or(s.len())];
                match word {
                    "pub" => {
                        s = s[3..].trim_start();
                        if s.starts_with('(') {
                            s = s.find(')').map_or("", |n| &s[n + 1..]);
                        }
                    }
                    _ => return word == "mod",
                }
            }
        }
    }

    #[test]
    fn string_literals_skip_comments_and_tests() {
        let src = "let a = \"x Apollo\"; // \"Apollo in a comment\"\n/* \"no\" */ let c = '\"'; let r = r#\"raw \"q\"\"#; let d = \"e\\\"f\";";
        assert_eq!(string_literals(src), ["x Apollo", "raw \"q\"", "e\\\"f"]);
        let src = "fn a() { \"1\" }\n#[cfg(test)]\nuse x;\n#[cfg(test)]\nmod tests { fn t() { if x { \"no }\" } } }\nfn b() { \"2\" }";
        assert_eq!(string_literals(src), ["1", "2"]);
        // Only a `#[cfg(test)]` module is hidden: not a parameter, field, statement, generic or other item.
        assert_eq!(string_literals("fn f(#[cfg(test)] x: ()) { let _ = \"Apollo\"; }"), ["Apollo"]);
        assert_eq!(string_literals("struct S { #[cfg(test)] x: u8 }\nfn g() { \"Apollo\" }"), ["Apollo"]);
        assert_eq!(string_literals("fn h() {\n    #[cfg(test)]\n    let t = 1;\n    \"Apollo\"\n}"), ["Apollo"]);
        assert_eq!(string_literals("#[cfg(test)] mod tests { \"Apollo\" }"), Vec::<String>::new());
        assert_eq!(string_literals("fn label<\n    #[allow(unused)]\n    #[cfg(test)]\n    const N: usize,\n>() -> &'static str {\n    \"Apollo\"\n}"), ["Apollo"]);
        assert_eq!(string_literals("#[cfg(test)]\n#[allow(dead_code)]\nfn t() { \"Apollo\" }"), ["Apollo"]);
        assert_eq!(string_literals("fn a() {}\n/// Docs.\n#[cfg(test)]\n#[allow(dead_code)]\npub(crate) mod t { \"Apollo\" }\n\"3\""), ["3"]);
    }

    /// The app's name in its Rust strings comes from `NAME` / `name!()`: no "Apollo"
    /// literal outside this file, test modules and test-only files.
    #[test]
    fn no_apollo_literal_outside_brand() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut stack = vec![root.clone()];
        let mut found = Vec::new();
        while let Some(dir) = stack.pop() {
            for e in std::fs::read_dir(&dir).unwrap().flatten() {
                let p = e.path();
                let name = p.file_name().unwrap().to_string_lossy().into_owned();
                if p.is_dir() {
                    stack.push(p);
                } else if name.ends_with(".rs") && name != "brand.rs" && !name.ends_with("_tests.rs") {
                    let src = std::fs::read_to_string(&p).unwrap();
                    for lit in string_literals(&src).into_iter().filter(|l| l.contains("Apollo")) {
                        found.push(format!("{}: {lit:?}", p.strip_prefix(&root).unwrap().display()));
                    }
                }
            }
        }
        assert!(found.is_empty(), "use crate::brand::NAME:\n{}", found.join("\n"));
    }

    /// Every user-visible "Herdr" in the app's own strings, shipped screens and mods says Apollo.
    /// Internal names stay and are stripped before matching: the key contexts `"Herdr"` and
    /// `"Herdr layout = …"` (user keymaps bind to them, src/keys.rs:120), herdr-core, herdr-app, HERDR_*.
    #[test]
    fn no_user_visible_herdr() {
        let internal = fancy_regex::Regex::new(r#"key_context\((?:format!\()?"Herdr(?: layout = \{\})?"|=> Ok\("Herdr"\.into\(\)\)|[Hh]erdr-(?:core|app)|HERDR_\w+"#).unwrap();
        // `"…\nHerdr"`: the escape's `n` is a word character, so \b alone misses it.
        let visible = fancy_regex::Regex::new(r"(?:\\n|\b)Herdr\b").unwrap();
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut files: Vec<std::path::PathBuf> = ["setup.rs", "main.rs", "macos.rs", "window.rs", "control.rs", "terminal.rs", "keys.rs"]
            .iter()
            .map(|f| root.join("src").join(f))
            .collect();
        let mut stack = vec![root.join("defaults/frame"), root.join("defaults/mods")];
        while let Some(dir) = stack.pop() {
            for e in std::fs::read_dir(&dir).unwrap().flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p)
                } else if p.extension().is_some_and(|x| x == "js") {
                    files.push(p)
                }
            }
        }
        // The source installers: `#` starts a comment there, `//` doesn't.
        let installers = [root.join("scripts/install-linux"), root.join("scripts/install-windows.ps1")];
        files.extend(installers.iter().cloned());
        // The guides `--skill` prints and the app writes into its home: prose, read whole.
        let guides = [root.join("src/SKILL.md"), root.join("defaults/AGENTS.md")];
        files.extend(guides.iter().cloned());
        for path in files {
            let src = std::fs::read_to_string(&path).unwrap();
            let shell = installers.contains(&path);
            let prose = guides.contains(&path);
            for (n, line) in src.lines().enumerate() {
                if shell && line.trim_start().starts_with('#') {
                    continue;
                }
                let code = if shell || prose { line } else { line.split("//").next().unwrap_or(line) };
                let code = internal.replace_all(code, "");
                assert!(!visible.is_match(code.as_ref()).unwrap(), "{}:{} still shows Herdr: {line}", path.display(), n + 1);
            }
        }
    }
}
