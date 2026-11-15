//! Windows can't rename a folder that is a process's working directory, and a
//! launcher or shortcut may start the app in its own folder. Leave it before the
//! core and terminals inherit it, so a newer Apollo can be put in its place
//! (`scripts/install-windows.ps1` renames the running app's folder aside).

use std::path::{Path, PathBuf};

/// Where this app is installed: the folder holding the binary and its icon
/// (cargo's `target/` has no icon).
fn root() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?.canonicalize().ok()?;
    let dir = exe.parent()?;
    dir.join("herdr-app.svg").is_file().then(|| dir.to_path_buf())
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
}
