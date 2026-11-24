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
    id.len() == 36 && id.chars().all(|c| c.is_ascii_hexdigit() || c == '-')
}

/// Claude's options that take a value: kept with it when cleaning argv.
const TAKES_VALUE: &[&str] = &[
    "--model", "--permission-mode", "--effort", "--agent", "--agents", "--append-system-prompt", "--system-prompt",
    "--settings", "--setting-sources", "--fallback-model", "--permission-prompt-tool", "--permission-prompts",
    "--max-budget-usd", "--name", "-n", "--plugin-url", "--remote-control-session-name-prefix", "--debug-file",
    "--input-format", "--output-format", "--json-schema", "--autocompact", "--system-prompt-snapshot",
];
/// Options that take one or more values (a run up to the next option).
const TAKES_VALUES: &[&str] = &[
    "--add-dir", "--allowedTools", "--allowed-tools", "--disallowedTools", "--disallowed-tools", "--tools",
    "--mcp-config", "--betas", "--file", "--plugin-dir",
];
/// Dropped with their value: a resume picks its own conversation.
const DROP_WITH_VALUE: &[&str] = &["--resume", "-r", "--session-id", "--from-pr", "--teleport", "--cloud", "--environment"];
/// Options whose value may be inline JSON (secrets, servers): kept only as a file path.
const JSON_OR_FILE: &[&str] = &["--settings", "--mcp-config", "--agents", "--json-schema"];
/// Dropped alone: they start another conversation or don't stay interactive.
const DROP: &[&str] = &["--continue", "-c", "--fork-session", "--print", "-p", "--bg", "--background"];

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
        } else if TAKES_VALUE.contains(&name) {
            let value = if inline { a.split_once('=').map(|(_, v)| v.to_owned()) } else { next_is_value(i + 1).then(|| argv[i + 1].clone()) };
            let skip = JSON_OR_FILE.contains(&name) && value.as_deref().is_some_and(inline_json);
            if !skip {
                out.push(a.clone());
                if !inline { out.extend(value.clone()); }
            }
            i += if !inline && value.is_some() { 2 } else { 1 };
        } else {
            out.push(a.clone());
            i += 1;
        }
    }
    Some(out)
}

/// Where the program is: argv[0], or the script an interpreter runs
/// (`node .../cli.js`, `sh .../claude`). Never an argument (`vim claude`).
fn program_at(argv: &[String]) -> Option<usize> {
    const INTERPRETERS: &[&str] = &["node", "bun", "deno", "sh", "bash", "zsh", "dash"];
    let base = |a: &str| {
        let b = a.rsplit(['/', '\\']).next().unwrap_or(a);
        b.strip_suffix(".exe").or_else(|| b.strip_suffix(".EXE")).unwrap_or(b).to_owned()
    };
    let first = argv.first()?;
    if INTERPRETERS.contains(&base(first).as_str()) {
        return argv.iter().skip(1).position(|a| !a.starts_with('-')).map(|i| i + 1);
    }
    Some(0)
}

/// `claude`, `claude.exe`, or npm's `.../@anthropic-ai/claude-code/cli.js` run by node.
fn is_claude(arg: &str) -> bool {
    let base = arg.rsplit(['/', '\\']).next().unwrap_or(arg);
    let base = base.strip_suffix(".exe").or_else(|| base.strip_suffix(".EXE")).unwrap_or(base);
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
    let base = name.rsplit(['/', '\\']).next().unwrap_or(name).trim_start_matches('-');
    let base = base.strip_suffix(".exe").or_else(|| base.strip_suffix(".EXE")).unwrap_or(base).to_ascii_lowercase();
    match base.as_str() {
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

/// One pass over the live terminals, off the UI thread: labels recipes,
/// drops stale ones, and types each due resume once, 500 ms apart.
/// `busy` holds terminals and conversations being resumed (shared with the
/// next pass, so neither is resumed twice); returns the addresses resumed.
pub fn tend(core: &dyn Calls, files: &Files, records: &[Value], restore: bool, busy: &std::sync::Mutex<HashSet<String>>) -> Vec<String> {
    let mut due = Vec::new();
    for record in records {
        let Some(address) = record["address"].as_str() else { continue };
        if busy.lock().is_ok_and(|b| b.contains(address)) {
            continue;
        }
        match step(record) {
            Some(Step::Read { pid }) => {
                let text = std::fs::read_to_string(session_file(&files.home, files.config_dir.as_deref(), pid)).ok();
                let session = text.as_deref().and_then(parse_session);
                if let Some(label) = capture(record, session.as_ref()) {
                    set_label(core, address, &label);
                }
            }
            Some(Step::Clear) => set_label(core, address, ""),
            Some(Step::Resume { recipe, shell }) if restore => due.push((address.to_owned(), Step::Resume { recipe, shell })),
            _ => {}
        }
    }
    let mut resumed = Vec::new();
    for (address, recipe, shell) in resumes(due.iter().map(|(a, s)| (a.as_str(), s)), &HashSet::new()) {
        // Claim the terminal and the conversation for this pass and the next.
        {
            let Ok(mut b) = busy.lock() else { break };
            if b.contains(&address) || !b.insert(format!("session:{}", recipe.id)) {
                continue;
            }
            b.insert(address.clone());
        }
        let done = resume_one(core, files, &address, &recipe, shell);
        if let Ok(mut b) = busy.lock() {
            b.remove(&format!("session:{}", recipe.id));
            b.remove(&address);
        }
        if done {
            resumed.push(address);
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
    }
    resumed
}

/// Checks the terminal again right before typing, marks it, then types.
fn resume_one(core: &dyn Calls, files: &Files, address: &str, recipe: &Recipe, shell: Shell) -> bool {
    let Ok(now) = core.call("terminal.get", json!({"terminal": address})) else { return false };
    let Some(Step::Resume { recipe: again, shell: kind }) = step(&now) else { return false };
    if &again != recipe || kind != shell {
        return false;
    }
    let Some(pid) = now["pid"].as_u64() else { return false };
    if !transcript(&files.home, files.config_dir.as_deref(), &recipe.cwd, &recipe.id).is_file() {
        tracing::info!("resume: {address}'s conversation {} has no transcript; not resuming", recipe.id);
        set_label(core, address, "");
        return false;
    }
    // At most once: the new shell is marked before anything is typed.
    let marked = Recipe { shell: pid, ..recipe.clone() };
    if core.call("terminal.label", json!({"terminal": address, "labels": {LABEL: marked.label()}})).is_err() {
        return false;
    }
    let line = resume_line(shell, &recipe.cwd, &recipe.argv, &recipe.id);
    match core.call("terminal.send", json!({"terminal": address, "text": line})) {
        Ok(_) => {
            tracing::info!("resume: {address} back in conversation {}", recipe.id);
            true
        }
        Err(e) => {
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
        let busy = std::sync::Mutex::new(HashSet::new());

        // While Claude runs: labeled.
        assert!(tend(&core, &files, &[running], true, &busy).is_empty());
        let labeled = core.records.lock().unwrap()["t1"].clone();
        let recipe = Recipe::from_label(labeled["labels"][LABEL].as_str().unwrap()).unwrap();
        assert_eq!(recipe, Recipe { id: ID.into(), cwd: "/w/my app".into(), argv: s(&["claude", "--model", "opus"]), shell: 100, agent: 500 });

        // Reboot: a new shell 200 at its prompt. No transcript yet: not resumed, label dropped.
        let mut back = labeled.clone();
        back["pid"] = 200.into();
        back["foreground"] = json!({"pid": 200, "name": "zsh", "argv": ["-zsh"], "cwd": "/w/my app"});
        core.records.lock().unwrap().insert("t1".into(), back.clone());
        assert!(tend(&core, &files, &[back.clone()], true, &busy).is_empty());
        assert!(core.records.lock().unwrap()["t1"]["labels"].get(LABEL).is_none());

        // With the transcript: typed once, marked with the new shell first.
        let t = transcript(&home, None, "/w/my app", ID);
        std::fs::create_dir_all(t.parent().unwrap()).unwrap();
        std::fs::write(&t, "{}\n").unwrap();
        core.records.lock().unwrap().insert("t1".into(), back.clone());
        core.calls.lock().unwrap().clear();
        assert_eq!(tend(&core, &files, &[back.clone()], true, &busy), ["t1"]);
        let calls = core.calls.lock().unwrap().clone();
        let sends: Vec<&Value> = calls.iter().filter(|(m, _)| m == "terminal.send").map(|(_, p)| p).collect();
        assert_eq!(sends.len(), 1);
        assert_eq!(sends[0]["text"], format!("cd '/w/my app' && claude --model opus --resume {ID}"));
        let order: Vec<&str> = calls.iter().map(|(m, _)| m.as_str()).collect();
        assert_eq!(order, ["terminal.get", "terminal.label", "terminal.send"], "marked before typing");

        // The next pass sees the marked label: same shell at its prompt means nothing to resume.
        let after = core.records.lock().unwrap()["t1"].clone();
        assert_eq!(Recipe::from_label(after["labels"][LABEL].as_str().unwrap()).unwrap().shell, 200);
        assert!(tend(&core, &files, &[after], true, &busy).is_empty());
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
        let busy = std::sync::Mutex::new(HashSet::new());
        assert!(tend(&core, &files, &[back.clone()], false, &busy).is_empty(), "restore_agents: false");
        // The person started something in the pane after the listing.
        let mut changed = back.clone();
        changed["foreground"] = json!({"pid": 300, "name": "vim", "argv": ["vim"], "cwd": "/w"});
        core.records.lock().unwrap().insert("t1".into(), changed);
        assert!(tend(&core, &files, &[back], true, &busy).is_empty());
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
        let busy = std::sync::Mutex::new(HashSet::new());
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
        assert!(tend(core.as_ref(), &files, &[get(&core)], true, &busy).is_empty());
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

        assert_eq!(tend(core.as_ref(), &files, &[get(&core)], true, &busy), [t1.clone()]);
        wait("resumed", &|| root.join("resumed").exists());
        let line = std::fs::read_to_string(root.join("resumed")).unwrap();
        assert_eq!(line.trim(), format!("resumed: --model opus --resume {ID}"));
        // Never twice: the next pass sees the marked shell.
        std::fs::remove_file(root.join("resumed")).unwrap();
        assert!(tend(core.as_ref(), &files, &[get(&core)], true, &busy).is_empty());
        let _ = core.call("terminal.kill", json!({"terminal": t1}));
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
