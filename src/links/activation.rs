//! What a clicked link means, and carrying it out through an `Opener`.

use super::{Target, file_url_path};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq)]
pub enum Activation {
    Reveal(PathBuf),
    OpenFolder(PathBuf),
    OpenUrl(String),
    Ignore,
}

/// Does what a clicked link means; production uses `SystemOpener`.
pub trait Opener {
    /// Shows `item` selected in its folder; never opens it.
    fn reveal(&self, item: &Path) -> std::io::Result<()>;
    fn open_folder(&self, dir: &Path);
    /// Never given a `file:` URL.
    fn open_url(&self, url: &str);
}

const PACKAGES: &[&str] = &["app", "bundle", "framework", "pkg", "mpkg", "plugin", "kext", "appex", "xpc", "prefpane", "qlgenerator", "saver", "wdgt"];

fn has_package_ext(p: &Path) -> bool {
    p.extension().and_then(|e| e.to_str()).is_some_and(|e| PACKAGES.iter().any(|k| k.eq_ignore_ascii_case(e)))
}

/// A package root, judged on the real location (symlinks and `..` resolved). macOS
/// also asks the system, which knows every declared package type (`.rtfd`, ...).
fn is_package_root(real: &Path) -> bool {
    #[cfg(target_os = "macos")]
    let native = || super::system::native_is_package(real);
    #[cfg(not(target_os = "macos"))]
    let native = || false;
    real.is_dir() && (has_package_ext(real) || native())
}

/// The outermost package containing (or equal to) the real location of `p`.
fn outermost_package(p: &Path) -> Option<PathBuf> {
    let real = std::fs::canonicalize(p).ok()?;
    real.ancestors().filter(|a| is_package_root(a)).last().map(Path::to_path_buf)
}

/// A folder safe to hand to the plain opener: its canonical form, climbing past every
/// package root (which `open` would launch). Real folders inside a package stay.
pub fn safe_folder(dir: &Path) -> PathBuf {
    let mut real = std::fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
    while is_package_root(&real) {
        match real.parent() {
            Some(up) => real = up.to_path_buf(),
            None => break,
        }
    }
    real
}

pub fn activation(target: &Target) -> Activation {
    match target {
        Target::Url(u) if u.get(..5).is_some_and(|s| s.eq_ignore_ascii_case("file:")) => {
            file_url_path(u).map_or(Activation::Ignore, |p| activation(&Target::File(p)))
        }
        Target::Url(u) => Activation::OpenUrl(u.clone()),
        Target::File(p) => {
            let Ok(p) = std::path::absolute(p) else { return Activation::Ignore };
            if p.exists() {
                if !p.is_dir() {
                    return Activation::Reveal(p);
                }
                // Judged by real location: a package root (even through a symlink or `..`) is
                // revealed; any other folder, including one inside a package, is opened.
                let real = std::fs::canonicalize(&p).unwrap_or(p);
                return if is_package_root(&real) { Activation::Reveal(real) } else { Activation::OpenFolder(real) };
            }
            match p.ancestors().skip(1).find(|a| a.exists()) {
                Some(q) => Activation::Reveal(outermost_package(q).unwrap_or_else(|| q.to_path_buf())),
                None => Activation::Ignore,
            }
        }
    }
}

pub fn activate(target: &Target, opener: &dyn Opener) {
    match activation(target) {
        Activation::Reveal(item) => {
            if let Err(e) = opener.reveal(&item) {
                tracing::warn!("cannot reveal {}: {e}", item.display());
                opener.open_folder(&safe_folder(item.parent().unwrap_or(&item)));
            }
        }
        Activation::OpenFolder(dir) => opener.open_folder(&dir),
        Activation::OpenUrl(url) => opener.open_url(&url),
        Activation::Ignore => {}
    }
}

#[cfg(test)]
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Call {
    Reveal(PathBuf),
    OpenFolder(PathBuf),
    OpenUrl(String),
}

#[cfg(test)]
#[derive(Default)]
pub(crate) struct Recorder {
    pub fail_reveal: bool,
    pub calls: std::cell::RefCell<Vec<Call>>,
}

#[cfg(test)]
impl Opener for Recorder {
    fn reveal(&self, item: &Path) -> std::io::Result<()> {
        self.calls.borrow_mut().push(Call::Reveal(item.into()));
        if self.fail_reveal { Err(std::io::Error::other("simulated")) } else { Ok(()) }
    }
    fn open_folder(&self, dir: &Path) {
        self.calls.borrow_mut().push(Call::OpenFolder(dir.into()));
    }
    fn open_url(&self, url: &str) {
        self.calls.borrow_mut().push(Call::OpenUrl(url.into()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// A fresh folder, canonical so `/var` and `/private/var` agree on macOS.
    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("apollo-act-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        fs::canonicalize(&d).unwrap()
    }

    /// Canonical paths on Windows are verbatim (`\\?\C:\…`); the URL drops that prefix.
    fn url(p: &Path) -> String {
        crate::links::file_url(p)
    }

    #[test]
    fn activation_table() {
        let d = tmp("table");
        fs::create_dir_all(d.join("Foo.app/Contents")).unwrap();
        fs::create_dir_all(d.join("plain")).unwrap();
        fs::create_dir_all(d.join("Bar.BUNDLE")).unwrap();
        fs::write(d.join("a.txt"), "").unwrap();
        let f = |p: PathBuf| activation(&Target::File(p));
        assert_eq!(f(d.join("a.txt")), Activation::Reveal(d.join("a.txt")));
        assert_eq!(f(d.join("plain")), Activation::OpenFolder(fs::canonicalize(d.join("plain")).unwrap()));
        assert_eq!(f(d.join("Foo.app")), Activation::Reveal(fs::canonicalize(d.join("Foo.app")).unwrap()));
        assert_eq!(f(d.join("Bar.BUNDLE")), Activation::Reveal(fs::canonicalize(d.join("Bar.BUNDLE")).unwrap()));
        assert_eq!(f(d.join("Foo.app/Contents/gone.plist")), Activation::Reveal(fs::canonicalize(d.join("Foo.app")).unwrap()));
        assert_eq!(f(d.join("plain/gone/x.txt")), Activation::Reveal(d.join("plain")));
        fs::create_dir_all(d.join("Outer.app/Contents/Inner.bundle/Contents")).unwrap();
        assert_eq!(
            crate::links::activation(&Target::File(d.join("Outer.app/Contents/Inner.bundle/Contents/gone.txt"))),
            Activation::Reveal(fs::canonicalize(d.join("Outer.app")).unwrap())
        );
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(d.join("a.txt"), d.join("lf")).unwrap();
            std::os::unix::fs::symlink(d.join("plain"), d.join("ld")).unwrap();
            assert_eq!(f(d.join("lf")), Activation::Reveal(d.join("lf")));
            assert_eq!(f(d.join("ld")), Activation::OpenFolder(fs::canonicalize(d.join("plain")).unwrap()));
        }
        let u = |s: String| activation(&Target::Url(s));
        assert_eq!(u("https://a.com".into()), Activation::OpenUrl("https://a.com".into()));
        // A URL names the non-verbatim path (Windows canonical paths are `\\?\C:\…`).
        let named = |p: &Path| file_url_path(&url(p)).unwrap();
        assert_eq!(u(url(&d.join("a.txt"))), Activation::Reveal(named(&d.join("a.txt"))));
        assert_eq!(u(url(&d.join("gone.txt"))), Activation::Reveal(named(&d)));
        assert_eq!(u("file:///%zz".into()), Activation::Ignore);
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(d.join("Foo.app"), d.join("alias")).unwrap();
            assert_eq!(f(d.join("alias")), Activation::Reveal(fs::canonicalize(d.join("Foo.app")).unwrap()));
        }
        assert_eq!(f(d.join("Foo.app/Contents/..")), Activation::Reveal(fs::canonicalize(d.join("Foo.app")).unwrap()));
        assert_eq!(f(d.join("Foo.app/Contents")), Activation::OpenFolder(fs::canonicalize(d.join("Foo.app/Contents")).unwrap()));
        assert_eq!(safe_folder(&d.join("Foo.app/Contents")), fs::canonicalize(d.join("Foo.app/Contents")).unwrap());
        assert_eq!(safe_folder(&d.join("Foo.app")), fs::canonicalize(&d).unwrap());
        fs::remove_dir_all(d).unwrap();
    }

    /// Packages the system declares (here `.rtfd`) count, not only the fixed list.
    #[cfg(target_os = "macos")]
    #[test]
    fn system_declared_packages_are_revealed() {
        let d = tmp("native");
        fs::create_dir_all(d.join("doc.rtfd")).unwrap();
        fs::create_dir_all(d.join("plain.notapackage")).unwrap();
        let f = |p: PathBuf| activation(&Target::File(p));
        assert_eq!(f(d.join("doc.rtfd")), Activation::Reveal(fs::canonicalize(d.join("doc.rtfd")).unwrap()));
        assert_eq!(f(d.join("plain.notapackage")), Activation::OpenFolder(fs::canonicalize(d.join("plain.notapackage")).unwrap()));
        fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn activate_only_reveals_files_and_falls_back_to_an_ordinary_folder() {
        let d = tmp("dispatch");
        fs::create_dir_all(d.join("Foo.app/Contents")).unwrap();
        fs::write(d.join("Foo.app/Contents/Info.plist"), "").unwrap();
        fs::write(d.join("a.txt"), "").unwrap();
        let rec = Recorder::default();
        for t in [
            Target::File(d.join("a.txt")),
            Target::File(d.join("Foo.app")),
            Target::Url(url(&d.join("a.txt"))),
            Target::File(d.join("Foo.app/Contents/gone")),
            Target::Url("FILE:///%zz".into()),
        ] {
            activate(&t, &rec);
        }
        let calls = rec.calls.take();
        assert!(calls.iter().all(|c| matches!(c, Call::Reveal(_))), "{calls:?}");
        assert_eq!(calls.last(), Some(&Call::Reveal(fs::canonicalize(d.join("Foo.app")).unwrap())));
        let failing = Recorder { fail_reveal: true, ..Default::default() };
        activate(&Target::File(d.join("Foo.app/Contents/Info.plist")), &failing);
        assert_eq!(
            failing.calls.take(),
            vec![Call::Reveal(d.join("Foo.app/Contents/Info.plist")), Call::OpenFolder(fs::canonicalize(d.join("Foo.app/Contents")).unwrap())]
        );
        activate(&Target::File(d.join("Foo.app")), &failing);
        assert_eq!(failing.calls.take(), vec![Call::Reveal(fs::canonicalize(d.join("Foo.app")).unwrap()), Call::OpenFolder(fs::canonicalize(&d).unwrap())]);
        fs::create_dir_all(d.join("Outer.app/Inner.bundle")).unwrap();
        fs::write(d.join("Outer.app/Inner.bundle/file.txt"), "").unwrap();
        activate(&Target::File(d.join("Outer.app/Inner.bundle/file.txt")), &failing);
        assert_eq!(failing.calls.take(), vec![Call::Reveal(d.join("Outer.app/Inner.bundle/file.txt")), Call::OpenFolder(fs::canonicalize(&d).unwrap())]);
        assert_eq!(safe_folder(&d.join("Outer.app/Inner.bundle")), fs::canonicalize(&d).unwrap());
        activate(&Target::Url("https://a.com".into()), &rec);
        assert_eq!(rec.calls.take(), vec![Call::OpenUrl("https://a.com".into())]);
        fs::remove_dir_all(d).unwrap();
    }
}
