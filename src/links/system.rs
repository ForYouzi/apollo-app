//! The system's own handlers: Finder, Explorer, or the desktop's opener.

use super::{Opener, safe_folder};
use std::ffi::OsStr;
#[cfg(any(not(windows), test))]
use std::ffi::OsString;
use std::path::Path;

pub struct SystemOpener;

/// macOS: Finder shows the item selected (`open -R`), never opening it.
#[cfg(any(not(windows), test))]
pub(crate) fn reveal_argv(item: &Path) -> std::io::Result<[OsString; 3]> {
    Ok(["/usr/bin/open".into(), "-R".into(), std::path::absolute(item)?.into_os_string()])
}

#[cfg(not(windows))]
impl Opener for SystemOpener {
    fn reveal(&self, item: &Path) -> std::io::Result<()> {
        if !cfg!(target_os = "macos") {
            self.open_folder(&safe_folder(item.parent().unwrap_or(item)));
            return Ok(());
        }
        let argv = reveal_argv(item)?;
        let fallback = safe_folder(item.parent().unwrap_or(item));
        spawn_reaped(argv.to_vec(), move || SystemOpener.open_folder(&fallback))
    }
    fn open_folder(&self, dir: &Path) {
        shell_open(dir.as_os_str(), true)
    }
    fn open_url(&self, url: &str) {
        shell_open(OsStr::new(url), false)
    }
}

/// Spawned and reaped on a worker thread (never on the UI thread); `failed` runs if
/// the process can't start or exits unsuccessfully.
#[cfg(not(windows))]
fn spawn_reaped(argv: Vec<OsString>, failed: impl FnOnce() + Send + 'static) -> std::io::Result<()> {
    use std::process::{Command, Stdio};
    std::thread::Builder::new().name("apollo-open".into()).spawn(move || {
        let ok = Command::new(&argv[0])
            .args(&argv[1..])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success());
        if !ok {
            failed()
        }
    })?;
    Ok(())
}

/// The desktop's own opener for a folder or URL: `/usr/bin/open` on macOS, `xdg-open` elsewhere.
#[cfg(not(windows))]
fn open_argv(arg: &OsStr) -> [OsString; 2] {
    let opener = if cfg!(target_os = "macos") { "/usr/bin/open" } else { "xdg-open" };
    [opener.into(), arg.into()]
}

#[cfg(not(windows))]
fn shell_open(arg: &OsStr, _folder: bool) {
    let argv = open_argv(arg);
    if let Err(e) = spawn_reaped(argv.to_vec(), || {}) {
        tracing::warn!("cannot start a thread for {}: {e}", argv[0].display());
    }
}

/// Hands `arg` to its default handler; a folder gets the explicit "open" verb.
#[cfg(windows)]
fn shell_open(arg: &OsStr, folder: bool) {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOWNORMAL};

    let Some(wide) = shell_target(&arg.encode_wide().collect::<Vec<u16>>()) else {
        tracing::warn!("cannot open a link containing a NUL");
        return;
    };
    let started = on_sta_thread(move || {
        let verb: Vec<u16> = "open\0".encode_utf16().collect();
        let verb = if folder { verb.as_ptr() } else { std::ptr::null() };
        // Explorer interprets '=' as its own arguments; the handler gets a literal target.
        // SAFETY: `wide` and `verb` are NUL-terminated and outlive the call.
        let result = unsafe { ShellExecuteW(std::ptr::null_mut(), verb, wide.as_ptr(), std::ptr::null(), std::ptr::null(), SW_SHOWNORMAL) } as isize;
        if result <= 32 {
            tracing::warn!("cannot open link: ShellExecuteW returned {result}");
        }
    });
    if let Err(e) = started {
        tracing::warn!("cannot start a thread to open a link: {e}");
    }
}

/// Whether `CoInitializeEx` initialized COM (S_OK, or S_FALSE when already initialized),
/// so it must be balanced by `CoUninitialize`; never after a failure like RPC_E_CHANGED_MODE.
#[cfg(any(windows, test))]
pub(crate) fn com_initialized(hr: i32) -> bool {
    hr == 0 || hr == 1
}

/// Runs `f` on a new thread in a single-threaded COM apartment, as the shell requires;
/// the shell can pump messages, so it must never run on (and reenter) the GPUI thread.
#[cfg(windows)]
pub(crate) fn on_sta_thread(f: impl FnOnce() + Send + 'static) -> std::io::Result<std::thread::JoinHandle<()>> {
    use windows_sys::Win32::System::Com::{COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoInitializeEx, CoUninitialize};
    std::thread::Builder::new().name("apollo-shell".into()).spawn(move || {
        // SAFETY: called once on this fresh thread with no reserved pointer.
        let hr = unsafe { CoInitializeEx(std::ptr::null(), (COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) as u32) };
        f();
        if com_initialized(hr) {
            // SAFETY: balances the successful `CoInitializeEx` above, on the same thread.
            unsafe { CoUninitialize() }
        }
    })
}

/// `\\?\C:\x` → `C:\x`, `\\?\UNC\s\x` → `\\s\x`: the shell doesn't parse verbatim paths.
#[cfg(any(windows, test))]
pub(crate) fn strip_verbatim(w: &[u16]) -> Vec<u16> {
    let p = |s: &str| s.encode_utf16().collect::<Vec<u16>>();
    if let Some(rest) = w.strip_prefix(&p(r"\\?\UNC\")[..]) {
        return [p(r"\\"), rest.to_vec()].concat();
    }
    w.strip_prefix(&p(r"\\?\")[..]).unwrap_or(w).to_vec()
}

/// What the shell is handed: no verbatim prefix (canonical paths carry one), NUL-terminated;
/// `None` if `arg` holds a NUL of its own.
#[cfg(any(windows, test))]
pub(crate) fn shell_target(arg: &[u16]) -> Option<Vec<u16>> {
    (!arg.contains(&0)).then(|| [strip_verbatim(arg), vec![0]].concat())
}

#[cfg(windows)]
use windows_sys::Win32::UI::Shell::Common::ITEMIDLIST;

/// Elsewhere only tests use the shell seam, through an opaque item list.
#[cfg(all(test, not(windows)))]
pub(crate) enum ITEMIDLIST {}

/// The shell calls a reveal makes, a seam so tests can count what is freed.
#[cfg(any(windows, test))]
pub(crate) trait Shell {
    /// The raw HRESULT and out-pointer; a failed parse may still hand back a PIDL.
    fn parse(&self, path: &[u16]) -> (i32, *mut ITEMIDLIST);
    fn select(&self, folder: *const ITEMIDLIST, cidl: u32, children: *const *const ITEMIDLIST, flags: u32) -> i32;
    fn free(&self, pidl: *mut ITEMIDLIST);
}

#[cfg(windows)]
pub(crate) struct RealShell;

#[cfg(windows)]
impl Shell for RealShell {
    fn parse(&self, path: &[u16]) -> (i32, *mut ITEMIDLIST) {
        let mut pidl = std::ptr::null_mut();
        // SAFETY: `path` is NUL-terminated (from `wide`); `pidl` is a valid out pointer.
        let hr = unsafe { windows_sys::Win32::UI::Shell::SHParseDisplayName(path.as_ptr(), std::ptr::null_mut(), &mut pidl, 0, std::ptr::null_mut()) };
        (hr, pidl)
    }
    fn select(&self, f: *const ITEMIDLIST, cidl: u32, kids: *const *const ITEMIDLIST, flags: u32) -> i32 {
        // SAFETY: `f` is a PIDL from `parse`, not yet freed; `kids` holds `cidl` entries.
        unsafe { windows_sys::Win32::UI::Shell::SHOpenFolderAndSelectItems(f, cidl, kids, flags) }
    }
    fn free(&self, pidl: *mut ITEMIDLIST) {
        // SAFETY: `pidl` is a non-null PIDL from `parse`, freed exactly once by `select_in_folder`.
        unsafe { windows_sys::Win32::UI::Shell::ILFree(pidl) }
    }
}

/// Opens the item's folder with it selected (cidl = 0); the PIDL is freed on every path.
#[cfg(any(windows, test))]
pub(crate) fn select_in_folder(shell: &dyn Shell, wide: &[u16]) -> Result<(), i32> {
    const E_FAIL: i32 = 0x80004005u32 as i32;
    let (hr, pidl) = shell.parse(wide);
    let result = match (hr, pidl.is_null()) {
        (hr, _) if hr < 0 => Err(hr),
        (_, true) => Err(E_FAIL),
        _ => match shell.select(pidl, 0, std::ptr::null(), 0) {
            hr if hr < 0 => Err(hr),
            _ => Ok(()),
        },
    };
    if !pidl.is_null() {
        shell.free(pidl);
    }
    result
}

/// The absolute, non-verbatim, NUL-terminated wide form of `p`.
#[cfg(windows)]
fn wide(p: &Path) -> std::io::Result<Vec<u16>> {
    use std::os::windows::ffi::OsStrExt;
    let w: Vec<u16> = std::path::absolute(p)?.as_os_str().encode_wide().collect();
    shell_target(&w).ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "NUL in path"))
}

#[cfg(windows)]
impl Opener for SystemOpener {
    fn reveal(&self, item: &Path) -> std::io::Result<()> {
        let w = wide(item)?;
        let fallback = safe_folder(item.parent().unwrap_or(item));
        on_sta_thread(move || {
            if let Err(e) = select_in_folder(&RealShell, &w) {
                tracing::warn!("cannot reveal: HRESULT {e:#x}");
                SystemOpener.open_folder(&fallback);
            }
        })?;
        Ok(())
    }
    fn open_folder(&self, dir: &Path) {
        shell_open(dir.as_os_str(), true)
    }
    fn open_url(&self, url: &str) {
        shell_open(OsStr::new(url), false)
    }
}

/// Whether the system treats `p` as a package (`kCFURLIsPackageKey`), which `open`
/// would launch rather than show; false on any failure.
#[cfg(target_os = "macos")]
pub(super) fn native_is_package(p: &Path) -> bool {
    use std::ffi::c_void;
    use std::os::unix::ffi::OsStrExt;
    type Ref = *const c_void;
    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        static kCFAllocatorDefault: Ref;
        static kCFURLIsPackageKey: Ref;
        static kCFBooleanTrue: Ref;
        fn CFURLCreateFromFileSystemRepresentation(allocator: Ref, buffer: *const u8, len: isize, is_directory: u8) -> Ref;
        fn CFURLCopyResourcePropertyForKey(url: Ref, key: Ref, value: *mut Ref, error: *mut Ref) -> u8;
        fn CFRelease(cf: Ref);
    }
    let bytes = p.as_os_str().as_bytes();
    let Ok(len) = isize::try_from(bytes.len()) else { return false };
    // SAFETY: `bytes` outlives the call and `len` is its length; a null result is checked.
    let url = unsafe { CFURLCreateFromFileSystemRepresentation(kCFAllocatorDefault, bytes.as_ptr(), len, 1) };
    if url.is_null() {
        return false;
    }
    let mut value: Ref = std::ptr::null();
    // SAFETY: `url` is a live CFURL; `value` is a valid out pointer; no error is requested.
    let ok = unsafe { CFURLCopyResourcePropertyForKey(url, kCFURLIsPackageKey, &mut value, std::ptr::null_mut()) } != 0;
    // SAFETY: `kCFBooleanTrue` is a CF constant; only the identity of `value` is compared.
    let package = ok && !value.is_null() && value == unsafe { kCFBooleanTrue };
    if !value.is_null() {
        // SAFETY: `value` was returned by a Copy function, so this call owns it.
        unsafe { CFRelease(value) };
    }
    // SAFETY: `url` was returned by a Create function, so this call owns it.
    unsafe { CFRelease(url) };
    package
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn strip_verbatim_prefixes() {
        let w = |s: &str| s.encode_utf16().collect::<Vec<u16>>();
        assert_eq!(strip_verbatim(&w(r"\\?\C:\a\b.txt")), w(r"C:\a\b.txt"));
        assert_eq!(strip_verbatim(&w(r"\\?\UNC\srv\share\x")), w(r"\\srv\share\x"));
        assert_eq!(strip_verbatim(&w(r"C:\a")), w(r"C:\a"));
    }

    #[test]
    fn shell_targets_are_non_verbatim_and_nul_terminated() {
        let w = |s: &str| s.encode_utf16().collect::<Vec<u16>>();
        let z = |s: &str| [w(s), vec![0]].concat();
        assert_eq!(shell_target(&w(r"\\?\C:\a\d")), Some(z(r"C:\a\d")));
        assert_eq!(shell_target(&w(r"\\?\UNC\srv\share\d")), Some(z(r"\\srv\share\d")));
        assert_eq!(shell_target(&w("https://a.com/?q=1")), Some(z("https://a.com/?q=1")));
        assert_eq!(shell_target(&[w("C:\\a"), vec![0], w("b")].concat()), None);
    }

    #[test]
    fn com_is_uninitialized_only_after_it_was_initialized() {
        assert!(com_initialized(0));
        assert!(com_initialized(1));
        assert!(!com_initialized(0x80010106u32 as i32));
        assert!(!com_initialized(-1));
    }

    /// A scripted shell: `parse` returns a fixed HRESULT and dummy PIDL, never dereferenced.
    struct Scripted {
        parsed: (i32, *mut ITEMIDLIST),
        selected: std::cell::Cell<u32>,
        freed: std::cell::RefCell<Vec<*mut ITEMIDLIST>>,
    }

    impl Shell for Scripted {
        fn parse(&self, _: &[u16]) -> (i32, *mut ITEMIDLIST) {
            self.parsed
        }
        fn select(&self, _: *const ITEMIDLIST, _: u32, _: *const *const ITEMIDLIST, _: u32) -> i32 {
            self.selected.set(self.selected.get() + 1);
            0
        }
        fn free(&self, pidl: *mut ITEMIDLIST) {
            self.freed.borrow_mut().push(pidl);
        }
    }

    #[test]
    fn select_in_folder_frees_a_pidl_from_a_failed_parse() {
        const E_FAIL: i32 = 0x80004005u32 as i32;
        let dummy = std::ptr::NonNull::<ITEMIDLIST>::dangling().as_ptr();
        let run = |hr: i32, pidl: *mut ITEMIDLIST| {
            let shell = Scripted { parsed: (hr, pidl), selected: Default::default(), freed: Default::default() };
            let result = select_in_folder(&shell, &[0]);
            (result, shell.selected.get(), shell.freed.take())
        };
        assert_eq!(run(E_FAIL, dummy), (Err(E_FAIL), 0, vec![dummy]));
        assert_eq!(run(E_FAIL, std::ptr::null_mut()), (Err(E_FAIL), 0, vec![]));
        assert_eq!(run(0, std::ptr::null_mut()), (Err(E_FAIL), 0, vec![]));
        assert_eq!(run(0, dummy), (Ok(()), 1, vec![dummy]));
    }

    #[cfg(windows)]
    #[test]
    fn shell_work_runs_on_a_new_sta_thread() {
        use windows_sys::Win32::System::Com::{APTTYPE_MAINSTA, APTTYPE_STA, CoGetApartmentType};
        let (tx, rx) = std::sync::mpsc::channel();
        on_sta_thread(move || {
            let (mut kind, mut qualifier) = (-1, -1);
            // SAFETY: both out pointers are valid for the call.
            let hr = unsafe { CoGetApartmentType(&mut kind, &mut qualifier) };
            tx.send((std::thread::current().id(), hr, kind)).unwrap();
        })
        .unwrap()
        .join()
        .unwrap();
        let (thread, hr, kind) = rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
        assert_ne!(thread, std::thread::current().id());
        // The process's first STA reports itself as the main STA; both are single-threaded.
        assert_eq!(hr, 0);
        assert!(kind == APTTYPE_STA || kind == APTTYPE_MAINSTA, "apartment type {kind}");
    }

    #[cfg(windows)]
    #[test]
    fn select_in_folder_frees_every_pidl() {
        use std::cell::{Cell, RefCell};
        struct Fake {
            real: RealShell,
            fail: bool,
            selected: RefCell<Vec<(bool, u32, bool, u32)>>,
            freed: Cell<u32>,
        }
        impl Shell for Fake {
            fn parse(&self, p: &[u16]) -> (i32, *mut ITEMIDLIST) {
                self.real.parse(p)
            }
            fn select(&self, f: *const ITEMIDLIST, cidl: u32, kids: *const *const ITEMIDLIST, flags: u32) -> i32 {
                self.selected.borrow_mut().push((f.is_null(), cidl, kids.is_null(), flags));
                if self.fail { -2147467259 } else { 0 }
            }
            fn free(&self, p: *mut ITEMIDLIST) {
                self.freed.set(self.freed.get() + 1);
                self.real.free(p)
            }
        }
        let file = std::env::temp_dir().join(format!("apollo-pidl-{}.txt", std::process::id()));
        std::fs::write(&file, "").unwrap();
        for fail in [false, true] {
            let fake = Fake { real: RealShell, fail, selected: Default::default(), freed: Cell::new(0) };
            assert_eq!(select_in_folder(&fake, &wide(&file).unwrap()).is_ok(), !fail);
            assert_eq!(fake.selected.take(), vec![(false, 0, true, 0)]);
            assert_eq!(fake.freed.get(), 1);
        }
        let fake = Fake { real: RealShell, fail: false, selected: Default::default(), freed: Cell::new(0) };
        assert!(select_in_folder(&fake, &wide(&file.with_extension("gone")).unwrap()).is_err());
        assert_eq!((fake.selected.take().len(), fake.freed.get()), (0, 0));
        std::fs::remove_file(file).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn windows_drive_and_unc_paths_only_reveal() {
        use crate::links::{Call, Recorder, activate, resolve};
        let ini = r"C:\Windows\win.ini";
        let unc = r"\\localhost\C$\Windows\win.ini";
        let line = format!("{ini}:3");
        for text in [ini, unc, line.as_str()] {
            let t = resolve(text, None).expect(text);
            let rec = Recorder::default();
            activate(&t, &rec);
            assert!(matches!(rec.calls.take().as_slice(), [Call::Reveal(_)]), "{text}");
        }
    }

    #[cfg(not(windows))]
    #[test]
    fn open_argv_hands_folders_and_urls_to_the_desktop_opener() {
        let opener = OsString::from(if cfg!(target_os = "macos") { "/usr/bin/open" } else { "xdg-open" });
        let dir = std::env::temp_dir();
        assert_eq!(open_argv(dir.as_os_str()), [opener.clone(), dir.clone().into_os_string()]);
        assert_eq!(open_argv(OsStr::new("https://a.com/?q=1")), [opener, OsString::from("https://a.com/?q=1")]);
    }

    /// Only `/usr/bin/true` and `/usr/bin/false` are ever spawned here.
    #[cfg(unix)]
    #[test]
    fn spawn_reaped_waits_on_its_worker_and_reports_failure() {
        use std::time::Duration;
        let run = |program: &str| {
            let (tx, rx) = std::sync::mpsc::channel();
            spawn_reaped(vec![program.into()], move || {
                let _ = tx.send(std::thread::current().name().map(str::to_owned));
            })
            .unwrap();
            rx.recv_timeout(Duration::from_secs(5))
        };
        assert_eq!(run("/usr/bin/false"), Ok(Some("apollo-open".into())));
        assert!(run("/usr/bin/true").is_err());
    }

    #[test]
    fn reveal_argv_is_absolute_open_r() {
        let argv = reveal_argv(Path::new("rel/x.txt")).unwrap();
        assert_eq!(argv[..2], [OsString::from("/usr/bin/open"), OsString::from("-R")]);
        assert_eq!(PathBuf::from(&argv[2]), std::env::current_dir().unwrap().join("rel/x.txt"));
    }
}
