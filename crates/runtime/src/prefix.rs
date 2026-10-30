//! Proton-style prefix bootstrap and DLL override handling.
//!
//! n2np does not implement the full Wine/Proton runtime yet, but it can create
//! and use a prefix with the same directory layout. This gives applications a
//! stable `C:\` tree and lets users point at real Windows DLLs (for example a
//! DXVK or VKD3D-Proton build) without replacing the whole runtime.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

/// The user name used inside a freshly created prefix.
pub const DEFAULT_USER: &str = "steamuser";

static ACTIVE_ROOT: OnceLock<PathBuf> = OnceLock::new();
static OVERRIDES: OnceLock<Mutex<Vec<DllOverride>>> = OnceLock::new();

/// A `NAME=PATH` DLL override, for example `d3d11=native`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DllOverride {
    pub name: String,
    pub path: PathBuf,
}

/// A Win32 prefix rooted at a host directory.
#[derive(Debug, Clone)]
pub struct Prefix {
    root: PathBuf,
}

impl Prefix {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn drive_c(&self) -> PathBuf {
        self.root.join("drive_c")
    }

    pub fn windows_dir(&self) -> PathBuf {
        self.drive_c().join("windows")
    }

    pub fn system32(&self) -> PathBuf {
        self.windows_dir().join("system32")
    }

    pub fn syswow64(&self) -> PathBuf {
        self.windows_dir().join("syswow64")
    }

    pub fn users_dir(&self) -> PathBuf {
        self.drive_c().join("users")
    }

    pub fn user_dir(&self) -> PathBuf {
        self.users_dir().join(DEFAULT_USER)
    }

    pub fn dosdevices(&self) -> PathBuf {
        self.root.join("dosdevices")
    }

    /// Create the standard prefix layout. Existing directories and links are
    /// left untouched, so this is safe to run on every start.
    pub fn ensure(&self) -> std::io::Result<()> {
        let user = self.user_dir();
        let directories = [
            self.root.clone(),
            self.drive_c(),
            self.windows_dir(),
            self.system32(),
            self.syswow64(),
            self.drive_c().join("Program Files"),
            self.drive_c().join("Program Files (x86)"),
            self.drive_c().join("ProgramData"),
            self.users_dir(),
            user.clone(),
            user.join("Desktop"),
            user.join("Documents"),
            user.join("Downloads"),
            user.join("AppData").join("Local"),
            user.join("AppData").join("Roaming"),
            self.dosdevices(),
        ];
        for directory in directories {
            fs::create_dir_all(directory)?;
        }

        link_drive(&self.dosdevices().join("c:"), &self.drive_c())?;
        link_drive(&self.dosdevices().join("z:"), Path::new("/"))?;
        Ok(())
    }
}

fn link_drive(link: &Path, target: &Path) -> std::io::Result<()> {
    if link.symlink_metadata().is_ok() {
        return Ok(());
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, link)
    }
    #[cfg(not(unix))]
    {
        let _ = (link, target);
        Ok(())
    }
}

/// Resolve the prefix root from `N2NP_PREFIX`, `WINEPREFIX`, or `$HOME/.n2np`.
pub fn default_root() -> PathBuf {
    if let Some(value) = std::env::var_os("N2NP_PREFIX") {
        return PathBuf::from(value);
    }
    if let Some(value) = std::env::var_os("WINEPREFIX") {
        return PathBuf::from(value);
    }
    std::env::var_os("HOME")
        .map(|home| PathBuf::from(home).join(".n2np"))
        .unwrap_or_else(|| PathBuf::from(".n2np"))
}

/// Set the active prefix. Returns the path that was selected.
pub fn set_root(root: impl Into<PathBuf>) -> PathBuf {
    let root = root.into();
    let _ = ACTIVE_ROOT.set(root.clone());
    root
}

/// The active prefix root, falling back to the environment/default.
pub fn active_root() -> PathBuf {
    ACTIVE_ROOT.get().cloned().unwrap_or_else(default_root)
}

/// Parse a single `NAME=PATH` override.
pub fn parse_override(spec: &str) -> Result<DllOverride, String> {
    let (name, path) = spec
        .split_once('=')
        .ok_or_else(|| format!("invalid DLL override `{spec}`: expected NAME=PATH"))?;
    let name = name.trim();
    let path = path.trim();
    if name.is_empty() || path.is_empty() {
        return Err(format!(
            "invalid DLL override `{spec}`: name and path must not be empty"
        ));
    }
    Ok(DllOverride {
        name: crate::modules::normalize(name),
        path: PathBuf::from(path),
    })
}

/// Combine command-line overrides with `N2NP_DLL_OVERRIDES` (colon or
/// semicolon separated `NAME=PATH` entries). Later entries win.
pub fn collect_overrides(extra: &[String]) -> Result<Vec<DllOverride>, String> {
    let mut specs = Vec::new();
    if let Some(value) = std::env::var_os("N2NP_DLL_OVERRIDES") {
        let value = value.to_string_lossy();
        let separator = if value.contains(';') { ';' } else { ':' };
        specs.extend(
            value
                .split(separator)
                .filter(|part| !part.trim().is_empty())
                .map(str::to_string),
        );
    }
    specs.extend(extra.iter().cloned());

    let mut overrides: Vec<DllOverride> = Vec::new();
    for spec in specs {
        let entry = parse_override(&spec)?;
        overrides.retain(|existing| existing.name != entry.name);
        overrides.push(entry);
    }
    Ok(overrides)
}

pub fn set_overrides(overrides: Vec<DllOverride>) {
    let registry = OVERRIDES.get_or_init(|| Mutex::new(Vec::new()));
    *registry.lock().unwrap() = overrides;
}

pub fn overrides() -> Vec<DllOverride> {
    OVERRIDES
        .get()
        .map(|registry| registry.lock().unwrap().clone())
        .unwrap_or_default()
}

pub fn find_override(name: &str) -> Option<PathBuf> {
    let name = crate::modules::normalize(name);
    overrides()
        .into_iter()
        .find(|entry| entry.name == name)
        .map(|entry| entry.path)
}

/// Add prefix DLL search locations in Win32-like order.
pub fn push_search_paths(paths: &mut Vec<PathBuf>) {
    let prefix = Prefix::new(active_root());
    paths.push(prefix.system32());
    paths.push(prefix.syswow64());
    paths.push(prefix.windows_dir());
    paths.push(prefix.drive_c());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_override_specs() {
        let entry = parse_override("D3D11.dll=/tmp/d3d11.dll").unwrap();
        assert_eq!(entry.name, "d3d11.dll");
        assert_eq!(entry.path, PathBuf::from("/tmp/d3d11.dll"));
        assert!(parse_override("broken").is_err());
        assert!(parse_override("=x").is_err());
    }

    #[test]
    fn bootstrap_creates_layout() {
        let directory = tempfile::tempdir().unwrap();
        let prefix = Prefix::new(directory.path());
        prefix.ensure().unwrap();
        assert!(prefix.system32().is_dir());
        assert!(prefix.user_dir().is_dir());
        assert!(prefix.dosdevices().join("c:").symlink_metadata().is_ok());
    }
}
