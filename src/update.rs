//! Updates. Apollo never replaces itself: Herdr's app server (`releases.herdr.dev/app`)
//! ships Herdr, not Apollo, and its self-update (download, stage, swap on restart or
//! quit) is gone. `HERDR_APP_RELEASE` is never set. A release build (`APOLLO_RELEASE`
//! set at build time) looks up the latest Apollo release on GitHub and says when a
//! newer one is out; the person downloads it from the Releases page.
//!
//! The core (herdr-core) keeps updating: a release build checks `core/latest.json`
//! a little after it opens and every hour; a build from source checks only when
//! `HERDR_RELEASES_URL` points at a test server. A newer core of the same generation
//! is offered, unless a package manager owns it. Within a generation a newer core
//! only adds.

use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Condvar, Mutex};
use std::time::Duration;

const EVERY: Duration = Duration::from_secs(60 * 60);
/// After a failed check (offline, a damaged file).
const RETRY: Duration = Duration::from_secs(30 * 60);

fn releases() -> String {
    std::env::var("HERDR_RELEASES_URL").unwrap_or_else(|_| "https://releases.herdr.dev".into())
}

/// GitHub's API, or a test server (`APOLLO_GITHUB_API`).
fn github() -> String {
    std::env::var("APOLLO_GITHUB_API").unwrap_or_else(|_| "https://api.github.com".into())
}

/// The latest Apollo release, from GitHub.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct AppRelease {
    pub version: String,
    /// Its page on GitHub (the Releases page if GitHub gave none of ours).
    pub url: String,
}

/// The core the app runs, from its `server.status`, and a newer release waiting.
#[derive(Clone, Default, serde::Serialize)]
pub struct CoreInfo {
    pub version: Option<String>,
    /// A package manager that owns it (homebrew, nix, mise): never updated by the app.
    pub managed: Option<String>,
    pub ready: Option<String>,
    pub updating: bool,
    pub error: Option<String>,
}

static LATEST: Mutex<Option<AppRelease>> = Mutex::new(None);
static CORE: Mutex<CoreInfo> = Mutex::new(CoreInfo { version: None, managed: None, ready: None, updating: false, error: None });
/// When the last check succeeded (unix seconds); a failed check clears it.
static CHECKED: Mutex<Option<u64>> = Mutex::new(None);
/// "Check now" wakes the checker early.
static WAKE: (Mutex<bool>, Condvar) = (Mutex::new(false), Condvar::new());
/// The checker runs (a release, or pointed at a test server), and is looking right now.
static RUNNING: AtomicBool = AtomicBool::new(false);
static CHECKING: AtomicBool = AtomicBool::new(false);

/// A newer Apollo than this one, if the last check found one.
pub fn latest() -> Option<AppRelease> {
    newer_release(LATEST.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone(), env!("CARGO_PKG_VERSION"))
}

/// What `app.update` opens: the newer release's page, else the Releases page.
pub fn download_page(latest: Option<AppRelease>) -> String {
    latest.map_or_else(|| crate::brand::RELEASES_PAGE.to_owned(), |r| r.url)
}

/// `release`, if it is newer than `running`.
fn newer_release(release: Option<AppRelease>, running: &str) -> Option<AppRelease> {
    release.filter(|r| newer(&r.version, running))
}

pub fn core() -> CoreInfo {
    CORE.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone()
}

/// What the connected core says about itself (on connect and every reconnect).
pub fn set_core(version: Option<String>, managed: Option<String>) {
    set_core_in(&CORE, version, managed);
}

fn set_core_in(core: &Mutex<CoreInfo>, version: Option<String>, managed: Option<String>) {
    let mut c = core.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    // A core updated elsewhere (its package manager, a terminal) is no longer offered.
    if let (Some(have), Some(ready)) = (&version, &c.ready)
        && !newer(ready, have)
    {
        c.ready = None;
    }
    c.version = version;
    c.managed = managed;
    if c.managed.is_some() {
        c.error = None;
    }
}

#[derive(Debug, PartialEq)]
enum Product {
    /// The latest Apollo release on GitHub: looked up, never downloaded.
    App,
    Core,
}

/// What each check (hourly, or "Check now") looks for.
fn to_check(app: bool, core: bool) -> Vec<Product> {
    [(app, Product::App), (core, Product::Core)].into_iter().filter_map(|(on, p)| on.then_some(p)).collect()
}

/// A release build checks both; a build from source only what a test server stands in for.
fn plan(release: bool, releases_url: bool, github_url: bool) -> Vec<Product> {
    to_check(release || github_url, release || releases_url)
}

/// For the settings screen: both programs, what's out, when we last looked.
/// `app.ready` and `app.updates` stay for mods written for Herdr: nothing is ever
/// downloaded, so they are always null and false.
pub fn status() -> serde_json::Value {
    let latest = latest();
    serde_json::json!({
        "app": {
            "version": env!("CARGO_PKG_VERSION"),
            "latest": latest.as_ref().map(|r| &r.version),
            "url": latest.as_ref().map_or(crate::brand::RELEASES_PAGE, |r| r.url.as_str()),
            "ready": null,
            "updates": false,
        },
        "notes": crate::update_notes::notes(None),
        "core": core(),
        "checked": *CHECKED.lock().unwrap_or_else(std::sync::PoisonError::into_inner),
        "checks": RUNNING.load(Ordering::Relaxed),
        "checking": CHECKING.load(Ordering::Relaxed),
    })
}

pub fn check_now() {
    if RUNNING.load(Ordering::Relaxed) {
        CHECKING.store(true, Ordering::Relaxed);
    }
    *WAKE.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = true;
    WAKE.1.notify_all();
}

fn sleep(d: Duration) {
    let woken = WAKE.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let (mut woken, _) = WAKE.1.wait_timeout_while(woken, d, |w| !*w).unwrap_or_else(std::sync::PoisonError::into_inner);
    *woken = false;
}

/// Starts checking in the background: the latest Apollo release and herdr-core.
pub fn start() {
    let products = plan(
        option_env!("APOLLO_RELEASE").is_some(),
        std::env::var_os("HERDR_RELEASES_URL").is_some(),
        std::env::var_os("APOLLO_GITHUB_API").is_some(),
    );
    if products.is_empty() {
        tracing::info!("a build from source: no updates");
        return;
    }
    RUNNING.store(true, Ordering::Relaxed);
    CHECKING.store(true, Ordering::Relaxed);
    std::thread::spawn(move || {
        sleep(Duration::from_secs(10));
        loop {
            CHECKING.store(true, Ordering::Relaxed);
            let succeeded = check_pass(&products, &github(), &releases(), &CORE);
            let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
            *CHECKED.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = checked_at(succeeded, now);
            CHECKING.store(false, Ordering::Relaxed);
            sleep(if succeeded { EVERY } else { RETRY });
        }
    });
}

/// One check of `products`: what the hourly check and "Check now" run.
/// False if any part failed (offline, a damaged answer).
fn check_pass(products: &[Product], github: &str, releases: &str, core: &Mutex<CoreInfo>) -> bool {
    let mut succeeded = true;
    if products.contains(&Product::App) {
        match latest_app_from(github) {
            Ok(found) => *LATEST.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = found,
            Err(e) => {
                tracing::warn!("{} release check: {e}", crate::brand::NAME);
                succeeded = false;
            }
        }
    }
    if products.contains(&Product::Core)
        && let Err(e) = check_core(releases, core)
    {
        tracing::warn!("core update check: {e}");
        succeeded = false;
    }
    succeeded
}

fn checked_at(succeeded: bool, now: u64) -> Option<u64> {
    succeeded.then_some(now)
}

/// The latest published Apollo release (`Ok(None)`: none yet, or not one we can name).
fn latest_app_from(github: &str) -> Result<Option<AppRelease>, String> {
    let url = format!("{github}/repos/{}/releases/latest", crate::brand::GITHUB_REPO);
    let text = curl_text_with(&url, &["-H", "Accept: application/vnd.github+json", "-H", "X-GitHub-Api-Version: 2022-11-28"])?;
    let json: serde_json::Value = serde_json::from_str(&text).map_err(|e| format!("{url}: {e}"))?;
    Ok(parse_latest(&json))
}

/// What GitHub's `releases/latest` says, if it is a published release with a version
/// tag (`v0.0.6` or `0.0.6`). Only a page of our own repository is linked.
fn parse_latest(json: &serde_json::Value) -> Option<AppRelease> {
    if json["draft"].as_bool() != Some(false) || json["prerelease"].as_bool() != Some(false) {
        return None;
    }
    let version = json["tag_name"].as_str()?.trim();
    let version = version.strip_prefix('v').unwrap_or(version);
    let mut parts = version.split('.');
    let numeric = (0..3).all(|_| parts.next().is_some_and(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit())));
    if !numeric || parts.next().is_some() {
        return None;
    }
    let ours = format!("{}/", crate::brand::RELEASES_PAGE);
    let url = json["html_url"].as_str().filter(|u| u.starts_with(&ours) && !u.contains(['"', '\'', ' ', '\n'])).unwrap_or(crate::brand::RELEASES_PAGE);
    Some(AppRelease { version: version.to_owned(), url: url.to_owned() })
}

/// Looks for a newer core of this app's generation.
fn check_core(releases: &str, core: &Mutex<CoreInfo>) -> Result<(), String> {
    let have = core.lock().unwrap_or_else(std::sync::PoisonError::into_inner).version.clone();
    let Some(have) = have else { return Ok(()) };
    let latest = latest_core_from(releases)?;
    let same = latest.generation == Some(crate::hc::GENERATION);
    core.lock().unwrap_or_else(std::sync::PoisonError::into_inner).ready = (same && newer(&latest.version, &have)).then_some(latest.version);
    Ok(())
}

#[derive(serde::Deserialize)]
struct CoreRelease {
    version: String,
    generation: Option<u64>,
}

fn latest_core_from(releases: &str) -> Result<CoreRelease, String> {
    let text = curl_text(&format!("{releases}/core/latest.json"))?;
    serde_json::from_str(&text).map_err(|e| format!("core/latest.json: {e}"))
}

/// `hc update` (terminals keep running; the app reconnects when the core restarts).
pub fn update_core() {
    {
        let mut c = CORE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if c.updating || c.managed.is_some() {
            return;
        }
        c.updating = true;
        c.error = None;
    }
    std::thread::spawn(|| {
        let result = crate::core_bin::update();
        let mut c = CORE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        c.updating = false;
        match result {
            Ok(now) => {
                c.version = Some(now.trim_start_matches("hc ").to_owned());
                c.ready = None;
                crate::telemetry::count("core_update_ok");
            }
            Err(e) => {
                c.error = Some(e);
                crate::telemetry::count("core_update_failed");
            }
        }
    });
}

/// curl and tar: on Windows its own (System32), since Git's tar, often first
/// on PATH, takes `C:\...` for a remote host.
pub fn system(name: &str) -> PathBuf {
    match std::env::var_os("SystemRoot").filter(|_| cfg!(windows)) {
        Some(root) => PathBuf::from(root).join("System32").join(format!("{name}.exe")),
        None => PathBuf::from(name),
    }
}

pub(crate) fn sha256(bytes: &[u8]) -> String {
    use sha2::Digest;
    sha2::Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

fn curl_text(url: &str) -> Result<String, String> {
    curl_text_with(url, &[])
}

/// GitHub's API wants a User-Agent; curl sends its own.
fn curl_text_with(url: &str, headers: &[&str]) -> Result<String, String> {
    let out = crate::spawn::command(system("curl")).args(["-fsSL", "--max-time", "30"]).args(headers).arg(url).stdin(Stdio::null()).output().map_err(|e| format!("curl: {e}"))?;
    if !out.status.success() {
        return Err(format!("{url}: {}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    String::from_utf8(out.stdout).map_err(|e| e.to_string())
}

/// `a` is a later version than `b` (semver; `0.2.0-rc.1` comes before `0.2.0`).
pub(crate) fn newer(a: &str, b: &str) -> bool {
    fn parts(v: &str) -> (Vec<u64>, Option<&str>) {
        let (core, pre) = v.trim_start_matches('v').split_once('-').map_or((v.trim_start_matches('v'), None), |(c, p)| (c, Some(p)));
        (core.split('.').map(|n| n.parse().unwrap_or(0)).collect(), pre)
    }
    let ((ca, pa), (cb, pb)) = (parts(a), parts(b));
    match ca.cmp(&cb) {
        std::cmp::Ordering::Equal => match (pa, pb) {
            (None, Some(_)) => true,
            (Some(x), Some(y)) => x > y,
            _ => false,
        },
        o => o == std::cmp::Ordering::Greater,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use std::process::Command;

    #[test]
    fn apollo_never_updates_itself() {
        let s = status();
        for k in ["notes", "core", "checked", "checks", "checking"] {
            assert!(s.get(k).is_some(), "{k}");
        }
        assert_eq!(s["app"]["updates"], false);
        assert_eq!(s["app"]["ready"], serde_json::Value::Null);
        assert_eq!(s["app"]["version"], env!("CARGO_PKG_VERSION"));
        assert_eq!(s["app"]["url"], crate::brand::RELEASES_PAGE);
    }

    #[test]
    fn a_release_checks_github_and_the_core() {
        assert_eq!(plan(true, false, false), [Product::App, Product::Core]);
        assert_eq!(plan(false, false, false), Vec::<Product>::new());
        assert_eq!(plan(false, true, false), [Product::Core]);
        assert_eq!(plan(false, false, true), [Product::App]);
    }

    #[test]
    fn github_latest_release_is_read_strictly() {
        let page = |tag: &str| format!("{}/tag/{tag}", crate::brand::RELEASES_PAGE);
        let release = |tag: &str, draft, pre, url: &str| serde_json::json!({"tag_name": tag, "draft": draft, "prerelease": pre, "html_url": url});
        assert_eq!(parse_latest(&release("v0.0.6", false, false, &page("v0.0.6"))), Some(AppRelease { version: "0.0.6".into(), url: page("v0.0.6") }));
        assert_eq!(parse_latest(&release("1.2.30", false, false, &page("1.2.30"))).map(|r| r.version), Some("1.2.30".into()));
        // Not ours, or not a link at all: the Releases page instead.
        for url in ["https://evil.example/ForYouzi/apollo-app/releases/x", "https://github.com/ForYouzi/apollo-app-evil/releases/x", "javascript:alert(1)"] {
            assert_eq!(parse_latest(&release("v0.0.7", false, false, url)).map(|r| r.url), Some(crate::brand::RELEASES_PAGE.into()), "{url}");
        }
        for (tag, draft, pre) in [("v0.0.7", true, false), ("v0.0.7-rc.1", false, true), ("v0.0.7", false, true), ("nightly", false, false), ("v0.0", false, false), ("v0.0.7.1", false, false), ("v0.0.x", false, false), ("", false, false)] {
            assert_eq!(parse_latest(&release(tag, draft, pre, &page(tag))), None, "{tag} draft={draft} pre={pre}");
        }
        assert_eq!(parse_latest(&serde_json::json!({"message": "Not Found"})), None);
        assert_eq!(parse_latest(&serde_json::json!({"tag_name": "v0.0.7"})), None, "missing draft/prerelease flags");
    }

    #[test]
    fn a_core_updated_elsewhere_is_no_longer_offered() {
        let core = Mutex::new(CoreInfo { version: Some("0.1.18".into()), ready: Some("0.1.20".into()), ..Default::default() });
        set_core_in(&core, Some("0.1.19".into()), None);
        assert_eq!(core.lock().unwrap().ready.as_deref(), Some("0.1.20"), "still newer: still offered");
        set_core_in(&core, Some("0.1.20".into()), None);
        assert_eq!(core.lock().unwrap().ready, None, "installed elsewhere: no longer offered");
        assert_eq!(core.lock().unwrap().version.as_deref(), Some("0.1.20"));
        core.lock().unwrap().ready = Some("0.1.21".into());
        set_core_in(&core, None, Some("homebrew".into()));
        assert_eq!(core.lock().unwrap().version, None, "an unknown version is reported as unknown");
        assert_eq!(core.lock().unwrap().ready.as_deref(), Some("0.1.21"));
    }

    #[test]
    fn download_opens_the_newer_release_or_the_releases_page() {
        let page = format!("{}/tag/v9.9.9", crate::brand::RELEASES_PAGE);
        assert_eq!(download_page(Some(AppRelease { version: "9.9.9".into(), url: page.clone() })), page);
        assert_eq!(download_page(None), crate::brand::RELEASES_PAGE);
    }

    #[test]
    fn only_a_newer_release_is_offered() {
        let r = |v: &str| Some(AppRelease { version: v.into(), url: crate::brand::RELEASES_PAGE.into() });
        assert_eq!(newer_release(r("0.0.7"), "0.0.6"), r("0.0.7"));
        for v in ["0.0.6", "0.0.5"] {
            assert_eq!(newer_release(r(v), "0.0.6"), None);
        }
        assert_eq!(newer_release(None, "0.0.6"), None);
    }

    /// A local server that answers `path` with `body` (404 otherwise) and records
    /// what was asked for, until `/stop`.
    fn serve(answers: Vec<(&'static str, String)>) -> (String, std::sync::Arc<Mutex<Vec<String>>>, impl FnOnce()) {
        use std::io::{Read, Write};
        use std::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let seen = std::sync::Arc::new(Mutex::new(Vec::<String>::new()));
        let server = std::thread::spawn({
            let seen = seen.clone();
            move || {
                for stream in listener.incoming() {
                    let mut stream = stream.unwrap();
                    stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
                    let mut request = Vec::new();
                    while !request.ends_with(b"\r\n\r\n") {
                        let mut byte = [0];
                        stream.read_exact(&mut byte).unwrap();
                        request.push(byte[0]);
                    }
                    let path = String::from_utf8_lossy(&request).split(' ').nth(1).unwrap_or_default().to_owned();
                    if path == "/stop" {
                        return;
                    }
                    seen.lock().unwrap().push(path.clone());
                    match answers.iter().find(|(p, _)| *p == path) {
                        Some((_, body)) => write!(stream, "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap(),
                        None => write!(stream, "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap(),
                    }
                }
            }
        });
        let stop = move || {
            use std::io::Write;
            write!(std::net::TcpStream::connect(addr).unwrap(), "GET /stop HTTP/1.1\r\n\r\n").unwrap();
            server.join().unwrap();
        };
        (format!("http://{addr}"), seen, stop)
    }

    /// "Check now" end to end, against local servers: it wakes the checker, whose pass
    /// asks GitHub for the latest release and the release server for `core/latest.json`,
    /// and nothing under `app/` (Herdr's app downloads).
    #[test]
    fn check_now_looks_up_github_and_the_core_only() {
        let endpoint = format!("/repos/{}/releases/latest", crate::brand::GITHUB_REPO);
        let release = serde_json::json!({"tag_name": "v99.0.0", "draft": false, "prerelease": false, "html_url": format!("{}/tag/v99.0.0", crate::brand::RELEASES_PAGE)}).to_string();
        let core_body = format!(r#"{{"version":"99.0.0","generation":{}}}"#, crate::hc::GENERATION);
        let (url, seen, stop) = serve(vec![(Box::leak(endpoint.clone().into_boxed_str()), release), ("/core/latest.json", core_body)]);

        let products = plan(true, true, true);
        let core = Mutex::new(CoreInfo { version: Some("0.0.1".into()), ..Default::default() });
        let checker = std::thread::spawn(move || {
            let woke = std::time::Instant::now();
            sleep(Duration::from_secs(20));
            let woke = woke.elapsed();
            (woke, check_pass(&products, &url, &url, &core), core.into_inner().unwrap().ready)
        });
        check_now();
        let (woke, succeeded, ready) = checker.join().unwrap();
        stop();

        assert!(woke < Duration::from_secs(10), "Check now did not wake the checker ({woke:?})");
        let seen = seen.lock().unwrap().clone();
        assert_eq!(seen, [endpoint.as_str(), "/core/latest.json"]);
        assert!(!seen.iter().any(|p| p.starts_with("/app/")));
        assert!(succeeded);
        assert_eq!(ready.as_deref(), Some("99.0.0"));
        assert_eq!(latest().map(|r| r.version).as_deref(), Some("99.0.0"));
        assert_eq!(status()["app"]["latest"], "99.0.0");
        *LATEST.lock().unwrap() = None;
    }

    #[cfg(unix)]
    #[test]
    fn managed_cores_report_new_releases_without_running_updates() {
        const CHILD: &str = "HERDR_TEST_MANAGED_CORE_UPDATE";
        if std::env::var_os(CHILD).is_none() {
            let root = std::env::temp_dir().join(format!("herdr-managed-release-{}", std::process::id()));
            std::fs::create_dir_all(root.join("core")).unwrap();
            std::fs::write(root.join("core/latest.json"), r#"{"version":"0.1.20","generation":1}"#).unwrap();
            let output = Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "update::tests::managed_cores_report_new_releases_without_running_updates", "--nocapture"])
                .env(CHILD, "1")
                .env("HERDR_RELEASES_URL", format!("file://{}", root.display()))
                .output().unwrap();
            std::fs::remove_dir_all(root).unwrap();
            assert!(output.status.success(), "{}\n{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
            return;
        }
        crate::core_bin::use_core(PathBuf::from("/nonexistent/herdr-core"));
        for manager in ["nix", "homebrew", "mise", "system package manager"] {
            CORE.lock().unwrap().error = Some("an earlier update failed".into());
            set_core(Some("0.1.18".into()), Some(manager.into()));
            check_core(&releases(), &CORE).unwrap();
            assert_eq!(core().ready.as_deref(), Some("0.1.20"));
            update_core();
            let state = core();
            assert_eq!(state.managed.as_deref(), Some(manager));
            assert!(!state.updating);
            assert!(state.error.is_none());
            assert_eq!(state.ready.as_deref(), Some("0.1.20"));
        }
    }

    #[test]
    fn failed_check_clears_success_and_recovery_records_a_new_time() {
        let endpoint = format!("/repos/{}/releases/latest", crate::brand::GITHUB_REPO);
        let body = serde_json::json!({"tag_name": format!("v{}", env!("CARGO_PKG_VERSION")), "draft": false, "prerelease": false}).to_string();
        // GitHub unreachable: a closed port. Then a server that answers.
        let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let down = format!("http://{}", closed.local_addr().unwrap());
        drop(closed);
        let mut checked = checked_at(true, 100);
        assert_eq!(checked, Some(100));
        let failed = latest_app_from(&down);
        assert!(failed.is_err(), "an unreachable GitHub must fail: {failed:?}");
        // The whole pass fails, so the checker retries sooner and Settings says so.
        let core = Mutex::new(CoreInfo::default());
        let pass = check_pass(&[Product::App], &down, &down, &core);
        assert!(!pass, "a failed GitHub lookup must fail the check");
        checked = checked_at(pass, 200);
        assert_eq!(checked, None);
        let (url, _, stop) = serve(vec![(Box::leak(endpoint.into_boxed_str()), body)]);
        let recovered = latest_app_from(&url);
        stop();
        assert_eq!(recovered.map(|r| r.map(|r| r.version)), Ok(Some(env!("CARGO_PKG_VERSION").to_owned())));
        checked = checked_at(true, 300);
        assert_eq!(checked, Some(300));
    }

    #[test]
    fn versions_compare_like_semver() {
        assert!(newer("0.2.0", "0.1.9"));
        assert!(newer("0.10.0", "0.9.0"));
        assert!(newer("1.0.0", "1.0.0-rc.1"));
        assert!(!newer("1.0.0-rc.1", "1.0.0"));
        assert!(!newer("0.1.0", "0.1.0"));
        assert!(!newer("0.0.9", "0.1.0"));
    }

    #[test]
    fn sha256_is_hex() {
        assert_eq!(sha256(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }
}
