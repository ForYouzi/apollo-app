//! Agents come back in their own conversations after a restart (issue #38).
//!
//! herdr-core gives a kept terminal a fresh shell when its program is lost
//! (reboot, core crash, logout) and never runs the old command again. While
//! Apollo runs, it notes each kept Claude Code terminal's conversation in a
//! core label, `apollo.resume`, read from Claude's own per-process file
//! `~/.claude/sessions/<pid>.json`: no hooks, the same on macOS and Windows.
//! When that terminal shows up again with a different shell, the resume line
//! is typed into it once. Only the decisions live here; window.rs does the
//! reading, labeling and typing off the UI thread.

use serde_json::{Value, json};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// The label holding a terminal's resume recipe: `{"id","cwd","argv","shell"}`.
pub const LABEL: &str = "apollo.resume";

/// What Claude Code writes for each running process (undocumented; read loosely).
#[derive(Debug, Clone, PartialEq)]
pub struct Session {
    pub id: String,
    pub cwd: String,
}

/// `~/.claude/sessions/<pid>.json` (honours `CLAUDE_CONFIG_DIR`, as Claude does).
pub fn session_file(home: &Path, config_dir: Option<&str>, pid: u64) -> PathBuf {
    claude_dir(home, config_dir).join("sessions").join(format!("{pid}.json"))
}

/// The transcript Claude resumes from: `projects/<cwd, / and \ and : as ->/<id>.jsonl`.
pub fn transcript(home: &Path, config_dir: Option<&str>, cwd: &str, id: &str) -> PathBuf {
    let slug: String = cwd.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '-' }).collect();
    claude_dir(home, config_dir).join("projects").join(slug).join(format!("{id}.jsonl"))
}

/// Whether a file is gone: `Some(true)` only when the system says it isn't
/// there; `None` when it can't tell (permissions, I/O).
pub fn gone(path: &Path) -> Option<bool> {
    match std::fs::metadata(path) {
        Ok(_) => Some(false),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Some(true),
        Err(_) => None,
    }
}

/// Whether Claude has the conversation's transcript.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Found {
    Yes,
    No,
    /// Couldn't look (no projects folder, an error): never a reason to drop a recipe.
    Unknown,
}

/// Claude's own folder for `cwd` first; it shortens long names, so then every
/// project folder (an id is unique).
pub fn find_transcript(home: &Path, config_dir: Option<&str>, cwd: &str, id: &str) -> Found {
    let mut unsure = false;
    match gone(&transcript(home, config_dir, cwd, id)) {
        Some(false) => return Found::Yes,
        None => unsure = true,
        Some(true) => {}
    }
    let Ok(projects) = std::fs::read_dir(claude_dir(home, config_dir).join("projects")) else { return Found::Unknown };
    let name = format!("{id}.jsonl");
    for entry in projects {
        let Ok(entry) = entry else { return Found::Unknown };
        match gone(&entry.path().join(&name)) {
            Some(false) => return Found::Yes,
            None => unsure = true,
            Some(true) => {}
        }
    }
    if unsure { Found::Unknown } else { Found::No }
}

fn claude_dir(home: &Path, config_dir: Option<&str>) -> PathBuf {
    config_dir.filter(|d| !d.is_empty()).map(PathBuf::from).unwrap_or_else(|| home.join(".claude"))
}

/// A person's interactive conversation, or None (background, daemon, spare, unreadable).
pub fn parse_session(text: &str) -> Option<Session> {
    let v: Value = serde_json::from_str(text).ok()?;
    if v.get("kind").and_then(Value::as_str).is_some_and(|k| k != "interactive") || v.get("spare").and_then(Value::as_bool) == Some(true) {
        return None;
    }
    let id = v.get("sessionId")?.as_str().filter(|s| valid_id(s))?.to_owned();
    let cwd = v.get("cwd")?.as_str().filter(|s| !s.is_empty())?.to_owned();
    Some(Session { id, cwd })
}

/// Session ids are UUIDs; anything else is never typed into a shell.
fn valid_id(id: &str) -> bool {
    let groups: Vec<&str> = id.split('-').collect();
    groups.iter().map(|g| g.len()).eq([8, 4, 4, 4, 12]) && groups.iter().all(|g| g.chars().all(|c| c.is_ascii_hexdigit()))
}

/// Claude's options that take a value: kept with it when cleaning argv.
const TAKES_VALUE: &[&str] = &[
    "--model", "--permission-mode", "--effort", "--agent", "--agents", "--append-system-prompt", "--system-prompt",
    "--settings", "--setting-sources", "--fallback-model", "--permission-prompt-tool", "--permission-prompts",
    "--max-budget-usd", "--name", "-n", "--plugin-url", "--remote-control-session-name-prefix", "--debug-file",
    "--input-format", "--output-format", "--json-schema", "--autocompact", "--system-prompt-snapshot",
    "--plugin-dir", "--system-prompt-file", "--append-system-prompt-file",
];
/// Options that take one or more values (a run up to the next option).
const TAKES_VALUES: &[&str] = &[
    "--add-dir", "--allowedTools", "--allowed-tools", "--disallowedTools", "--disallowed-tools", "--tools",
    "--mcp-config", "--betas", "--file",
];
/// Options that may take a value (`-d api`): kept with it, as Claude parses them.
const MAYBE_VALUE: &[&str] = &["--debug", "-d", "--prompt-suggestions"];
/// Flags without a value that a resumed session keeps.
const FLAGS: &[&str] = &[
    "--allow-dangerously-skip-permissions", "--dangerously-skip-permissions", "--ax-screen-reader", "--bare", "--brief",
    "--chrome", "--no-chrome", "--disable-slash-commands", "--exclude-dynamic-system-prompt-sections",
    "--forward-subagent-text", "--ide", "--include-hook-events", "--include-partial-messages",
    "--no-session-persistence", "--replay-user-messages", "--restricted", "--safe-mode", "--strict-mcp-config", "--verbose",
];
/// Dropped with their value: a resume picks its own conversation, and these
/// start somewhere else (another worktree, tmux, a remote or desktop session).
const DROP_WITH_VALUE: &[&str] = &[
    "--resume", "-r", "--session-id", "--from-pr", "--teleport", "--cloud", "--environment",
    "--worktree", "-w", "--remote-control", "--tmux",
];
/// Options whose value may be inline JSON (secrets, servers): kept only as a file path.
const JSON_OR_FILE: &[&str] = &["--settings", "--mcp-config", "--agents", "--json-schema"];
/// Dropped alone: they start another conversation or don't stay interactive.
const DROP: &[&str] = &["--continue", "-c", "--fork-session", "--print", "-p", "--bg", "--background", "--desktop", "--help", "-h", "--version", "-v"];

/// The agent's own options from its argv, without what picks a conversation
/// and without a first message (a positional prompt would be sent again).
/// `None` when argv isn't Claude Code.
pub fn clean_argv(argv: &[String]) -> Option<Vec<String>> {
    let at = program_at(argv).filter(|&i| is_claude(&argv[i]))?;
    let mut out = vec!["claude".to_owned()];
    let mut i = at + 1;
    while i < argv.len() {
        let a = &argv[i];
        let (name, inline) = a.split_once('=').map_or((a.as_str(), false), |(n, _)| (n, true));
        let next_is_value = |j: usize| argv.get(j).is_some_and(|v| !v.starts_with('-'));
        let inline_json = |v: &str| v.trim_start().starts_with('{') || v.trim_start().starts_with('[');
        if !a.starts_with('-') || a == "-" || a == "--" {
            // A positional: the first message. Everything after `--` is too.
            if a == "--" { break; }
            i += 1;
            continue;
        }
        if DROP.contains(&name) {
            i += 1;
        } else if DROP_WITH_VALUE.contains(&name) {
            i += if !inline && next_is_value(i + 1) { 2 } else { 1 };
        } else if TAKES_VALUES.contains(&name) {
            let json = JSON_OR_FILE.contains(&name);
            let mut values = Vec::new();
            i += 1;
            if !inline {
                while next_is_value(i) {
                    if !(json && inline_json(&argv[i])) { values.push(argv[i].clone()); }
                    i += 1;
                }
            }
            if inline && !(json && inline_json(a.split_once('=').map_or("", |(_, v)| v))) {
                out.push(a.clone());
            } else if !inline && !values.is_empty() {
                out.push(a.clone());
                out.extend(values);
            }
        } else if TAKES_VALUE.contains(&name) || MAYBE_VALUE.contains(&name) {
            let value = if inline { a.split_once('=').map(|(_, v)| v.to_owned()) } else { next_is_value(i + 1).then(|| argv[i + 1].clone()) };
            let skip = JSON_OR_FILE.contains(&name) && value.as_deref().is_some_and(inline_json);
            if !skip {
                out.push(a.clone());
                if !inline { out.extend(value.clone()); }
            }
            i += if !inline && value.is_some() { 2 } else { 1 };
        } else if FLAGS.contains(&name) && !inline {
            out.push(a.clone());
            i += 1;
        } else {
            // An option this doesn't know: dropped with what may be its value,
            // so neither it nor a first message after it comes back.
            i += if !inline && next_is_value(i + 1) { 2 } else { 1 };
        }
    }
    Some(out)
}

/// Where the program is: argv[0], or the script an interpreter runs
/// (`node .../cli.js`, `sh .../claude`). Never an argument (`vim claude`).
fn program_at(argv: &[String]) -> Option<usize> {
    const INTERPRETERS: &[&str] = &["node", "bun", "deno", "sh", "bash", "zsh", "dash"];
    let base = |a: &str| {
        let b = a.rsplit(['/', '\\']).next().unwrap_or(a).to_ascii_lowercase();
        b.strip_suffix(".exe").map(str::to_owned).unwrap_or(b)
    };
    let first = argv.first()?;
    if INTERPRETERS.contains(&base(first).as_str()) {
        return argv.iter().skip(1).position(|a| !a.starts_with('-')).map(|i| i + 1);
    }
    Some(0)
}

/// `claude`, `claude.exe`, or npm's `.../@anthropic-ai/claude-code/cli.js` run by node.
fn is_claude(arg: &str) -> bool {
    let base = arg.rsplit(['/', '\\']).next().unwrap_or(arg).to_ascii_lowercase();
    let base = base.strip_suffix(".exe").unwrap_or(&base);
    base == "claude" || (base == "cli.js" && arg.replace('\\', "/").contains("@anthropic-ai/claude-code/"))
}

/// The shells a resume line can be typed into, by how they quote.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Shell {
    Posix,
    Fish,
    PowerShell,
}

/// The shell a terminal shows now (core's `foreground.name`), or None (cmd, nu, a program).
pub fn shell_kind(name: &str) -> Option<Shell> {
    let base = name.rsplit(['/', '\\']).next().unwrap_or(name).trim_start_matches('-').to_ascii_lowercase();
    match base.strip_suffix(".exe").unwrap_or(&base) {
        "sh" | "bash" | "zsh" | "dash" | "ksh" => Some(Shell::Posix),
        "fish" => Some(Shell::Fish),
        "pwsh" | "powershell" => Some(Shell::PowerShell),
        _ => None,
    }
}

/// The line typed into a fresh shell: go to the conversation's folder, then resume it.
pub fn resume_line(shell: Shell, cwd: &str, argv: &[String], id: &str) -> String {
    let mut words: Vec<String> = argv.to_vec();
    words.extend(["--resume".to_owned(), id.to_owned()]);
    match shell {
        Shell::Posix | Shell::Fish => {
            let q = |w: &str| if shell == Shell::Fish { fish_quote(w) } else { posix_quote(w) };
            format!("cd {} && {}", q(cwd), words.iter().map(|w| q(w)).collect::<Vec<_>>().join(" "))
        }
        Shell::PowerShell => format!(
            "Set-Location -LiteralPath {}; if ($?) {{ {} }}",
            crate::programs::powershell_line(&[cwd.to_owned()]).trim_start_matches("& "),
            crate::programs::powershell_line(&words)
        ),
    }
}

fn posix_quote(w: &str) -> String {
    if !w.is_empty() && w.chars().all(|c| c.is_ascii_alphanumeric() || "_-./:@%+=,".contains(c)) {
        return w.to_owned();
    }
    format!("'{}'", w.replace('\'', "'\\''"))
}

fn fish_quote(w: &str) -> String {
    if !w.is_empty() && w.chars().all(|c| c.is_ascii_alphanumeric() || "_-./:@%+=,".contains(c)) {
        return w.to_owned();
    }
    format!("'{}'", w.replace('\\', "\\\\").replace('\'', "\\'"))
}

/// One terminal's saved recipe.
#[derive(Debug, Clone, PartialEq)]
pub struct Recipe {
    pub id: String,
    pub cwd: String,
    pub argv: Vec<String>,
    /// The terminal's shell pid when it was written: a different one means its program was lost.
    pub shell: u64,
    /// The Claude process it was read from: another one in the pane makes it stale.
    pub agent: u64,
}

impl Recipe {
    pub fn label(&self) -> String {
        json!({"id": self.id, "cwd": self.cwd, "argv": self.argv, "shell": self.shell, "agent": self.agent}).to_string()
    }

    pub fn from_label(text: &str) -> Option<Recipe> {
        let v: Value = serde_json::from_str(text).ok()?;
        let id = v["id"].as_str().filter(|s| valid_id(s))?.to_owned();
        let cwd = v["cwd"].as_str().filter(|s| !s.is_empty())?.to_owned();
        let argv: Vec<String> = v["argv"].as_array()?.iter().map(|a| a.as_str().map(str::to_owned)).collect::<Option<_>>()?;
        // Labels are shared: anything another client could have put there is
        // checked again. Only what `clean_argv` itself would keep, no control
        // characters (they'd reach the shell as keys).
        if argv.first().map(String::as_str) != Some("claude") || clean_argv(&argv).as_ref() != Some(&argv) {
            return None;
        }
        if std::iter::once(&cwd).chain(&argv).any(|w| w.chars().any(char::is_control)) {
            return None;
        }
        Some(Recipe { id, cwd, argv, shell: v["shell"].as_u64()?, agent: v["agent"].as_u64()? })
    }
}

/// What to do for one terminal, from its core record.
#[derive(Debug, Clone, PartialEq)]
pub enum Step {
    /// Read Claude's session file for this foreground pid, then `capture`.
    Read { pid: u64 },
    /// The person quit the agent: remove the label.
    Clear,
    /// Its program was lost and a fresh shell is up: type this once, then relabel with the new pid.
    Resume { recipe: Recipe, shell: Shell },
}

/// The step for a local terminal record, if any.
pub fn step(record: &Value) -> Option<Step> {
    let address = record["address"].as_str()?;
    // Apollo's panes only (placed by Apollo or adopted): another client's kept terminal is theirs.
    let placed = record.pointer("/labels/herdr.space").and_then(Value::as_str).is_some_and(|s| !s.is_empty());
    if address.contains('/') || record["state"] != "live" || record["keep"] != true || !placed {
        return None;
    }
    let pid = record["pid"].as_u64()?;
    let fg = record.get("foreground").filter(|f| f.is_object())?;
    let recipe = record.pointer(&format!("/labels/{LABEL}")).and_then(Value::as_str).filter(|s| !s.is_empty());
    let argv: Vec<String> = fg["argv"].as_array().into_iter().flatten().filter_map(|a| a.as_str().map(str::to_owned)).collect();
    if clean_argv(&argv).is_some() {
        return fg["pid"].as_u64().map(|pid| Step::Read { pid });
    }
    // Only the terminal's own shell, idle at its prompt: not a nested shell, not a program.
    let shell = shell_kind(fg["name"].as_str().unwrap_or_default()).filter(|_| fg["pid"].as_u64() == Some(pid));
    let recipe = recipe.and_then(Recipe::from_label);
    match (recipe, shell) {
        (Some(r), Some(_)) if r.shell == pid => Some(Step::Clear),
        (Some(r), Some(shell)) => Some(Step::Resume { recipe: r, shell }),
        // A recipe in a terminal showing something else (cmd, an editor): leave it.
        _ => None,
    }
}

/// The label to write after reading the session file (None: unchanged;
/// `""`: remove it). Without a file, a recipe from another Claude process
/// is stale; one from this process stays (its file may be briefly unreadable).
pub fn capture(record: &Value, session: Option<&Session>) -> Option<String> {
    let fg = &record["foreground"];
    let agent = fg["pid"].as_u64()?;
    let old = record.pointer(&format!("/labels/{LABEL}")).and_then(Value::as_str).filter(|s| !s.is_empty());
    let Some(session) = session else {
        return old.filter(|o| Recipe::from_label(o).is_none_or(|r| r.agent != agent)).map(|_| String::new());
    };
    let argv: Vec<String> = fg["argv"].as_array().into_iter().flatten().filter_map(|a| a.as_str().map(str::to_owned)).collect();
    let recipe = Recipe { id: session.id.clone(), cwd: session.cwd.clone(), argv: clean_argv(&argv)?, shell: record["pid"].as_u64()?, agent };
    let label = recipe.label();
    // A recipe Apollo would refuse to read back (control characters) is never written.
    Recipe::from_label(&label)?;
    (old.and_then(Recipe::from_label) != Some(recipe)).then_some(label)
}

/// The resumes to type this round: one per conversation, in address order.
pub fn resumes<'a>(steps: impl IntoIterator<Item = (&'a str, &'a Step)>, done: &HashSet<String>) -> Vec<(String, Recipe, Shell)> {
    let mut picked: Vec<(String, Recipe, Shell)> = steps
        .into_iter()
        .filter_map(|(addr, s)| match s {
            Step::Resume { recipe, shell } if !done.contains(addr) => Some((addr.to_owned(), recipe.clone(), *shell)),
            _ => None,
        })
        .collect();
    picked.sort_by(|a, b| natural(&a.0).cmp(&natural(&b.0)));
    let mut seen = HashSet::new();
    picked.retain(|(_, r, _)| seen.insert(r.id.clone()));
    picked
}

/// `t9` before `t10`.
fn natural(addr: &str) -> (usize, String) {
    (addr.len(), addr.to_owned())
}

/// What passes share (the window keeps one).
#[derive(Default)]
pub struct State {
    /// Terminals and conversations being resumed right now.
    pub busy: HashSet<String>,
    /// Each conversation brought back this run, and the terminal it's in:
    /// that terminal may bring it back again (another crash), no other may.
    pub resumed: std::collections::HashMap<String, String>,
    /// Each terminal's shell pid as Apollo first saw it, and when: a new pid
    /// seen moments ago, after an old one, is a recovery Apollo watched.
    pub seen: std::collections::HashMap<String, (u64, std::time::Instant, bool)>,
    /// (terminal, shell pid) already offered to the person.
    pub offered: HashSet<(String, u64)>,
    /// The person asked (agent.resume): resume what was only offered.
    pub force: bool,
    /// (terminal, its shell pid) tried and not resumed (no transcript yet, a failed send):
    /// not again for a minute.
    pub tried: std::collections::HashMap<(String, u64), std::time::Instant>,
}

/// The terminals a quiet poll should tend: a resume or clear due, or a Claude
/// whose recipe is missing or names another Claude process. Not what's being
/// resumed, nor what was tried in the last minute.
pub fn due(records: &[Value], state: &State) -> Vec<String> {
    let mut out = Vec::new();
    for r in records {
        let (Some(address), Some(pid)) = (r["address"].as_str(), r["pid"].as_u64()) else { continue };
        if state.busy.contains(address) || state.tried.get(&(address.to_owned(), pid)).is_some_and(|t| t.elapsed() < std::time::Duration::from_secs(60)) {
            continue;
        }
        let wants = match step(r) {
            Some(Step::Resume { .. } | Step::Clear) => true,
            Some(Step::Read { pid: agent }) => {
                let label = r.pointer(&format!("/labels/{LABEL}")).and_then(Value::as_str);
                label.and_then(Recipe::from_label).is_none_or(|old| old.agent != agent)
            }
            None => false,
        };
        if wants {
            out.push(address.to_owned());
        }
    }
    out
}

/// What `tend` needs from the core: the real one, or a test's.
pub trait Calls {
    fn call(&self, method: &str, params: Value) -> Result<Value, String>;
}

impl Calls for crate::hc::Core {
    fn call(&self, method: &str, params: Value) -> Result<Value, String> {
        crate::hc::Core::call(self, method, params).map_err(|e| format!("{e:#}"))
    }
}

/// Where Claude keeps its files: this machine's home and `CLAUDE_CONFIG_DIR`.
pub struct Files {
    pub home: PathBuf,
    pub config_dir: Option<String>,
}

impl Files {
    pub fn here() -> Files {
        Files { home: crate::mods::home(), config_dir: std::env::var("CLAUDE_CONFIG_DIR").ok() }
    }
}

/// What a pass did.
#[derive(Debug, Default, PartialEq)]
pub struct Pass {
    /// Typed into: back in their conversations.
    pub resumed: Vec<String>,
    /// Ready, but Apollo didn't see them recover (opened after a reboot): the
    /// shell may hold someone's draft, so the person is asked first.
    pub offered: Vec<String>,
}

/// How soon after Apollo sees a recovered shell it may type into it.
const FRESH: std::time::Duration = std::time::Duration::from_secs(10);

/// One pass over the live terminals, off the UI thread: labels recipes,
/// drops stale ones, and resumes what's due, 500 ms apart. Apollo never
/// edits a prompt line: it types only into a shell whose recovery it
/// watched (moments ago), or after the person says so (`State::force`).
pub fn tend(core: &dyn Calls, files: &Files, records: &[Value], restore: &dyn Fn() -> bool, state: &std::sync::Mutex<State>) -> Pass {
    let now = std::time::Instant::now();
    let mut due = Vec::new();
    let mut pass = Pass::default();
    for record in records {
        let (Some(address), Some(pid)) = (record["address"].as_str(), record["pid"].as_u64()) else { continue };
        let witnessed = {
            let Ok(mut s) = state.lock() else { return pass };
            if s.busy.contains(address) || s.tried.get(&(address.to_owned(), pid)).is_some_and(|t| t.elapsed() < std::time::Duration::from_secs(60)) {
                continue;
            }
            // (this shell, when Apollo first saw it, whether it replaced one Apollo saw)
            let entry = match s.seen.get(address).copied() {
                Some((p, at, replaced)) if p == pid => (p, at, replaced),
                before => (pid, now, before.is_some()),
            };
            s.seen.insert(address.to_owned(), entry);
            entry.2 && entry.1.elapsed() < FRESH
        };
        match step(record) {
            Some(Step::Read { pid }) => {
                let text = std::fs::read_to_string(session_file(&files.home, files.config_dir.as_deref(), pid)).ok();
                let session = text.as_deref().and_then(parse_session);
                if let Some(label) = capture(record, session.as_ref()) {
                    set_label(core, address, &label);
                }
            }
            Some(Step::Clear) => {
                // The shell can flash into the foreground while Claude runs (a tool
                // run): it only quit when its session file is gone (removed on exit).
                let recipe = record.pointer(&format!("/labels/{LABEL}")).and_then(Value::as_str).and_then(Recipe::from_label);
                if recipe.is_some_and(|r| gone(&session_file(&files.home, files.config_dir.as_deref(), r.agent)) == Some(true)) {
                    set_label(core, address, "");
                }
            }
            Some(Step::Resume { recipe, shell }) => due.push((address.to_owned(), pid, witnessed, Step::Resume { recipe, shell })),
            None => {}
        }
    }
    let picked = resumes(due.iter().map(|(a, _, _, s)| (a.as_str(), s)), &HashSet::new());
    for (address, recipe, shell) in picked {
        let Some(&(_, pid, witnessed, _)) = due.iter().find(|(a, ..)| *a == address) else { continue };
        // Read again before each one: turning it off stops what's queued.
        if !restore() {
            break;
        }
        let session = format!("session:{}", recipe.id);
        {
            let Ok(mut s) = state.lock() else { break };
            if s.resumed.get(&recipe.id).is_some_and(|t| *t != address) {
                // Already back in another pane this run: no copy here. Its recipe
                // goes only if a fresh read still shows this one.
                drop(s);
                if let Ok(now) = core.call("terminal.get", json!({"terminal": address})) {
                    let still = now.pointer(&format!("/labels/{LABEL}")).and_then(Value::as_str).and_then(Recipe::from_label);
                    if still.as_ref() == Some(&recipe) {
                        set_label(core, &address, "");
                    }
                }
                continue;
            }
            if !witnessed && !s.force {
                if s.offered.insert((address.clone(), pid)) {
                    pass.offered.push(address.clone());
                }
                continue;
            }
            if s.busy.contains(&address) || !s.busy.insert(session.clone()) {
                continue;
            }
            s.busy.insert(address.clone());
            // Claimed before typing: a slow send can't let another pane take it.
            s.resumed.insert(recipe.id.clone(), address.clone());
        }
        let done = resume_one(core, files, &address, &recipe, shell);
        if let Ok(mut s) = state.lock() {
            s.busy.remove(&session);
            s.busy.remove(&address);
            if !done {
                s.tried.insert((address.clone(), pid), std::time::Instant::now());
                if s.resumed.get(&recipe.id) == Some(&address) && done_marked(core, &address, pid) == Some(false) {
                    // Nothing was marked or typed: this conversation is free again.
                    s.resumed.remove(&recipe.id);
                }
            }
        }
        if done {
            pass.resumed.push(address);
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
    }
    if let Ok(mut s) = state.lock() {
        s.force = false;
    }
    pass
}

/// Whether a terminal's recipe already names this shell (a resume was marked).
fn done_marked(core: &dyn Calls, address: &str, pid: u64) -> Option<bool> {
    let now = core.call("terminal.get", json!({"terminal": address})).ok()?;
    let recipe = now.pointer(&format!("/labels/{LABEL}")).and_then(Value::as_str).and_then(Recipe::from_label)?;
    Some(recipe.shell == pid)
}

/// Checks the terminal again right before typing, marks it, then types.
fn resume_one(core: &dyn Calls, files: &Files, address: &str, recipe: &Recipe, shell: Shell) -> bool {
    let Ok(now) = core.call("terminal.get", json!({"terminal": address})) else { return false };
    let Some(Step::Resume { recipe: again, shell: kind }) = step(&now) else { return false };
    if &again != recipe || kind != shell {
        return false;
    }
    let Some(pid) = now["pid"].as_u64() else { return false };
    match find_transcript(&files.home, files.config_dir.as_deref(), &recipe.cwd, &recipe.id) {
        Found::Yes => {}
        Found::No => {
            tracing::info!("resume: {address}'s conversation {} has no transcript; not resuming", recipe.id);
            set_label(core, address, "");
            return false;
        }
        Found::Unknown => return false,
    }
    // At most once: the new shell is marked before anything is typed.
    let marked = Recipe { shell: pid, ..recipe.clone() };
    if core.call("terminal.label", json!({"terminal": address, "labels": {LABEL: marked.label()}})).is_err() {
        return false;
    }
    // The core holds this while a person has an unfinished draft there.
    let line = resume_line(shell, &recipe.cwd, &recipe.argv, &recipe.id);
    match core.call("terminal.send", json!({"terminal": address, "text": line})) {
        Ok(_) => {
            tracing::info!("resume: {address} back in conversation {}", recipe.id);
            true
        }
        Err(e) => {
            // Marked already: never typed twice, even if the send went through late.
            tracing::warn!("resume: {address}: {e}");
            false
        }
    }
}

fn set_label(core: &dyn Calls, address: &str, value: &str) {
    if let Err(e) = core.call("terminal.label", json!({"terminal": address, "labels": {LABEL: value}})) {
        tracing::warn!("resume label for {address}: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|a| a.to_string()).collect()
    }

    const ID: &str = "9c9065a0-4ed1-4104-9e2c-1c906429aba3";

    #[test]
    fn session_files_give_interactive_conversations_only() {
        let file = format!(r#"{{"pid":11480,"sessionId":"{ID}","cwd":"/w/app","kind":"interactive","status":"busy"}}"#);
        assert_eq!(parse_session(&file), Some(Session { id: ID.into(), cwd: "/w/app".into() }));
        assert_eq!(parse_session(&file.replace("interactive", "bg")), None);
        assert_eq!(parse_session(&file.replace(r#""status""#, r#""spare":true,"status""#)), None);
        assert_eq!(parse_session(&file.replace(ID, "not-a-uuid; rm -rf ~")), None, "never typed into a shell");
        assert_eq!(parse_session("{"), None);
        // Older files without `kind` still count.
        assert!(parse_session(&format!(r#"{{"sessionId":"{ID}","cwd":"/w"}}"#)).is_some());
    }

    #[test]
    fn paths_follow_claudes_layout() {
        let home = Path::new("/Users/me");
        assert_eq!(session_file(home, None, 42), Path::new("/Users/me/.claude/sessions/42.json"));
        assert_eq!(session_file(home, Some("/cfg"), 42), Path::new("/cfg/sessions/42.json"));
        assert_eq!(transcript(home, None, "/Users/me/fun-code/apollo-app", ID), Path::new(&format!("/Users/me/.claude/projects/-Users-me-fun-code-apollo-app/{ID}.jsonl")));
        assert_eq!(transcript(home, None, r"C:\Users\me\app.x", ID).file_name().unwrap(), std::ffi::OsStr::new(&format!("{ID}.jsonl")));
        assert!(transcript(home, None, r"C:\Users\me\app.x", ID).to_string_lossy().contains("C--Users-me-app-x"));
    }

    #[test]
    fn argv_keeps_the_agents_flags_and_drops_the_conversation_and_first_message() {
        let argv = s(&["claude", "--permission-mode", "bypassPermissions", "--model", "claude-opus-5-5[1m]", "--effort", "xhigh", "--resume", ID]);
        assert_eq!(clean_argv(&argv).unwrap(), s(&["claude", "--permission-mode", "bypassPermissions", "--model", "claude-opus-5-5[1m]", "--effort", "xhigh"]));
        // Apollo starts agents with their task as the last word.
        assert_eq!(clean_argv(&s(&["claude", "--model", "opus", "fix the login bug"])).unwrap(), s(&["claude", "--model", "opus"]));
        assert_eq!(clean_argv(&s(&["claude", "-c", "--fork-session", "-r=abc", "--session-id", ID, "-p"])).unwrap(), s(&["claude"]));
        assert_eq!(clean_argv(&s(&["claude", "--add-dir", "/a", "/b", "--dangerously-skip-permissions", "hi"])).unwrap(), s(&["claude", "--add-dir", "/a", "/b", "--dangerously-skip-permissions"]));
        assert_eq!(clean_argv(&s(&["claude", "--model=opus", "--", "-weird prompt"])).unwrap(), s(&["claude", "--model=opus"]));
        // Windows: npm runs node with Claude's cli.js; native installs are claude.exe.
        assert_eq!(clean_argv(&s(&[r"C:\nodejs\node.exe", r"C:\npm\node_modules\@anthropic-ai\claude-code\cli.js", "--model", "opus"])).unwrap(), s(&["claude", "--model", "opus"]));
        assert_eq!(clean_argv(&s(&[r"C:\Users\me\.local\bin\claude.exe"])).unwrap(), s(&["claude"]));
        assert_eq!(clean_argv(&s(&["node", "server.js"])), None);
        // Only the program counts, never an argument naming it.
        assert_eq!(clean_argv(&s(&["vim", "notes/claude"])), None);
        assert_eq!(clean_argv(&s(&["less", "claude"])), None);
        assert_eq!(clean_argv(&s(&["node", "--inspect", "/x/claude", "--model", "opus"])).unwrap(), s(&["claude", "--model", "opus"]));
        assert_eq!(clean_argv(&s(&["/bin/sh", "/x/bin/claude", "--model", "opus"])).unwrap(), s(&["claude", "--model", "opus"]));
        assert_eq!(clean_argv(&s(&["claude-squad"])), None);
    }

    #[test]
    fn shells_are_known_by_how_they_quote() {
        assert_eq!(shell_kind("zsh"), Some(Shell::Posix));
        assert_eq!(shell_kind("-zsh"), Some(Shell::Posix));
        assert_eq!(shell_kind("/bin/bash"), Some(Shell::Posix));
        assert_eq!(shell_kind("fish"), Some(Shell::Fish));
        assert_eq!(shell_kind("pwsh.exe"), Some(Shell::PowerShell));
        assert_eq!(shell_kind("powershell.exe"), Some(Shell::PowerShell));
        assert_eq!(shell_kind("cmd.exe"), None, "cmd's quoting is skipped");
        assert_eq!(shell_kind("nu"), None);
        assert_eq!(shell_kind("claude"), None);
    }

    #[test]
    fn resume_lines_quote_for_their_shell() {
        let argv = s(&["claude", "--model", "claude-opus-5-5[1m]", "--append-system-prompt", "it's fine"]);
        assert_eq!(
            resume_line(Shell::Posix, "/Users/me/my app", &argv, ID),
            format!("cd '/Users/me/my app' && claude --model 'claude-opus-5-5[1m]' --append-system-prompt 'it'\\''s fine' --resume {ID}")
        );
        assert_eq!(
            resume_line(Shell::Fish, "/w", &s(&["claude", "--append-system-prompt", r"a\b 'c'"]), ID),
            format!(r"cd /w && claude --append-system-prompt 'a\\b \'c\'' --resume {ID}")
        );
        assert_eq!(
            resume_line(Shell::PowerShell, r"C:\Users\me\it's", &s(&["claude", "--model", "opus"]), ID),
            format!(r"Set-Location -LiteralPath 'C:\Users\me\it''s'; if ($?) {{ & 'claude' '--model' 'opus' '--resume' '{ID}' }}")
        );
    }

    #[test]
    fn recipes_round_trip_through_their_label_and_reject_junk() {
        let r = Recipe { id: ID.into(), cwd: "/w".into(), argv: s(&["claude", "--model", "opus"]), shell: 77, agent: 78 };
        assert_eq!(Recipe::from_label(&r.label()), Some(r.clone()));
        assert_eq!(Recipe::from_label(&r.label().replace(ID, "x")), None);
        assert_eq!(Recipe::from_label(&r.label().replace("\"claude\"", "\"rm\"")), None, "only Claude is ever resumed");
        assert_eq!(Recipe::from_label("not json"), None);
    }

    fn record(addr: &str, pid: u64, fg: Value, label: Option<&Recipe>) -> Value {
        let mut labels = json!({"herdr.space": "sp1"});
        if let Some(r) = label { labels[LABEL] = r.label().into(); }
        json!({"address": addr, "state": "live", "keep": true, "pid": pid, "foreground": fg, "labels": labels})
    }

    #[test]
    fn steps_capture_clear_and_resume_once() {
        let claude = json!({"pid": 500, "name": "claude", "argv": ["claude", "--model", "opus"], "cwd": "/w"});
        let zsh = json!({"pid": 100, "name": "zsh", "argv": ["-zsh"], "cwd": "/w"});
        let r = Recipe { id: ID.into(), cwd: "/w".into(), argv: s(&["claude", "--model", "opus"]), shell: 100, agent: 500 };

        // Running: read its session file and label it.
        let running = record("t1", 100, claude.clone(), None);
        assert_eq!(step(&running), Some(Step::Read { pid: 500 }));
        let session = Session { id: ID.into(), cwd: "/w".into() };
        let label = capture(&running, Some(&session)).unwrap();
        assert_eq!(Recipe::from_label(&label), Some(r.clone()));
        // Already labeled the same: no write (labels events would loop).
        assert_eq!(capture(&record("t1", 100, claude.clone(), Some(&r)), Some(&session)), None);

        // Quit: same shell pid back at the prompt.
        assert_eq!(step(&record("t1", 100, zsh.clone(), Some(&r))), Some(Step::Clear));
        // Lost: a new shell (pid 200) at the prompt.
        assert_eq!(step(&record("t1", 200, json!({"pid": 200, "name": "zsh", "argv": ["-zsh"], "cwd": "/w"}), Some(&r))), Some(Step::Resume { recipe: r.clone(), shell: Shell::Posix }));
        // Not yet: no foreground (between loss and the new shell), not kept, remote, finished, cmd.
        let mut pending = record("t1", 200, Value::Null, Some(&r));
        assert_eq!(step(&pending), None);
        pending["foreground"] = json!({"pid": 200, "name": "cmd.exe", "argv": ["cmd.exe"], "cwd": "/w"});
        assert_eq!(step(&pending), None);
        let fresh = json!({"pid": 200, "name": "zsh", "argv": ["-zsh"], "cwd": "/w"});
        let mut not_kept = record("t1", 200, fresh.clone(), Some(&r));
        not_kept["keep"] = false.into();
        assert_eq!(step(&not_kept), None);
        assert_eq!(step(&record("box/t1", 200, fresh.clone(), Some(&r))), None);
        let mut finished = record("t1", 200, fresh.clone(), Some(&r));
        finished["state"] = "finished".into();
        assert_eq!(step(&finished), None);
        // No recipe, a shell: nothing.
        assert_eq!(step(&record("t2", 100, zsh, None)), None);
    }

    /// Audit findings (spec audit, gpt-6.2-astra): ownership, stale recipes,
    /// trusted labels, and only an idle shell of the terminal's own.
    #[test]
    fn only_apollos_panes_with_their_own_idle_shell_resume() {
        let r = Recipe { id: ID.into(), cwd: "/w".into(), argv: s(&["claude"]), shell: 100, agent: 500 };
        let zsh = |pid: u64| json!({"pid": pid, "name": "zsh", "argv": ["-zsh"], "cwd": "/w"});
        // Another client's kept terminal (no Apollo placement): never touched.
        let mut foreign = record("t1", 200, zsh(200), Some(&r));
        foreign["labels"].as_object_mut().unwrap().remove("herdr.space");
        assert_eq!(step(&foreign), None);
        // A nested shell or a program in the fresh shell: wait, never type into it.
        assert_eq!(step(&record("t1", 200, json!({"pid": 333, "name": "bash", "argv": ["bash"], "cwd": "/w"}), Some(&r))), None);
        // Its own shell at the prompt: resume.
        assert!(matches!(step(&record("t1", 200, zsh(200), Some(&r))), Some(Step::Resume { .. })));
        // Same shell back at its prompt: the person quit.
        assert_eq!(step(&record("t1", 100, zsh(100), Some(&r))), Some(Step::Clear));
    }

    #[test]
    fn a_new_claude_in_the_pane_replaces_or_drops_the_old_recipe() {
        let old = Recipe { id: ID.into(), cwd: "/w".into(), argv: s(&["claude"]), shell: 100, agent: 500 };
        let claude = |pid: u64| json!({"pid": pid, "name": "claude", "argv": ["claude"], "cwd": "/w"});
        // Claude 500 still running, its file read failed this time: keep the recipe.
        let same = record("t1", 100, claude(500), Some(&old));
        assert_eq!(step(&same), Some(Step::Read { pid: 500 }));
        assert_eq!(capture(&same, None), None);
        // A new Claude (600) whose file isn't there yet: the old recipe is stale.
        let newer = record("t1", 100, claude(600), Some(&old));
        assert_eq!(capture(&newer, None), Some(String::new()), "remove the label");
        let fresh = Session { id: ID.replace('9', "7"), cwd: "/w".into() };
        let label = capture(&newer, Some(&fresh)).unwrap();
        assert_eq!(Recipe::from_label(&label).unwrap().agent, 600);
    }

    #[test]
    fn labels_are_checked_again_before_anything_is_typed() {
        let good = Recipe { id: ID.into(), cwd: "/w".into(), argv: s(&["claude", "--model", "opus"]), shell: 1, agent: 2 };
        assert!(Recipe::from_label(&good.label()).is_some());
        // Someone else wrote a first message, a picker or control characters into the label.
        for argv in [s(&["claude", "do something"]), s(&["claude", "--continue"]), s(&["claude", "--model", "a\u{1b}[2J"]), s(&["claude", "--model", "x\ny"])] {
            let bad = Recipe { argv, ..good.clone() };
            assert_eq!(Recipe::from_label(&bad.label()), None, "{:?}", bad.argv);
        }
        let bad_cwd = Recipe { cwd: "/w\r\nrm -rf ~".into(), ..good };
        assert_eq!(Recipe::from_label(&bad_cwd.label()), None);
    }

    #[test]
    fn inline_json_settings_are_not_copied_into_labels() {
        let argv = s(&["claude", "--settings", r#"{"apiKeyHelper":"x"}"#, "--mcp-config", "/m.json", r#"{"mcpServers":{}}"#, "--agents", r#"{"a":{}}"#, "--model", "opus"]);
        assert_eq!(clean_argv(&argv).unwrap(), s(&["claude", "--mcp-config", "/m.json", "--model", "opus"]));
        assert_eq!(clean_argv(&s(&["claude", "--settings=/s.json", "--settings={\"a\":1}"])).unwrap(), s(&["claude", "--settings=/s.json"]));
    }

    /// A core that answers from a table and records what was asked.
    struct FakeCore {
        records: std::sync::Mutex<HashMap<String, Value>>,
        calls: std::sync::Mutex<Vec<(String, Value)>>,
    }

    impl Calls for FakeCore {
        fn call(&self, method: &str, params: Value) -> Result<Value, String> {
            self.calls.lock().unwrap().push((method.to_owned(), params.clone()));
            let address = params["terminal"].as_str().unwrap_or_default().to_owned();
            let mut records = self.records.lock().unwrap();
            match method {
                "terminal.get" => records.get(&address).cloned().ok_or_else(|| "not_found".into()),
                "terminal.label" => {
                    let r = records.get_mut(&address).ok_or("not_found")?;
                    for (k, v) in params["labels"].as_object().unwrap() {
                        if v == "" { r["labels"].as_object_mut().unwrap().remove(k); } else { r["labels"][k] = v.clone(); }
                    }
                    Ok(json!({}))
                }
                "terminal.send" => Ok(json!({})),
                _ => Err(format!("unexpected {method}")),
            }
        }
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("apollo-resume-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_lost_claude_pane_is_captured_then_resumed_once() {
        let home = scratch("once");
        let files = Files { home: home.clone(), config_dir: None };
        std::fs::create_dir_all(home.join(".claude/sessions")).unwrap();
        std::fs::write(home.join(".claude/sessions/500.json"), format!(r#"{{"sessionId":"{ID}","cwd":"/w/my app","kind":"interactive"}}"#)).unwrap();
        let running = record("t1", 100, json!({"pid": 500, "name": "claude", "argv": ["claude", "--model", "opus", "first task"], "cwd": "/w/my app"}), None);
        let core = FakeCore { records: std::sync::Mutex::new([("t1".to_owned(), running.clone())].into()), calls: Default::default() };
        let busy = std::sync::Mutex::new(State::default());

        // While Claude runs: labeled.
        assert_eq!(tend(&core, &files, &[running], &|| true, &busy), Pass::default());
        let labeled = core.records.lock().unwrap()["t1"].clone();
        let recipe = Recipe::from_label(labeled["labels"][LABEL].as_str().unwrap()).unwrap();
        assert_eq!(recipe, Recipe { id: ID.into(), cwd: "/w/my app".into(), argv: s(&["claude", "--model", "opus"]), shell: 100, agent: 500 });

        // Reboot: a new shell 200 at its prompt. Claude's projects folder can't be
        // read: not resumed now, the recipe stays (tried again in a minute).
        let mut back = labeled.clone();
        back["pid"] = 200.into();
        back["foreground"] = json!({"pid": 200, "name": "zsh", "argv": ["-zsh"], "cwd": "/w/my app"});
        core.records.lock().unwrap().insert("t1".into(), back.clone());
        assert!(tend(&core, &files, &[back.clone()], &|| true, &busy).resumed.is_empty());
        assert!(core.records.lock().unwrap()["t1"]["labels"].get(LABEL).is_some());
        assert!(due(&[back.clone()], &busy.lock().unwrap()).is_empty(), "not again right away");
        // The folder is there but the conversation isn't: the recipe goes.
        std::fs::create_dir_all(home.join(".claude/projects/elsewhere")).unwrap();
        busy.lock().unwrap().tried.clear();
        assert!(tend(&core, &files, &[back.clone()], &|| true, &busy).resumed.is_empty());
        assert!(core.records.lock().unwrap()["t1"]["labels"].get(LABEL).is_none());
        core.records.lock().unwrap().insert("t1".into(), back.clone());

        // With the transcript: typed once, marked with the new shell first.
        let t = transcript(&home, None, "/w/my app", ID);
        std::fs::create_dir_all(t.parent().unwrap()).unwrap();
        std::fs::write(&t, "{}\n").unwrap();
        core.records.lock().unwrap().insert("t1".into(), back.clone());
        core.calls.lock().unwrap().clear();
        busy.lock().unwrap().tried.clear();
        assert_eq!(tend(&core, &files, &[back.clone()], &|| true, &busy).resumed, ["t1"]);
        let calls = core.calls.lock().unwrap().clone();
        let sends: Vec<&Value> = calls.iter().filter(|(m, _)| m == "terminal.send").map(|(_, p)| p).collect();
        assert_eq!(sends.len(), 1);
        assert_eq!(sends[0]["text"], format!("cd '/w/my app' && claude --model opus --resume {ID}"));
        let order: Vec<&str> = calls.iter().map(|(m, _)| m.as_str()).collect();
        assert_eq!(order, ["terminal.get", "terminal.label", "terminal.send"], "marked, then typed; the line is never edited");

        // The next pass sees the marked label: same shell at its prompt means nothing to resume.
        let after = core.records.lock().unwrap()["t1"].clone();
        assert_eq!(Recipe::from_label(after["labels"][LABEL].as_str().unwrap()).unwrap().shell, 200);
        assert_eq!(tend(&core, &files, &[after], &|| true, &busy), Pass::default());
        assert_eq!(core.calls.lock().unwrap().iter().filter(|(m, _)| m == "terminal.send").count(), 1);
        std::fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn nothing_is_typed_when_turned_off_or_when_the_pane_changed_meanwhile() {
        let home = scratch("off");
        let files = Files { home: home.clone(), config_dir: None };
        let t = transcript(&home, None, "/w", ID);
        std::fs::create_dir_all(t.parent().unwrap()).unwrap();
        std::fs::write(&t, "{}\n").unwrap();
        let r = Recipe { id: ID.into(), cwd: "/w".into(), argv: s(&["claude"]), shell: 100, agent: 500 };
        let back = record("t1", 200, json!({"pid": 200, "name": "zsh", "argv": ["-zsh"], "cwd": "/w"}), Some(&r));
        let core = FakeCore { records: std::sync::Mutex::new([("t1".to_owned(), back.clone())].into()), calls: Default::default() };
        let busy = std::sync::Mutex::new(State { force: true, ..State::default() });
        assert!(tend(&core, &files, &[back.clone()], &|| false, &busy).resumed.is_empty(), "restore_agents: false");
        // The person started something in the pane after the listing.
        let mut changed = back.clone();
        changed["foreground"] = json!({"pid": 300, "name": "vim", "argv": ["vim"], "cwd": "/w"});
        core.records.lock().unwrap().insert("t1".into(), changed);
        busy.lock().unwrap().force = true;
        assert!(tend(&core, &files, &[back], &|| true, &busy).resumed.is_empty());
        assert!(core.calls.lock().unwrap().iter().all(|(m, _)| m != "terminal.send"));
        std::fs::remove_dir_all(home).unwrap();
    }

    /// The whole path against a real core: a kept zsh running a stand-in
    /// `claude`, captured; the core stopped and started (its kept terminal
    /// gets a new shell, as after a reboot); the resume typed once into it.
    #[test]
    #[ignore = "requires an isolated HC_SESSION=test-* and a running hc on PATH; stops and starts that core"]
    fn a_real_core_restart_brings_claude_back_in_its_conversation() {
        assert!(std::env::var("HC_SESSION").unwrap().starts_with("test-"));
        let root = PathBuf::from(std::env::var_os("XDG_STATE_HOME").unwrap());
        let (home, bin) = (root.join("home"), root.join("bin"));
        std::fs::create_dir_all(home.join(".claude/sessions")).unwrap();
        std::fs::create_dir_all(&bin).unwrap();
        // The stand-in writes Claude's session file for its own pid, then waits.
        let fake = format!(
            "#!/bin/sh\nif [ -n \"$RESUMED\" ] || printf '%s\\n' \"$@\" | grep -qx -- --resume; then echo \"resumed: $*\" > {r}/resumed; fi\n\
             printf '{{\"sessionId\":\"{ID}\",\"cwd\":\"%s\",\"kind\":\"interactive\"}}' \"$PWD\" > {h}/.claude/sessions/$$.json\nsleep 600 & wait\n",
            r = root.display(), h = home.display());
        std::fs::write(bin.join("claude"), fake).unwrap();
        #[cfg(unix)] { use std::os::unix::fs::PermissionsExt; std::fs::set_permissions(bin.join("claude"), std::fs::Permissions::from_mode(0o755)).unwrap(); }
        let work = root.join("my work");
        std::fs::create_dir_all(&work).unwrap();
        let t = transcript(&home, None, &work.to_string_lossy(), ID);
        std::fs::create_dir_all(t.parent().unwrap()).unwrap();
        std::fs::write(&t, "{}\n").unwrap();
        let files = Files { home: home.clone(), config_dir: None };
        let busy = std::sync::Mutex::new(State::default());
        let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap());

        let core = crate::hc::Core::connect().unwrap();
        let ran = core.call("terminal.run", json!({"argv": ["/bin/zsh", "-f", "-i"], "cwd": work, "keep": true,
            "env": {"PATH": path, "HOME": home}, "labels": {"herdr.space": "sp1"}})).unwrap();
        let t1 = ran["address"].as_str().unwrap().to_owned();
        let get = |core: &crate::hc::Core| core.call("terminal.get", json!({"terminal": t1})).unwrap();
        let wait = |what: &str, ok: &dyn Fn() -> bool| {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
            while !ok() { assert!(std::time::Instant::now() < deadline, "timed out: {what}"); std::thread::sleep(std::time::Duration::from_millis(200)); }
        };
        let foreground_is = |core: &crate::hc::Core, name: &str| get(core)["foreground"]["name"].as_str().is_some_and(|n| n == name);
        wait("zsh", &|| foreground_is(&core, "zsh"));
        core.call("terminal.send", json!({"terminal": t1, "text": "claude --model opus 'first task'"})).unwrap();
        let runs_claude = |core: &crate::hc::Core| get(core)["foreground"]["argv"].as_array().is_some_and(|a| a.iter().any(|w| w.as_str().is_some_and(|w| w.ends_with("/claude"))));
        wait("claude running", &|| runs_claude(&core));
        wait("session file", &|| std::fs::read_dir(home.join(".claude/sessions")).unwrap().count() > 0);
        assert_eq!(tend(core.as_ref(), &files, &[get(&core)], &|| true, &busy), Pass::default());
        let label = get(&core)["labels"][LABEL].as_str().unwrap().to_owned();
        let recipe = Recipe::from_label(&label).unwrap();
        assert_eq!(recipe.argv, s(&["claude", "--model", "opus"]));
        assert_eq!(recipe.cwd, work.to_string_lossy());
        let before = get(&core)["pid"].as_u64().unwrap();
        drop(core);

        // "Reboot": stop parks kept terminals; the next start gives them shells.
        let hc = |arg: &str| assert!(std::process::Command::new(crate::core_bin::path()).args(["server", arg]).status().unwrap().success(), "hc server {arg}");
        hc("stop");
        hc("start");
        let core = crate::hc::Core::connect().unwrap();
        wait("a new shell", &|| get(&core)["pid"].as_u64().is_some_and(|p| p != before) && foreground_is(&core, "zsh"));
        assert_eq!(Recipe::from_label(get(&core)["labels"][LABEL].as_str().unwrap()), Some(recipe.clone()), "the label survived the restart");

        assert_eq!(tend(core.as_ref(), &files, &[get(&core)], &|| true, &busy).resumed, [t1.clone()], "a recovery Apollo watched: typed without asking");
        wait("resumed", &|| root.join("resumed").exists());
        let line = std::fs::read_to_string(root.join("resumed")).unwrap();
        assert_eq!(line.trim(), format!("resumed: --model opus --resume {ID}"));
        // Never twice: the next pass sees the marked shell.
        std::fs::remove_file(root.join("resumed")).unwrap();
        assert_eq!(tend(core.as_ref(), &files, &[get(&core)], &|| true, &busy), Pass::default());
        let _ = core.call("terminal.kill", json!({"terminal": t1}));
    }

    /// QA findings (gpt-6.2-astra, round 1).
    #[test]
    fn single_value_options_dont_swallow_the_first_message() {
        assert_eq!(clean_argv(&s(&["claude", "--plugin-dir", "/plugins", "first task"])).unwrap(), s(&["claude", "--plugin-dir", "/plugins"]));
        assert_eq!(clean_argv(&s(&["claude", "--system-prompt-file", "/p.md", "hi"])).unwrap(), s(&["claude", "--system-prompt-file", "/p.md"]));
        assert_eq!(clean_argv(&s(&["claude", "--append-system-prompt-file", "/a.md"])).unwrap(), s(&["claude", "--append-system-prompt-file", "/a.md"]));
    }

    #[test]
    fn ids_are_real_uuids_and_windows_names_any_case() {
        assert!(valid_id(ID));
        assert!(!valid_id("------------------------------------"));
        assert!(!valid_id(&ID.replace('-', "a")));
        assert!(!valid_id("9c9065a04-ed1-4104-9e2c-1c906429aba3"));
        assert_eq!(clean_argv(&s(&[r"C:\Tools\Claude.Exe"])).unwrap(), s(&["claude"]));
        assert_eq!(shell_kind(r"C:\Program Files\PowerShell\7\PWSH.EXE"), Some(Shell::PowerShell));
    }

    #[test]
    fn transcripts_are_found_wherever_claude_put_them() {
        let home = scratch("find");
        // Claude shortens long folder names; the id is unique, so look in every project.
        let folder = home.join(".claude/projects/-long-name-shortened-abc123");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join(format!("{ID}.jsonl")), "{}\n").unwrap();
        assert_eq!(find_transcript(&home, None, "/a/very/long/path", ID), Found::Yes);
        assert_eq!(find_transcript(&home, None, "/a/very/long/path", &ID.replace('9', "8")), Found::No);
        // No projects folder at all: can't tell, so never drop the recipe for it.
        let empty = scratch("find-empty");
        assert_eq!(find_transcript(&empty, None, "/w", ID), Found::Unknown);
        std::fs::remove_dir_all(home).unwrap();
        std::fs::remove_dir_all(empty).unwrap();
    }

    #[test]
    fn the_poll_retries_due_work_but_not_what_it_just_tried() {
        let r = Recipe { id: ID.into(), cwd: "/w".into(), argv: s(&["claude"]), shell: 100, agent: 500 };
        let lost = record("t1", 200, json!({"pid": 200, "name": "zsh", "argv": ["-zsh"], "cwd": "/w"}), Some(&r));
        let quiet = record("t2", 300, json!({"pid": 300, "name": "zsh", "argv": ["-zsh"], "cwd": "/w"}), None);
        let mut state = State::default();
        assert!(due(&[quiet.clone()], &state).is_empty(), "nothing to do");
        assert_eq!(due(&[quiet.clone(), lost.clone()], &state), ["t1"]);
        state.tried.insert(("t1".into(), 200), std::time::Instant::now());
        assert!(due(&[lost.clone()], &state).is_empty(), "tried this shell a moment ago");
        // A Claude whose recipe names another Claude process is due (stale recipe).
        let stale = record("t3", 100, json!({"pid": 600, "name": "claude", "argv": ["claude"], "cwd": "/w"}), Some(&r));
        assert_eq!(due(&[stale], &State::default()), ["t3"]);
        let current = record("t4", 100, json!({"pid": 500, "name": "claude", "argv": ["claude"], "cwd": "/w"}), Some(&r));
        assert!(due(&[current], &State::default()).is_empty(), "recipe already matches this Claude");
    }

    #[test]
    fn a_conversation_comes_back_in_one_pane_only_and_a_live_claude_is_never_cleared() {
        let home = scratch("dup");
        let files = Files { home: home.clone(), config_dir: None };
        let t = transcript(&home, None, "/w", ID);
        std::fs::create_dir_all(t.parent().unwrap()).unwrap();
        std::fs::write(&t, "{}\n").unwrap();
        let r = Recipe { id: ID.into(), cwd: "/w".into(), argv: s(&["claude"]), shell: 100, agent: 500 };
        let a = record("t1", 200, json!({"pid": 200, "name": "zsh", "argv": ["-zsh"], "cwd": "/w"}), Some(&r));
        let b = record("t2", 201, json!({"pid": 201, "name": "zsh", "argv": ["-zsh"], "cwd": "/w"}), Some(&r));
        let core = FakeCore { records: std::sync::Mutex::new([("t1".to_owned(), a.clone()), ("t2".to_owned(), b.clone())].into()), calls: Default::default() };
        let state = std::sync::Mutex::new(State::default());
        state.lock().unwrap().force = true;
        assert_eq!(tend(&core, &files, &[a.clone()], &|| true, &state).resumed, ["t1"]);
        // A later pass sees t2 with the same conversation: no second copy; its recipe goes.
        state.lock().unwrap().force = true;
        assert!(tend(&core, &files, &[b.clone()], &|| true, &state).resumed.is_empty());
        assert_eq!(core.calls.lock().unwrap().iter().filter(|(m, _)| m == "terminal.send").count(), 1);
        assert!(core.records.lock().unwrap()["t2"]["labels"].get(LABEL).is_none());

        // The shell flashed into the foreground while Claude (500) still runs: keep the recipe.
        std::fs::create_dir_all(home.join(".claude/sessions")).unwrap();
        std::fs::write(home.join(".claude/sessions/500.json"), "{}").unwrap();
        let flash = record("t3", 100, json!({"pid": 100, "name": "zsh", "argv": ["-zsh"], "cwd": "/w"}), Some(&r));
        core.records.lock().unwrap().insert("t3".into(), flash.clone());
        tend(&core, &files, &[flash.clone()], &|| true, &state);
        assert!(core.records.lock().unwrap()["t3"]["labels"].get(LABEL).is_some(), "Claude's file is there: not quit");
        std::fs::remove_file(home.join(".claude/sessions/500.json")).unwrap();
        tend(&core, &files, &[flash], &|| true, &state);
        assert!(core.records.lock().unwrap()["t3"]["labels"].get(LABEL).is_none(), "gone: the person quit");
        std::fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn turning_it_off_stops_queued_sends() {
        let home = scratch("clear");
        let files = Files { home: home.clone(), config_dir: None };
        let t = transcript(&home, None, "/w", ID);
        std::fs::create_dir_all(t.parent().unwrap()).unwrap();
        std::fs::write(&t, "{}\n").unwrap();
        let other = ID.replace('9', "8");
        let t2 = transcript(&home, None, "/w", &other);
        std::fs::write(&t2, "{}\n").unwrap();
        let r = Recipe { id: ID.into(), cwd: "/w".into(), argv: s(&["claude"]), shell: 100, agent: 500 };
        let a = record("t1", 200, json!({"pid": 200, "name": "pwsh.exe", "argv": ["pwsh.exe"], "cwd": "/w"}), Some(&r));
        let b = record("t2", 201, json!({"pid": 201, "name": "zsh", "argv": ["-zsh"], "cwd": "/w"}), Some(&Recipe { id: other, ..r.clone() }));
        let core = FakeCore { records: std::sync::Mutex::new([("t1".to_owned(), a.clone()), ("t2".to_owned(), b.clone())].into()), calls: Default::default() };
        let state = std::sync::Mutex::new(State { force: true, ..State::default() });
        let on = std::sync::atomic::AtomicBool::new(true);
        // Turned off after the first send: the second never goes.
        let restore = || on.swap(false, std::sync::atomic::Ordering::SeqCst);
        assert_eq!(tend(&core, &files, &[a, b], &restore, &state).resumed, ["t1"]);
        let sent = sends(&core);
        assert_eq!(sent.len(), 1);
        assert!(sent[0].1.starts_with("Set-Location -LiteralPath '/w'; if ($?) { & 'claude'"), "{}", sent[0].1);
        std::fs::remove_dir_all(home).unwrap();
    }

    /// QA findings (gpt-6.2-astra, round 2).
    #[test]
    fn unknown_options_go_with_their_value_known_flags_stay() {
        assert_eq!(clean_argv(&s(&["claude", "--advisor", "opus", "--verbose", "task"])).unwrap(), s(&["claude", "--verbose"]));
        assert_eq!(clean_argv(&s(&["claude", "--advisor=opus", "--bare"])).unwrap(), s(&["claude", "--bare"]));
        assert_eq!(clean_argv(&s(&["claude", "-d", "api", "--safe-mode"])).unwrap(), s(&["claude", "-d", "api", "--safe-mode"]));
        assert_eq!(clean_argv(&s(&["claude", "--worktree", "feat", "--tmux", "--remote-control", "x", "--model", "opus"])).unwrap(), s(&["claude", "--model", "opus"]));
        assert_eq!(shell_kind("PowerShell.Exe"), Some(Shell::PowerShell));
    }

    fn fixture(name: &str) -> (PathBuf, Files) {
        let home = scratch(name);
        let files = Files { home: home.clone(), config_dir: None };
        for id in [ID.to_owned(), ID.replace('9', "8")] {
            let t = transcript(&home, None, "/w", &id);
            std::fs::create_dir_all(t.parent().unwrap()).unwrap();
            std::fs::write(&t, "{}\n").unwrap();
        }
        (home, files)
    }

    fn sends(core: &FakeCore) -> Vec<(String, String)> {
        core.calls.lock().unwrap().iter().filter(|(m, _)| m == "terminal.send").map(|(_, p)| (p["terminal"].as_str().unwrap().to_owned(), p["text"].as_str().unwrap().to_owned())).collect()
    }

    #[test]
    fn a_recovery_apollo_saw_resumes_one_it_didnt_is_offered_first() {
        let (home, files) = fixture("seen");
        let r = Recipe { id: ID.into(), cwd: "/w".into(), argv: s(&["claude"]), shell: 100, agent: 500 };
        let running = record("t1", 100, json!({"pid": 500, "name": "claude", "argv": ["claude"], "cwd": "/w"}), Some(&r));
        let back = record("t1", 200, json!({"pid": 200, "name": "zsh", "argv": ["-zsh"], "cwd": "/w"}), Some(&r));
        let core = FakeCore { records: std::sync::Mutex::new([("t1".to_owned(), back.clone())].into()), calls: Default::default() };

        // Apollo saw shell 100, then 200 appeared: nobody could have typed yet. Resumed, nothing edited.
        let state = std::sync::Mutex::new(State::default());
        tend(&core, &files, &[running.clone()], &|| true, &state);
        let pass = tend(&core, &files, &[back.clone()], &|| true, &state);
        assert_eq!((pass.resumed.clone(), pass.offered.clone()), (vec!["t1".to_owned()], vec![]));
        let order: Vec<String> = core.calls.lock().unwrap().iter().map(|(m, _)| m.clone()).collect();
        assert!(!order.iter().any(|m| m == "terminal.keys"), "never edits the prompt line");

        // Apollo opened after the reboot: the shell may hold someone's draft. Offered, not typed.
        let core = FakeCore { records: std::sync::Mutex::new([("t1".to_owned(), back.clone())].into()), calls: Default::default() };
        let state = std::sync::Mutex::new(State::default());
        let pass = tend(&core, &files, &[back.clone()], &|| true, &state);
        assert_eq!((pass.resumed.clone(), pass.offered.clone()), (vec![], vec!["t1".to_owned()]));
        assert!(sends(&core).is_empty());
        // Offered once per shell.
        assert!(tend(&core, &files, &[back.clone()], &|| true, &state).offered.is_empty());
        // The person said yes (agent.resume): typed then.
        state.lock().unwrap().force = true;
        assert_eq!(tend(&core, &files, &[back.clone()], &|| true, &state).resumed, ["t1"]);
        assert_eq!(sends(&core).len(), 1);
        std::fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn a_second_crash_brings_the_same_pane_back_again_and_never_into_another() {
        let (home, files) = fixture("again");
        let r = Recipe { id: ID.into(), cwd: "/w".into(), argv: s(&["claude"]), shell: 100, agent: 500 };
        let core = FakeCore { records: Default::default(), calls: Default::default() };
        let state = std::sync::Mutex::new(State::default());
        let at = |pid: u64, fg: u64, recipe: &Recipe| {
            let (name, argv) = if pid == fg { ("zsh", "-zsh") } else { ("claude", "claude") };
            record("t1", pid, json!({"pid": fg, "name": name, "argv": [argv], "cwd": "/w"}), Some(recipe))
        };
        let put = |rec: &Value| { core.records.lock().unwrap().insert(rec["address"].as_str().unwrap().into(), rec.clone()); rec.clone() };
        std::fs::create_dir_all(home.join(".claude/sessions")).unwrap();
        for pid in [500, 600] {
            std::fs::write(home.join(format!(".claude/sessions/{pid}.json")), format!(r#"{{"sessionId":"{ID}","cwd":"/w","kind":"interactive"}}"#)).unwrap();
        }
        tend(&core, &files, &[put(&at(100, 500, &r))], &|| true, &state);
        assert_eq!(tend(&core, &files, &[put(&at(200, 200, &r))], &|| true, &state).resumed, ["t1"]);
        // Claude runs again (recaptured with shell 200), then the core crashes again.
        let r2 = Recipe { shell: 200, agent: 600, ..r.clone() };
        tend(&core, &files, &[put(&at(200, 600, &r2))], &|| true, &state);
        assert_eq!(tend(&core, &files, &[put(&at(300, 300, &r2))], &|| true, &state).resumed, ["t1"], "same pane, same conversation: again");
        // Another pane with that conversation: never a copy; its recipe goes only if it's still that one.
        let other = put(&record("t2", 400, json!({"pid": 400, "name": "zsh", "argv": ["-zsh"], "cwd": "/w"}), Some(&r)));
        state.lock().unwrap().force = true;
        assert!(tend(&core, &files, &[other], &|| true, &state).resumed.is_empty());
        assert_eq!(sends(&core).iter().filter(|(t, _)| t == "t2").count(), 0);
        assert!(core.records.lock().unwrap()["t2"]["labels"].get(LABEL).is_none());
        std::fs::remove_dir_all(home).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn unreadable_is_not_gone() {
        use std::os::unix::fs::PermissionsExt;
        let home = scratch("perm");
        let sessions = home.join(".claude/sessions");
        std::fs::create_dir_all(&sessions).unwrap();
        std::fs::write(sessions.join("500.json"), "{}").unwrap();
        std::fs::set_permissions(&sessions, std::fs::Permissions::from_mode(0o000)).unwrap();
        assert_eq!(gone(&sessions.join("500.json")), None, "can't tell");
        std::fs::set_permissions(&sessions, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(gone(&sessions.join("500.json")), Some(false));
        assert_eq!(gone(&sessions.join("501.json")), Some(true));
        std::fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn listings_wait_out_a_failed_try_too() {
        let (home, files) = fixture("wait");
        let r = Recipe { id: ID.into(), cwd: "/w".into(), argv: s(&["claude"]), shell: 100, agent: 500 };
        let back = record("t1", 200, json!({"pid": 200, "name": "zsh", "argv": ["-zsh"], "cwd": "/w"}), Some(&r));
        // A core that has no such terminal any more: the re-check fails.
        let core = FakeCore { records: Default::default(), calls: Default::default() };
        let state = std::sync::Mutex::new(State { force: true, ..State::default() });
        assert!(tend(&core, &files, &[back.clone()], &|| true, &state).resumed.is_empty());
        let asked = core.calls.lock().unwrap().len();
        assert!(tend(&core, &files, &[back], &|| true, &state).resumed.is_empty());
        assert_eq!(core.calls.lock().unwrap().len(), asked, "nothing asked again within the minute");
        std::fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn one_resume_per_conversation_and_never_twice() {
        let r = Recipe { id: ID.into(), cwd: "/w".into(), argv: s(&["claude"]), shell: 1, agent: 2 };
        let other = Recipe { id: ID.replace('9', "8"), ..r.clone() };
        let steps = [
            ("t10".to_owned(), Step::Resume { recipe: r.clone(), shell: Shell::Posix }),
            ("t9".to_owned(), Step::Resume { recipe: r.clone(), shell: Shell::Posix }),
            ("t11".to_owned(), Step::Resume { recipe: other.clone(), shell: Shell::Fish }),
            ("t12".to_owned(), Step::Clear),
        ];
        let picked = resumes(steps.iter().map(|(a, s)| (a.as_str(), s)), &HashSet::new());
        assert_eq!(picked.iter().map(|p| p.0.as_str()).collect::<Vec<_>>(), ["t9", "t11"]);
        let done: HashSet<String> = ["t9".to_owned()].into();
        let picked = resumes(steps.iter().map(|(a, s)| (a.as_str(), s)), &done);
        assert_eq!(picked.iter().map(|p| p.0.as_str()).collect::<Vec<_>>(), ["t10", "t11"]);
    }
}
