use std::path::{Path, PathBuf};

pub fn git_root_from(start: &Path) -> Option<PathBuf> {
    // Walk upwards looking for .git directory.
    let mut cur = start;
    loop {
        if cur.join(".git").exists() {
            return Some(cur.to_path_buf());
        }
        match cur.parent() {
            Some(p) => cur = p,
            None => return None,
        }
    }
}

fn crumbs_root_from(start: &Path) -> Option<PathBuf> {
    // Walk upwards looking for an existing .crumbs directory.
    let mut cur = start;
    loop {
        if cur.join(".crumbs").is_dir() {
            return Some(cur.to_path_buf());
        }
        match cur.parent() {
            Some(p) => cur = p,
            None => return None,
        }
    }
}

/// Store root resolution:
/// - If inside a git repo: use the git root
/// - Else if an ancestor already has .crumbs/: reuse that directory
/// - Else: use the current directory
pub fn store_root_from_cwd(cwd: &Path) -> PathBuf {
    if let Some(root) = git_root_from(cwd) {
        return root;
    }
    if let Some(root) = crumbs_root_from(cwd) {
        return root;
    }
    cwd.to_path_buf()
}
