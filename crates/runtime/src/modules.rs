use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

pub const BUILTIN_MODULES: &[&str] = &[
    "kernel32.dll",
    "kernelbase.dll",
    "user32.dll",
    "ntdll.dll",
    "msvcrt.dll",
    "ucrtbase.dll",
    "advapi32.dll",
    "gdi32.dll",
    "shell32.dll",
    "ole32.dll",
    "oleaut32.dll",
    "comdlg32.dll",
    "comctl32.dll",
    "shlwapi.dll",
    "ws2_32.dll",
    "winmm.dll",
    "version.dll",
    "psapi.dll",
    "dbghelp.dll",
    "bcrypt.dll",
    "crypt32.dll",
    "sechost.dll",
];

const BUILTIN_BASE: usize = 0x7FFE_0000_0000;

struct Registry {
    by_name: HashMap<String, usize>,
    by_handle: HashMap<usize, String>,
    paths: HashMap<usize, String>,
    next_handle: usize,
    main_handle: usize,
}

static REGISTRY: OnceLock<Mutex<Registry>> = OnceLock::new();
static LOADER: OnceLock<Mutex<Option<fn(&str) -> Option<usize>>>> = OnceLock::new();
static EXPORT_RESOLVER: OnceLock<Mutex<Option<fn(usize, &str) -> Option<usize>>>> = OnceLock::new();

fn registry() -> &'static Mutex<Registry> {
    REGISTRY.get_or_init(|| {
        let mut by_name = HashMap::new();
        let mut by_handle = HashMap::new();
        for (index, name) in BUILTIN_MODULES.iter().enumerate() {
            let handle = BUILTIN_BASE + (index + 1) * 0x1_0000;
            by_name.insert((*name).to_string(), handle);
            by_handle.insert(handle, (*name).to_string());
        }
        Mutex::new(Registry {
            by_name,
            by_handle,
            paths: HashMap::new(),
            next_handle: 0x1000_0000,
            main_handle: 0,
        })
    })
}

pub fn normalize(name: &str) -> String {
    let base = name
        .rsplit(['\\', '/'])
        .next()
        .unwrap_or(name)
        .trim()
        .to_ascii_lowercase();
    if base.contains('.') {
        base
    } else {
        format!("{base}.dll")
    }
}

pub fn is_builtin(name: &str) -> bool {
    let name = normalize(name);
    BUILTIN_MODULES.contains(&name.as_str())
}

pub fn is_builtin_handle(handle: usize) -> bool {
    handle >= BUILTIN_BASE && handle < BUILTIN_BASE + 0x1_0000 * 0x1000
}

pub fn handle_for(name: &str) -> Option<usize> {
    registry()
        .lock()
        .unwrap()
        .by_name
        .get(&normalize(name))
        .copied()
}

pub fn name_for(handle: usize) -> Option<String> {
    registry().lock().unwrap().by_handle.get(&handle).cloned()
}

pub fn is_known_handle(handle: usize) -> bool {
    registry().lock().unwrap().by_handle.contains_key(&handle)
}

pub fn register(name: &str, handle: usize) {
    let mut registry = registry().lock().unwrap();
    registry.by_name.insert(normalize(name), handle);
    registry.by_handle.insert(handle, normalize(name));
}

pub fn set_path(handle: usize, path: &str) {
    registry()
        .lock()
        .unwrap()
        .paths
        .insert(handle, path.to_string());
}

pub fn path_for(handle: usize) -> Option<String> {
    registry().lock().unwrap().paths.get(&handle).cloned()
}

pub fn allocate_handle() -> usize {
    let mut registry = registry().lock().unwrap();
    let handle = registry.next_handle;
    registry.next_handle += 0x1_0000;
    handle
}

pub fn set_main(handle: usize) {
    registry().lock().unwrap().main_handle = handle;
}

pub fn main_handle() -> usize {
    registry().lock().unwrap().main_handle
}

pub fn set_loader(loader: fn(&str) -> Option<usize>) {
    *LOADER.get_or_init(|| Mutex::new(None)).lock().unwrap() = Some(loader);
}

pub fn load_library(name: &str) -> Option<usize> {
    if let Some(handle) = handle_for(name) {
        return Some(handle);
    }
    let loader = *LOADER.get_or_init(|| Mutex::new(None)).lock().unwrap();
    let handle = loader?(name)?;
    register(name, handle);
    Some(handle)
}

pub fn set_export_resolver(resolver: fn(usize, &str) -> Option<usize>) {
    *EXPORT_RESOLVER
        .get_or_init(|| Mutex::new(None))
        .lock()
        .unwrap() = Some(resolver);
}

pub fn resolve_export(handle: usize, name: &str) -> Option<usize> {
    let resolver = *EXPORT_RESOLVER
        .get_or_init(|| Mutex::new(None))
        .lock()
        .unwrap();
    resolver?(handle, name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_windows_paths() {
        assert_eq!(
            normalize(r"C:\Windows\System32\KERNEL32.dll"),
            "kernel32.dll"
        );
        assert_eq!(normalize("USER32.DLL"), "user32.dll");
        assert_eq!(
            normalize("api-ms-win-core-file-l1-1-0.dll"),
            "api-ms-win-core-file-l1-1-0.dll"
        );
    }

    #[test]
    fn registers_and_looks_up_modules() {
        register("test_module.dll", 0x1234_5678);
        assert_eq!(handle_for("TEST_MODULE.DLL"), Some(0x1234_5678));
        assert_eq!(name_for(0x1234_5678).as_deref(), Some("test_module.dll"));
        assert!(handle_for("missing_module.dll").is_none());
    }
}
