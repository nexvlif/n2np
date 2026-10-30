const E_FILE_NOT_FOUND: i32 = 2;
const E_ACCESS_DENIED: i32 = 5;
const E_INVALID_HANDLE: i32 = 6;
const E_INVALID_PARAMETER: i32 = 87;
const E_MORE_DATA: i32 = 234;
const E_NO_MORE_ITEMS: i32 = 259;
use crate::utf;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::ffi::c_void;
use std::fs;
use std::path::PathBuf;
use std::ptr;
use std::slice;
use std::sync::{Mutex, OnceLock};

#[allow(dead_code)]
pub const HKEY_CLASSES_ROOT: usize = 0x8000_0000;
#[allow(dead_code)]
pub const HKEY_CURRENT_USER: usize = 0x8000_0001;
#[allow(dead_code)]
pub const HKEY_LOCAL_MACHINE: usize = 0x8000_0002;
#[allow(dead_code)]
pub const HKEY_USERS: usize = 0x8000_0003;
#[allow(dead_code)]
pub const HKEY_CURRENT_CONFIG: usize = 0x8000_0005;

const REG_SZ: u32 = 1;
const REG_EXPAND_SZ: u32 = 2;
#[allow(dead_code)]
const REG_DWORD: u32 = 4;

const HANDLE_BASE: usize = 0x9100_0000;
const MAX_VALUE_SIZE: usize = 16 << 20;
const REG_CREATED_NEW_KEY: u32 = 1;
const REG_OPENED_EXISTING_KEY: u32 = 2;

#[derive(Clone, Serialize, Deserialize)]
enum ValueData {
    Bytes(Vec<u8>),
    Text(String),
}

#[derive(Clone, Serialize, Deserialize)]
struct Value {
    kind: u32,
    data: ValueData,
}

impl Value {
    fn encode(&self, wide: bool) -> Vec<u8> {
        match &self.data {
            ValueData::Bytes(bytes) => bytes.clone(),
            ValueData::Text(text) if wide => text
                .encode_utf16()
                .chain(std::iter::once(0))
                .flat_map(|unit| unit.to_le_bytes())
                .collect(),
            ValueData::Text(text) => {
                let mut bytes = text.as_bytes().to_vec();
                bytes.push(0);
                bytes
            }
        }
    }
}

#[derive(Clone, Default, Serialize, Deserialize)]
struct KeyNode {
    #[serde(default)]
    values: BTreeMap<String, Value>,
    #[serde(default)]
    subkeys: BTreeMap<String, KeyNode>,
}

impl KeyNode {
    fn get(&self, path: &[String]) -> Option<&KeyNode> {
        path.iter()
            .try_fold(self, |node, name| node.subkeys.get(name))
    }

    fn get_mut(&mut self, path: &[String]) -> Option<&mut KeyNode> {
        if path.is_empty() {
            return Some(self);
        }
        let (first, rest) = path.split_first()?;
        self.subkeys.get_mut(first)?.get_mut(rest)
    }

    fn ensure(&mut self, path: &[String]) -> &mut KeyNode {
        if path.is_empty() {
            return self;
        }
        let (first, rest) = path.split_first().unwrap();
        self.subkeys.entry(first.clone()).or_default().ensure(rest)
    }
}

#[derive(Clone, PartialEq, Eq)]
struct KeyPath {
    root: u8,
    path: Vec<String>,
}

struct Store {
    roots: Vec<KeyNode>,
    handles: HashMap<usize, KeyPath>,
    next_handle: usize,
}

impl Store {
    fn load() -> Self {
        let mut store = Self {
            roots: vec![KeyNode::default(); 5],
            handles: HashMap::new(),
            next_handle: HANDLE_BASE,
        };
        if let Some(path) = persistence_path() {
            if let Ok(data) = fs::read(&path) {
                match serde_json::from_slice::<Vec<KeyNode>>(&data) {
                    Ok(roots) if roots.len() == 5 => store.roots = roots,
                    Ok(_) => log::warn!("[n2np-registry] ignoring registry with unexpected shape"),
                    Err(err) => {
                        log::warn!("[n2np-registry] failed to parse {}: {err}", path.display())
                    }
                }
            }
        }
        store
    }

    fn node(&self, path: &KeyPath) -> Option<&KeyNode> {
        self.roots.get(path.root as usize)?.get(&path.path)
    }

    fn node_mut(&mut self, path: &KeyPath) -> Option<&mut KeyNode> {
        self.roots.get_mut(path.root as usize)?.get_mut(&path.path)
    }

    fn allocate(&mut self, path: KeyPath) -> usize {
        let handle = self.next_handle;
        self.next_handle = self.next_handle.wrapping_add(0x10);
        self.handles.insert(handle, path);
        handle
    }
}

static STORE: OnceLock<Mutex<Store>> = OnceLock::new();

fn store() -> &'static Mutex<Store> {
    STORE.get_or_init(|| Mutex::new(Store::load()))
}

fn persistence_path() -> Option<PathBuf> {
    if cfg!(test) {
        return None;
    }
    if let Some(path) = std::env::var_os("N2NP_REGISTRY") {
        let path = PathBuf::from(path);
        if path.as_os_str() == ":memory:" {
            return None;
        }
        return Some(path);
    }
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))?;
    Some(PathBuf::from(home).join(".n2np").join("registry.json"))
}

fn persist(store: &Store) {
    let Some(path) = persistence_path() else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    match serde_json::to_vec_pretty(&store.roots) {
        Ok(data) => {
            if let Err(err) = fs::write(&path, data) {
                log::warn!("[n2np-registry] failed to write {}: {err}", path.display());
            }
        }
        Err(err) => log::warn!("[n2np-registry] failed to encode registry: {err}"),
    }
}

fn root_index(handle: usize) -> Option<u8> {
    let low = (handle & 0xFFFF_FFFF) as u32;
    let high = if usize::BITS > 32 { handle >> 32 } else { 0 };
    if high != 0 && high != 0xFFFF_FFFF {
        return None;
    }
    match low {
        0x8000_0000 => Some(0),
        0x8000_0001 => Some(1),
        0x8000_0002 => Some(2),
        0x8000_0003 => Some(3),
        0x8000_0005 => Some(4),
        _ => None,
    }
}

fn resolve_path(base: &KeyPath, subkey: &str) -> KeyPath {
    let mut path = base.path.clone();
    for component in subkey.split(['\\', '/']) {
        match component {
            "" | "." => {}
            ".." => {
                path.pop();
            }
            name => path.push(name.to_ascii_lowercase()),
        }
    }
    KeyPath {
        root: base.root,
        path,
    }
}

fn base_path(store: &Store, handle: usize) -> Option<KeyPath> {
    if let Some(root) = root_index(handle) {
        return Some(KeyPath {
            root,
            path: Vec::new(),
        });
    }
    store.handles.get(&handle).cloned()
}

fn open_existing(store: &Store, handle: usize, subkey: &str) -> Result<KeyPath, i32> {
    let base = base_path(store, handle).ok_or(E_INVALID_HANDLE)?;
    let path = resolve_path(&base, subkey);
    if store.node(&path).is_some() {
        Ok(path)
    } else {
        Err(E_FILE_NOT_FOUND)
    }
}

fn create_key(store: &mut Store, handle: usize, subkey: &str) -> Result<(KeyPath, bool), i32> {
    let base = base_path(store, handle).ok_or(E_INVALID_HANDLE)?;
    let path = resolve_path(&base, subkey);
    let existed = store.node(&path).is_some();
    let root = store
        .roots
        .get_mut(path.root as usize)
        .ok_or(E_INVALID_HANDLE)?;
    root.ensure(&path.path);
    Ok((path, existed))
}

fn find_value<'a>(node: &'a KeyNode, name: &str) -> Option<&'a Value> {
    node.values.get(&name.to_ascii_lowercase())
}

fn set_value_on_path(
    store: &mut Store,
    path: &KeyPath,
    name: &str,
    kind: u32,
    bytes: &[u8],
    wide: bool,
) -> i32 {
    let value = match kind {
        REG_SZ | REG_EXPAND_SZ => {
            let text = if wide {
                let units: Vec<u16> = bytes
                    .chunks_exact(2)
                    .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
                    .take_while(|unit| *unit != 0)
                    .collect();
                String::from_utf16_lossy(&units)
            } else {
                let end = bytes
                    .iter()
                    .position(|byte| *byte == 0)
                    .unwrap_or(bytes.len());
                String::from_utf8_lossy(&bytes[..end]).into_owned()
            };
            Value {
                kind,
                data: ValueData::Text(text),
            }
        }
        _ => Value {
            kind,
            data: ValueData::Bytes(bytes.to_vec()),
        },
    };
    let node = match store.node_mut(path) {
        Some(node) => node,
        None => return E_FILE_NOT_FOUND,
    };
    node.values.insert(name.to_ascii_lowercase(), value);
    0
}

unsafe fn query_value_on_path(
    store: &Store,
    path: &KeyPath,
    name: &str,
    type_out: *mut u32,
    data: *mut c_void,
    size: *mut u32,
    wide: bool,
) -> i32 {
    if size.is_null() {
        return E_INVALID_PARAMETER;
    }
    let node = match store.node(path) {
        Some(node) => node,
        None => return E_FILE_NOT_FOUND,
    };
    let value = match find_value(node, name) {
        Some(value) => value,
        None => return E_FILE_NOT_FOUND,
    };
    if !type_out.is_null() {
        *type_out = value.kind;
    }
    copy_bytes(data, size, &value.encode(wide))
}

unsafe fn copy_bytes(data: *mut c_void, size: *mut u32, bytes: &[u8]) -> i32 {
    if data.is_null() {
        if !size.is_null() {
            *size = bytes.len() as u32;
        }
        return 0;
    }
    if size.is_null() {
        return E_INVALID_PARAMETER;
    }
    let capacity = (*size as usize).min(MAX_VALUE_SIZE);
    if capacity < bytes.len() {
        *size = bytes.len() as u32;
        return E_MORE_DATA;
    }
    if !bytes.is_empty() {
        ptr::copy_nonoverlapping(bytes.as_ptr(), data as *mut u8, bytes.len());
    }
    *size = bytes.len() as u32;
    0
}

unsafe fn copy_name(dst: *mut c_void, size: *mut u32, name: &str, wide: bool) -> i32 {
    let units = if wide {
        name.encode_utf16().count()
    } else {
        name.len()
    };
    if dst.is_null() {
        if !size.is_null() {
            *size = (units + 1) as u32;
        }
        return 0;
    }
    if size.is_null() {
        return E_INVALID_PARAMETER;
    }
    if (*size as usize) < units + 1 {
        *size = (units + 1) as u32;
        return E_MORE_DATA;
    }
    if wide {
        utf::write_wide(dst as *mut u16, name, *size as usize);
    } else {
        utf::write_ansi(dst as *mut u8, name, *size as usize);
    }
    *size = units as u32;
    0
}

unsafe fn read_raw(data: *const u8, size: u32) -> Vec<u8> {
    if data.is_null() || size == 0 {
        Vec::new()
    } else {
        slice::from_raw_parts(data, (size as usize).min(MAX_VALUE_SIZE)).to_vec()
    }
}

unsafe fn open_result(store: &mut Store, path: KeyPath, result: *mut usize) -> i32 {
    if result.is_null() {
        return E_INVALID_PARAMETER;
    }
    *result = store.allocate(path);
    0
}

winapi_api!(
    "advapi32.dll",
    "RegOpenKeyExA",
    pub unsafe extern "win64" fn reg_open_key_ex_a(
        hkey: usize,
        sub_key: *const u8,
        _options: u32,
        _sam: u32,
        result: *mut usize,
    ) -> i32 {
        let sub_key = utf::read_ansi(sub_key);
        let mut store = store().lock().unwrap();
        match open_existing(&store, hkey, &sub_key) {
            Ok(path) => open_result(&mut store, path, result),
            Err(code) => code,
        }
    }
);

winapi_api!(
    "advapi32.dll",
    "RegOpenKeyExW",
    pub unsafe extern "win64" fn reg_open_key_ex_w(
        hkey: usize,
        sub_key: *const u16,
        _options: u32,
        _sam: u32,
        result: *mut usize,
    ) -> i32 {
        let sub_key = utf::read_wide(sub_key);
        let mut store = store().lock().unwrap();
        match open_existing(&store, hkey, &sub_key) {
            Ok(path) => open_result(&mut store, path, result),
            Err(code) => code,
        }
    }
);

winapi_api!(
    "advapi32.dll",
    "RegCreateKeyExA",
    pub unsafe extern "win64" fn reg_create_key_ex_a(
        hkey: usize,
        sub_key: *const u8,
        _reserved: u32,
        _class: *const u8,
        _options: u32,
        _sam: u32,
        _security: *mut c_void,
        result: *mut usize,
        disposition: *mut u32,
    ) -> i32 {
        let sub_key = utf::read_ansi(sub_key);
        let mut store = store().lock().unwrap();
        match create_key(&mut store, hkey, &sub_key) {
            Ok((path, existed)) => {
                if !disposition.is_null() {
                    *disposition = if existed {
                        REG_OPENED_EXISTING_KEY
                    } else {
                        REG_CREATED_NEW_KEY
                    };
                }
                let code = open_result(&mut store, path, result);
                if code == 0 && !existed {
                    persist(&store);
                }
                code
            }
            Err(code) => code,
        }
    }
);

winapi_api!(
    "advapi32.dll",
    "RegCreateKeyExW",
    pub unsafe extern "win64" fn reg_create_key_ex_w(
        hkey: usize,
        sub_key: *const u16,
        _reserved: u32,
        _class: *const u16,
        _options: u32,
        _sam: u32,
        _security: *mut c_void,
        result: *mut usize,
        disposition: *mut u32,
    ) -> i32 {
        let sub_key = utf::read_wide(sub_key);
        let mut store = store().lock().unwrap();
        match create_key(&mut store, hkey, &sub_key) {
            Ok((path, existed)) => {
                if !disposition.is_null() {
                    *disposition = if existed {
                        REG_OPENED_EXISTING_KEY
                    } else {
                        REG_CREATED_NEW_KEY
                    };
                }
                let code = open_result(&mut store, path, result);
                if code == 0 && !existed {
                    persist(&store);
                }
                code
            }
            Err(code) => code,
        }
    }
);

winapi_api!(
    "advapi32.dll",
    "RegCloseKey",
    pub unsafe extern "win64" fn reg_close_key(hkey: usize) -> i32 {
        if root_index(hkey).is_some() {
            return 0;
        }
        let mut store = store().lock().unwrap();
        if store.handles.remove(&hkey).is_some() {
            0
        } else {
            E_INVALID_HANDLE
        }
    }
);

winapi_api!(
    "advapi32.dll",
    "RegQueryValueExA",
    pub unsafe extern "win64" fn reg_query_value_ex_a(
        hkey: usize,
        value_name: *const u8,
        _reserved: *mut u32,
        type_out: *mut u32,
        data: *mut u8,
        size: *mut u32,
    ) -> i32 {
        let name = utf::read_ansi(value_name);
        let store = store().lock().unwrap();
        let path = match base_path(&store, hkey) {
            Some(path) => path,
            None => return E_INVALID_HANDLE,
        };
        query_value_on_path(
            &store,
            &path,
            &name,
            type_out,
            data as *mut c_void,
            size,
            false,
        )
    }
);

winapi_api!(
    "advapi32.dll",
    "RegQueryValueExW",
    pub unsafe extern "win64" fn reg_query_value_ex_w(
        hkey: usize,
        value_name: *const u16,
        _reserved: *mut u32,
        type_out: *mut u32,
        data: *mut u8,
        size: *mut u32,
    ) -> i32 {
        let name = utf::read_wide(value_name);
        let store = store().lock().unwrap();
        let path = match base_path(&store, hkey) {
            Some(path) => path,
            None => return E_INVALID_HANDLE,
        };
        query_value_on_path(
            &store,
            &path,
            &name,
            type_out,
            data as *mut c_void,
            size,
            true,
        )
    }
);

winapi_api!(
    "advapi32.dll",
    "RegSetValueExA",
    pub unsafe extern "win64" fn reg_set_value_ex_a(
        hkey: usize,
        value_name: *const u8,
        _reserved: u32,
        kind: u32,
        data: *const u8,
        size: u32,
    ) -> i32 {
        let name = utf::read_ansi(value_name);
        let bytes = read_raw(data, size);
        let mut store = store().lock().unwrap();
        let path = match base_path(&store, hkey) {
            Some(path) => path,
            None => return E_INVALID_HANDLE,
        };
        let code = set_value_on_path(&mut store, &path, &name, kind, &bytes, false);
        if code == 0 {
            persist(&store);
        }
        code
    }
);

winapi_api!(
    "advapi32.dll",
    "RegSetValueExW",
    pub unsafe extern "win64" fn reg_set_value_ex_w(
        hkey: usize,
        value_name: *const u16,
        _reserved: u32,
        kind: u32,
        data: *const u8,
        size: u32,
    ) -> i32 {
        let name = utf::read_wide(value_name);
        let bytes = read_raw(data, size);
        let mut store = store().lock().unwrap();
        let path = match base_path(&store, hkey) {
            Some(path) => path,
            None => return E_INVALID_HANDLE,
        };
        let code = set_value_on_path(&mut store, &path, &name, kind, &bytes, true);
        if code == 0 {
            persist(&store);
        }
        code
    }
);

winapi_api!(
    "advapi32.dll",
    "RegDeleteValueA",
    pub unsafe extern "win64" fn reg_delete_value_a(hkey: usize, value_name: *const u8) -> i32 {
        let name = utf::read_ansi(value_name).to_ascii_lowercase();
        let mut store = store().lock().unwrap();
        let path = match base_path(&store, hkey) {
            Some(path) => path,
            None => return E_INVALID_HANDLE,
        };
        let removed = match store.node_mut(&path) {
            Some(node) => node.values.remove(&name).is_some(),
            None => return E_FILE_NOT_FOUND,
        };
        if removed {
            persist(&store);
            0
        } else {
            E_FILE_NOT_FOUND
        }
    }
);

winapi_api!(
    "advapi32.dll",
    "RegDeleteValueW",
    pub unsafe extern "win64" fn reg_delete_value_w(hkey: usize, value_name: *const u16) -> i32 {
        let name = utf::read_wide(value_name).to_ascii_lowercase();
        let mut store = store().lock().unwrap();
        let path = match base_path(&store, hkey) {
            Some(path) => path,
            None => return E_INVALID_HANDLE,
        };
        let removed = match store.node_mut(&path) {
            Some(node) => node.values.remove(&name).is_some(),
            None => return E_FILE_NOT_FOUND,
        };
        if removed {
            persist(&store);
            0
        } else {
            E_FILE_NOT_FOUND
        }
    }
);

fn delete_key(store: &mut Store, hkey: usize, subkey: &str) -> i32 {
    let path = match open_existing(store, hkey, subkey) {
        Ok(path) => path,
        Err(code) => return code,
    };
    if path.path.is_empty() {
        return E_INVALID_PARAMETER;
    }
    match store.node(&path) {
        Some(node) if !node.subkeys.is_empty() => return E_ACCESS_DENIED,
        Some(_) => {}
        None => return E_FILE_NOT_FOUND,
    }
    let (last, parent) = path.path.split_last().unwrap();
    let parent_path = KeyPath {
        root: path.root,
        path: parent.to_vec(),
    };
    if let Some(node) = store.node_mut(&parent_path) {
        node.subkeys.remove(last);
    }
    store.handles.retain(|_, handle_path| handle_path != &path);
    persist(store);
    0
}

winapi_api!(
    "advapi32.dll",
    "RegDeleteKeyA",
    pub unsafe extern "win64" fn reg_delete_key_a(hkey: usize, sub_key: *const u8) -> i32 {
        let sub_key = utf::read_ansi(sub_key);
        let mut store = store().lock().unwrap();
        delete_key(&mut store, hkey, &sub_key)
    }
);

winapi_api!(
    "advapi32.dll",
    "RegDeleteKeyW",
    pub unsafe extern "win64" fn reg_delete_key_w(hkey: usize, sub_key: *const u16) -> i32 {
        let sub_key = utf::read_wide(sub_key);
        let mut store = store().lock().unwrap();
        delete_key(&mut store, hkey, &sub_key)
    }
);

winapi_api!(
    "advapi32.dll",
    "RegDeleteKeyExA",
    pub unsafe extern "win64" fn reg_delete_key_ex_a(
        hkey: usize,
        sub_key: *const u8,
        _sam: u32,
        _reserved: u32,
    ) -> i32 {
        reg_delete_key_a(hkey, sub_key)
    }
);

winapi_api!(
    "advapi32.dll",
    "RegDeleteKeyExW",
    pub unsafe extern "win64" fn reg_delete_key_ex_w(
        hkey: usize,
        sub_key: *const u16,
        _sam: u32,
        _reserved: u32,
    ) -> i32 {
        reg_delete_key_w(hkey, sub_key)
    }
);

unsafe fn enum_key(
    store: &Store,
    hkey: usize,
    index: u32,
    name: *mut c_void,
    name_size: *mut u32,
    class: *mut c_void,
    class_size: *mut u32,
    wide: bool,
) -> i32 {
    let path = match base_path(store, hkey) {
        Some(path) => path,
        None => return E_INVALID_HANDLE,
    };
    let node = match store.node(&path) {
        Some(node) => node,
        None => return E_INVALID_HANDLE,
    };
    let Some((key, _)) = node.subkeys.iter().nth(index as usize) else {
        return E_NO_MORE_ITEMS;
    };
    let code = copy_name(name, name_size, key, wide);
    if code != 0 {
        return code;
    }
    if !class_size.is_null() {
        *class_size = 0;
    }
    if !class.is_null() && !class_size.is_null() {
        if wide {
            *(class as *mut u16) = 0;
        } else {
            *(class as *mut u8) = 0;
        }
    }
    0
}

winapi_api!(
    "advapi32.dll",
    "RegEnumKeyExA",
    pub unsafe extern "win64" fn reg_enum_key_ex_a(
        hkey: usize,
        index: u32,
        name: *mut u8,
        name_size: *mut u32,
        _reserved: *mut u32,
        class: *mut u8,
        class_size: *mut u32,
        _last_write: *mut u64,
    ) -> i32 {
        let store = store().lock().unwrap();
        enum_key(
            &store,
            hkey,
            index,
            name as *mut c_void,
            name_size,
            class as *mut c_void,
            class_size,
            false,
        )
    }
);

winapi_api!(
    "advapi32.dll",
    "RegEnumKeyExW",
    pub unsafe extern "win64" fn reg_enum_key_ex_w(
        hkey: usize,
        index: u32,
        name: *mut u16,
        name_size: *mut u32,
        _reserved: *mut u32,
        class: *mut u16,
        class_size: *mut u32,
        _last_write: *mut u64,
    ) -> i32 {
        let store = store().lock().unwrap();
        enum_key(
            &store,
            hkey,
            index,
            name as *mut c_void,
            name_size,
            class as *mut c_void,
            class_size,
            true,
        )
    }
);

unsafe fn enum_value(
    store: &Store,
    hkey: usize,
    index: u32,
    name: *mut c_void,
    name_size: *mut u32,
    type_out: *mut u32,
    data: *mut c_void,
    data_size: *mut u32,
    wide: bool,
) -> i32 {
    let path = match base_path(store, hkey) {
        Some(path) => path,
        None => return E_INVALID_HANDLE,
    };
    let node = match store.node(&path) {
        Some(node) => node,
        None => return E_INVALID_HANDLE,
    };
    let Some((value_name, value)) = node.values.iter().nth(index as usize) else {
        return E_NO_MORE_ITEMS;
    };
    if !name.is_null() {
        let code = copy_name(name, name_size, value_name, wide);
        if code != 0 {
            return code;
        }
    }
    if !type_out.is_null() {
        *type_out = value.kind;
    }
    copy_bytes(data, data_size, &value.encode(wide))
}

winapi_api!(
    "advapi32.dll",
    "RegEnumValueA",
    pub unsafe extern "win64" fn reg_enum_value_a(
        hkey: usize,
        index: u32,
        name: *mut u8,
        name_size: *mut u32,
        _reserved: *mut u32,
        type_out: *mut u32,
        data: *mut u8,
        data_size: *mut u32,
    ) -> i32 {
        let store = store().lock().unwrap();
        enum_value(
            &store,
            hkey,
            index,
            name as *mut c_void,
            name_size,
            type_out,
            data as *mut c_void,
            data_size,
            false,
        )
    }
);

winapi_api!(
    "advapi32.dll",
    "RegEnumValueW",
    pub unsafe extern "win64" fn reg_enum_value_w(
        hkey: usize,
        index: u32,
        name: *mut u16,
        name_size: *mut u32,
        _reserved: *mut u32,
        type_out: *mut u32,
        data: *mut u8,
        data_size: *mut u32,
    ) -> i32 {
        let store = store().lock().unwrap();
        enum_value(
            &store,
            hkey,
            index,
            name as *mut c_void,
            name_size,
            type_out,
            data as *mut c_void,
            data_size,
            true,
        )
    }
);

winapi_api!(
    "advapi32.dll",
    "RegQueryInfoKeyA",
    pub unsafe extern "win64" fn reg_query_info_key_a(
        hkey: usize,
        subkeys: *mut u32,
        max_subkey_len: *mut u32,
        max_class_len: *mut u32,
        values: *mut u32,
        max_value_name_len: *mut u32,
        max_value_len: *mut u32,
        _security: *mut u32,
        last_write: *mut u64,
    ) -> i32 {
        query_info_key(
            hkey,
            subkeys,
            max_subkey_len,
            max_class_len,
            values,
            max_value_name_len,
            max_value_len,
            last_write,
            false,
        )
    }
);

winapi_api!(
    "advapi32.dll",
    "RegQueryInfoKeyW",
    pub unsafe extern "win64" fn reg_query_info_key_w(
        hkey: usize,
        subkeys: *mut u32,
        max_subkey_len: *mut u32,
        max_class_len: *mut u32,
        values: *mut u32,
        max_value_name_len: *mut u32,
        max_value_len: *mut u32,
        _security: *mut u32,
        last_write: *mut u64,
    ) -> i32 {
        query_info_key(
            hkey,
            subkeys,
            max_subkey_len,
            max_class_len,
            values,
            max_value_name_len,
            max_value_len,
            last_write,
            true,
        )
    }
);

#[allow(clippy::too_many_arguments)]
unsafe fn query_info_key(
    hkey: usize,
    subkeys: *mut u32,
    max_subkey_len: *mut u32,
    max_class_len: *mut u32,
    values: *mut u32,
    max_value_name_len: *mut u32,
    max_value_len: *mut u32,
    last_write: *mut u64,
    wide: bool,
) -> i32 {
    let store = store().lock().unwrap();
    let path = match base_path(&store, hkey) {
        Some(path) => path,
        None => return E_INVALID_HANDLE,
    };
    let node = match store.node(&path) {
        Some(node) => node,
        None => return E_INVALID_HANDLE,
    };
    if !subkeys.is_null() {
        *subkeys = node.subkeys.len() as u32;
    }
    if !values.is_null() {
        *values = node.values.len() as u32;
    }
    if !max_subkey_len.is_null() {
        *max_subkey_len = node
            .subkeys
            .keys()
            .map(|name| {
                if wide {
                    name.encode_utf16().count()
                } else {
                    name.len()
                }
            })
            .max()
            .unwrap_or(0) as u32;
    }
    if !max_class_len.is_null() {
        *max_class_len = 0;
    }
    if !max_value_name_len.is_null() {
        *max_value_name_len = node
            .values
            .keys()
            .map(|name| {
                if wide {
                    name.encode_utf16().count()
                } else {
                    name.len()
                }
            })
            .max()
            .unwrap_or(0) as u32;
    }
    if !max_value_len.is_null() {
        *max_value_len = node
            .values
            .values()
            .map(|value| value.encode(wide).len())
            .max()
            .unwrap_or(0) as u32;
    }
    if !last_write.is_null() {
        *last_write = 0;
    }
    0
}

winapi_api!(
    "advapi32.dll",
    "RegGetValueA",
    pub unsafe extern "win64" fn reg_get_value_a(
        hkey: usize,
        sub_key: *const u8,
        value_name: *const u8,
        _flags: u32,
        type_out: *mut u32,
        data: *mut u8,
        size: *mut u32,
    ) -> i32 {
        let sub_key = utf::read_ansi(sub_key);
        let name = utf::read_ansi(value_name);
        let store = store().lock().unwrap();
        let path = match open_existing(&store, hkey, &sub_key) {
            Ok(path) => path,
            Err(code) => return code,
        };
        query_value_on_path(
            &store,
            &path,
            &name,
            type_out,
            data as *mut c_void,
            size,
            false,
        )
    }
);

winapi_api!(
    "advapi32.dll",
    "RegGetValueW",
    pub unsafe extern "win64" fn reg_get_value_w(
        hkey: usize,
        sub_key: *const u16,
        value_name: *const u16,
        _flags: u32,
        type_out: *mut u32,
        data: *mut u8,
        size: *mut u32,
    ) -> i32 {
        let sub_key = utf::read_wide(sub_key);
        let name = utf::read_wide(value_name);
        let store = store().lock().unwrap();
        let path = match open_existing(&store, hkey, &sub_key) {
            Ok(path) => path,
            Err(code) => return code,
        };
        query_value_on_path(
            &store,
            &path,
            &name,
            type_out,
            data as *mut c_void,
            size,
            true,
        )
    }
);

winapi_api!(
    "advapi32.dll",
    "RegSetKeyValueA",
    pub unsafe extern "win64" fn reg_set_key_value_a(
        hkey: usize,
        sub_key: *const u8,
        value_name: *const u8,
        kind: u32,
        data: *const u8,
        size: u32,
    ) -> i32 {
        let sub_key = utf::read_ansi(sub_key);
        let name = utf::read_ansi(value_name);
        let bytes = read_raw(data, size);
        let mut store = store().lock().unwrap();
        let path = match create_key(&mut store, hkey, &sub_key) {
            Ok((path, _)) => path,
            Err(code) => return code,
        };
        let code = set_value_on_path(&mut store, &path, &name, kind, &bytes, false);
        if code == 0 {
            persist(&store);
        }
        code
    }
);

winapi_api!(
    "advapi32.dll",
    "RegSetKeyValueW",
    pub unsafe extern "win64" fn reg_set_key_value_w(
        hkey: usize,
        sub_key: *const u16,
        value_name: *const u16,
        kind: u32,
        data: *const u8,
        size: u32,
    ) -> i32 {
        let sub_key = utf::read_wide(sub_key);
        let name = utf::read_wide(value_name);
        let bytes = read_raw(data, size);
        let mut store = store().lock().unwrap();
        let path = match create_key(&mut store, hkey, &sub_key) {
            Ok((path, _)) => path,
            Err(code) => return code,
        };
        let code = set_value_on_path(&mut store, &path, &name, kind, &bytes, true);
        if code == 0 {
            persist(&store);
        }
        code
    }
);

winapi_api!(
    "advapi32.dll",
    "RegFlushKey",
    pub unsafe extern "win64" fn reg_flush_key(hkey: usize) -> i32 {
        let store = store().lock().unwrap();
        if base_path(&store, hkey).is_none() {
            return E_INVALID_HANDLE;
        }
        persist(&store);
        0
    }
);

winapi_api!(
    "advapi32.dll",
    "RegNotifyChangeKeyValue",
    pub unsafe extern "win64" fn reg_notify_change_key_value(
        hkey: usize,
        _watch_subtree: i32,
        _filter: u32,
        _event: usize,
        _async: i32,
    ) -> i32 {
        let store = store().lock().unwrap();
        if base_path(&store, hkey).is_some() {
            0
        } else {
            E_INVALID_HANDLE
        }
    }
);

#[cfg(test)]
mod tests {
    use super::*;

    fn wide(text: &str) -> Vec<u16> {
        utf::wide_with_nul(text)
    }

    #[test]
    fn string_value_round_trips_between_a_and_w() {
        let subkey = format!("Software\\n2np-test-{}-string", std::process::id());
        let subkey_w = wide(&subkey);
        let name_w = wide("Greeting");
        let value_w = wide("hello");
        unsafe {
            let mut key = 0usize;
            let mut disposition = 0u32;
            assert_eq!(
                reg_create_key_ex_w(
                    HKEY_CURRENT_USER,
                    subkey_w.as_ptr(),
                    0,
                    ptr::null(),
                    0,
                    0,
                    ptr::null_mut(),
                    &mut key,
                    &mut disposition,
                ),
                0
            );
            assert_eq!(disposition, REG_CREATED_NEW_KEY);
            assert_eq!(
                reg_set_value_ex_w(
                    key,
                    name_w.as_ptr(),
                    0,
                    REG_SZ,
                    value_w.as_ptr() as *const u8,
                    (value_w.len() * 2) as u32,
                ),
                0
            );
            let mut kind = 0u32;
            let mut size = 0u32;
            assert_eq!(
                reg_query_value_ex_a(
                    key,
                    b"Greeting\0".as_ptr(),
                    ptr::null_mut(),
                    &mut kind,
                    ptr::null_mut(),
                    &mut size,
                ),
                0
            );
            assert_eq!(kind, REG_SZ);
            assert_eq!(size, 6);
            let mut buffer = vec![0u8; size as usize];
            assert_eq!(
                reg_query_value_ex_a(
                    key,
                    b"Greeting\0".as_ptr(),
                    ptr::null_mut(),
                    &mut kind,
                    buffer.as_mut_ptr(),
                    &mut size,
                ),
                0
            );
            assert_eq!(&buffer[..5], b"hello");
            assert_eq!(reg_close_key(key), 0);
            assert_eq!(reg_delete_key_w(HKEY_CURRENT_USER, subkey_w.as_ptr()), 0);
        }
    }

    #[test]
    fn enumerates_keys_and_values() {
        let subkey = format!("Software\\n2np-test-{}-enum", std::process::id());
        let subkey_w = wide(&subkey);
        unsafe {
            let mut key = 0usize;
            let mut disposition = 0u32;
            assert_eq!(
                reg_create_key_ex_w(
                    HKEY_CURRENT_USER,
                    subkey_w.as_ptr(),
                    0,
                    ptr::null(),
                    0,
                    0,
                    ptr::null_mut(),
                    &mut key,
                    &mut disposition,
                ),
                0
            );
            let dword = 42u32.to_le_bytes();
            assert_eq!(
                reg_set_value_ex_a(key, b"Answer\0".as_ptr(), 0, REG_DWORD, dword.as_ptr(), 4,),
                0
            );
            let mut name = [0u16; 64];
            let mut name_len = name.len() as u32;
            assert_eq!(
                reg_enum_value_w(
                    key,
                    0,
                    name.as_mut_ptr(),
                    &mut name_len,
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                ),
                0
            );
            assert_eq!(
                String::from_utf16_lossy(&name[..name_len as usize]),
                "answer"
            );
            let mut value = vec![0u8; 4];
            let mut value_len = 4u32;
            assert_eq!(
                reg_query_value_ex_a(
                    key,
                    b"Answer\0".as_ptr(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                    value.as_mut_ptr(),
                    &mut value_len,
                ),
                0
            );
            assert_eq!(u32::from_le_bytes(value.try_into().unwrap()), 42);
            assert_eq!(reg_close_key(key), 0);
            assert_eq!(reg_delete_key_w(HKEY_CURRENT_USER, subkey_w.as_ptr()), 0);
        }
    }
}

#[cfg(test)]
mod extra_tests {
    use super::*;

    #[test]
    fn dword_info_and_delete() {
        let subkey = format!("Software\\n2np-test-{}-dword", std::process::id());
        let subkey_w = utf::wide_with_nul(&subkey);
        unsafe {
            let mut key = 0usize;
            let mut disposition = 0u32;
            assert_eq!(
                reg_create_key_ex_w(
                    HKEY_CURRENT_USER,
                    subkey_w.as_ptr(),
                    0,
                    ptr::null(),
                    0,
                    0,
                    ptr::null_mut(),
                    &mut key,
                    &mut disposition,
                ),
                0
            );
            let dword = 1337u32.to_le_bytes();
            assert_eq!(
                reg_set_value_ex_w(
                    key,
                    utf::wide_with_nul("Number").as_ptr(),
                    0,
                    REG_DWORD,
                    dword.as_ptr(),
                    4,
                ),
                0
            );
            let mut values = 0u32;
            let mut subkeys = 0u32;
            let mut max_value = 0u32;
            assert_eq!(
                reg_query_info_key_w(
                    key,
                    &mut subkeys,
                    ptr::null_mut(),
                    ptr::null_mut(),
                    &mut values,
                    ptr::null_mut(),
                    &mut max_value,
                    ptr::null_mut(),
                    ptr::null_mut(),
                ),
                0
            );
            assert_eq!((subkeys, values, max_value), (0, 1, 4));

            let mut kind = 0u32;
            let mut size = 4u32;
            let mut value = [0u8; 4];
            assert_eq!(
                reg_get_value_w(
                    HKEY_CURRENT_USER,
                    subkey_w.as_ptr(),
                    utf::wide_with_nul("Number").as_ptr(),
                    0,
                    &mut kind,
                    value.as_mut_ptr(),
                    &mut size,
                ),
                0
            );
            assert_eq!(kind, REG_DWORD);
            assert_eq!(u32::from_le_bytes(value), 1337);

            assert_eq!(
                reg_delete_value_w(key, utf::wide_with_nul("Number").as_ptr()),
                0
            );
            let mut missing = 0u32;
            assert_eq!(
                reg_query_value_ex_w(
                    key,
                    utf::wide_with_nul("Number").as_ptr(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                    &mut missing,
                ),
                E_FILE_NOT_FOUND
            );
            assert_eq!(reg_close_key(key), 0);
            assert_eq!(reg_delete_key_w(HKEY_CURRENT_USER, subkey_w.as_ptr()), 0);
            let mut reopened = 0usize;
            assert_eq!(
                reg_open_key_ex_w(HKEY_CURRENT_USER, subkey_w.as_ptr(), 0, 0, &mut reopened,),
                E_FILE_NOT_FOUND
            );
        }
    }
}
