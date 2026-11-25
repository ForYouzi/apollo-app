//! Windows can't rename a folder that is a process's working directory, and a
//! launcher or shortcut may start the app in its own folder. Leave it before the
//! core and terminals inherit it, so a newer Apollo can be put in its place
//! (`scripts/install-windows.ps1` renames the running app's folder aside).

use std::path::{Path, PathBuf};

/// The icon beside the binary that marks its install folder: the release zip and
/// `scripts/install-windows.ps1` ship `apollo-app.svg`; older source installs have `herdr-app.svg`.
pub(crate) const ICONS: [&str; 2] = ["apollo-app.svg", "herdr-app.svg"];

/// Where this app is installed: the folder holding the binary and its icon
/// (cargo's `target/` has no icon).
fn root() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?.canonicalize().ok()?;
    root_of(exe.parent()?)
}

fn root_of(dir: &Path) -> Option<PathBuf> {
    ICONS.iter().any(|icon| dir.join(icon).is_file()).then(|| dir.to_path_buf())
}

/// Where to go instead of `cwd`: home, when `cwd` is inside the install folder.
fn elsewhere(cwd: &Path, root: &Path, home: Option<PathBuf>) -> Option<PathBuf> {
    cwd.starts_with(root).then_some(home).flatten()
}

pub fn leave() {
    if !cfg!(windows) {
        return;
    }
    let (Some(root), Ok(cwd)) = (root(), std::env::current_dir()) else { return };
    let cwd = cwd.canonicalize().unwrap_or(cwd);
    if let Some(home) = elsewhere(&cwd, &root, std::env::home_dir())
        && let Err(e) = std::env::set_current_dir(&home)
    {
        tracing::warn!("cannot leave the install folder for {}: {e}", home.display());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starting_inside_the_install_folder_moves_home() {
        let root = Path::new("/apps/apollo-app");
        let home = Some(PathBuf::from("/home/me"));
        assert_eq!(elsewhere(root, root, home.clone()), home);
        assert_eq!(elsewhere(&root.join("sub"), root, home.clone()), home);
        assert_eq!(elsewhere(Path::new("/home/me/work"), root, home.clone()), None);
        // A sibling whose name merely starts the same is not inside.
        assert_eq!(elsewhere(Path::new("/apps/apollo-app-old"), root, home), None);
        assert_eq!(elsewhere(root, root, None), None);
    }

    /// The release zip ships `apollo-app.svg`, the install script `herdr-app.svg`:
    /// either marks the install folder; neither (cargo's `target/`) doesn't.
    #[test]
    fn either_shipped_icon_marks_the_install_folder() {
        let dir = std::env::temp_dir().join(format!("apollo-install-folder-{}", std::process::id()));
        for icons in [&["apollo-app.svg"][..], &["herdr-app.svg"], &["apollo-app.svg", "herdr-app.svg"], &[], &["apollo.svg"]] {
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            for icon in icons {
                std::fs::write(dir.join(icon), "<svg/>").unwrap();
            }
            let marked = icons.iter().any(|i| ICONS.contains(i));
            assert_eq!(root_of(&dir), marked.then(|| dir.clone()), "{icons:?}");
        }
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(ICONS, ["apollo-app.svg", "herdr-app.svg"]);
    }
}
