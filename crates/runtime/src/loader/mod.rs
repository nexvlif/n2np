use crate::modules;
use crate::winapi;
use n2np_pe::{PeImage, Section};
use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::ptr;
use std::sync::{Mutex, OnceLock};

#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    #[error("pe error: {0}")]
    Pe(#[from] n2np_pe::PeError),
    #[error("image is not x86_64")]
    UnsupportedArch,
    #[error("mmap failed: {0}")]
    Mmap(String),
    #[error("mprotect failed: {0}")]
    Mprotect(String),
    #[error("image requires relocations but mapping was not possible at its preferred base")]
    NeedsRelocations,
    #[error("unsupported relocation type {0}")]
    UnsupportedRelocation(u16),
    #[error("invalid section {0}")]
    InvalidSection(String),
    #[error("module not found: {0}")]
    ModuleNotFound(String),
    #[error("image has no entry point")]
    NoEntryPoint,
}

const PAGE_SIZE: usize = 4096;
const IMAGE_SCN_MEM_EXECUTE: u32 = 0x2000_0000;
const IMAGE_SCN_MEM_READ: u32 = 0x4000_0000;
const IMAGE_SCN_MEM_WRITE: u32 = 0x8000_0000;
const DLL_PROCESS_ATTACH: u32 = 1;
const DLL_PROCESS_DETACH: u32 = 0;
const DLL_PROCESS_VERIFIER: u32 = 4;

#[derive(Debug)]
struct LoadedModule {
    name: String,
    path: PathBuf,
    image: PeImage,
    base: usize,
    size: usize,
    handle: usize,
    is_main: bool,
    initialized: bool,
    tls_callbacks: Vec<usize>,
    tls_index_address: Option<usize>,
}

#[derive(Debug)]
struct Loader {
    search_paths: Vec<PathBuf>,
    modules: Vec<LoadedModule>,
}

static ACTIVE: OnceLock<Mutex<Loader>> = OnceLock::new();

fn with_active<R>(f: impl FnOnce(&mut Loader) -> R) -> Option<R> {
    ACTIVE.get().map(|loader| f(&mut loader.lock().unwrap()))
}

fn align_up(value: usize, align: usize) -> usize {
    (value + align - 1) & !(align - 1)
}

fn map_image(image: &PeImage) -> Result<(*mut u8, usize, bool), LoadError> {
    let size = align_up(image.size_of_image as usize, PAGE_SIZE);
    let preferred = image.image_base as *mut c_void;
    let prot = libc::PROT_READ | libc::PROT_WRITE;

    unsafe {
        let mapped = libc::mmap(
            preferred,
            size,
            prot,
            libc::MAP_PRIVATE | libc::MAP_ANONYMOUS | libc::MAP_FIXED_NOREPLACE,
            -1,
            0,
        );
        if mapped != libc::MAP_FAILED {
            return Ok((mapped as *mut u8, size, true));
        }

        let fallback = libc::mmap(
            ptr::null_mut(),
            size,
            prot,
            libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
            -1,
            0,
        );
        if fallback == libc::MAP_FAILED {
            let err = std::io::Error::last_os_error();
            return Err(LoadError::Mmap(err.to_string()));
        }
        Ok((fallback as *mut u8, size, false))
    }
}

fn copy_image(image: &PeImage, base: *mut u8) -> Result<(), LoadError> {
    let header_size = image
        .size_of_headers
        .min(image.data.len() as u32)
        .min(image.size_of_image) as usize;
    unsafe {
        ptr::copy_nonoverlapping(image.data.as_ptr(), base, header_size);
    }

    for section in &image.sections {
        let dst = unsafe { base.add(section.virtual_address as usize) };
        let virt_len = section.mapped_size() as usize;
        if section.virtual_address as usize + virt_len > image.size_of_image as usize {
            return Err(LoadError::InvalidSection(section.name_string()));
        }
        unsafe {
            ptr::write_bytes(dst, 0, virt_len);
        }
        if section.raw_size == 0 || section.raw_ptr == 0 {
            continue;
        }
        let raw_start = section.raw_ptr as usize;
        let raw_end = raw_start
            .checked_add(section.raw_size as usize)
            .ok_or_else(|| LoadError::InvalidSection(section.name_string()))?;
        if raw_end > image.data.len() {
            return Err(LoadError::InvalidSection(section.name_string()));
        }
        let copy_len = (section.raw_size as usize).min(virt_len);
        unsafe {
            ptr::copy_nonoverlapping(image.data.as_ptr().add(raw_start), dst, copy_len);
        }
    }
    Ok(())
}

fn apply_relocations(image: &PeImage, base: *mut u8) -> Result<(), LoadError> {
    let delta = (base as u64).wrapping_sub(image.image_base);
    if delta == 0 {
        return Ok(());
    }
    if image.reloc_rva == 0 || image.reloc_size == 0 {
        return Err(LoadError::NeedsRelocations);
    }
    let start = image
        .rva_to_offset(image.reloc_rva)
        .ok_or(LoadError::NeedsRelocations)?;
    let end = start
        .checked_add(image.reloc_size as usize)
        .unwrap_or(image.data.len())
        .min(image.data.len());
    let mut off = start;
    while off + 8 <= end {
        let page_rva = read_u32(&image.data, off).unwrap_or(0);
        let block_size = read_u32(&image.data, off + 4).unwrap_or(0) as usize;
        if block_size == 0 || block_size < 8 || off + block_size > end {
            break;
        }
        let count = (block_size - 8) / 2;
        for i in 0..count {
            let entry = read_u16(&image.data, off + 8 + i * 2).unwrap_or(0);
            let reloc_type = entry >> 12;
            let reloc_off = (entry & 0x0FFF) as u32;
            let target_rva = page_rva.wrapping_add(reloc_off);
            if target_rva as usize >= image.size_of_image as usize {
                continue;
            }
            let target = unsafe { base.add(target_rva as usize) };
            match reloc_type {
                0 => {}
                3 => unsafe {
                    let value = ptr::read_unaligned(target as *const u32);
                    ptr::write_unaligned(target as *mut u32, value.wrapping_add(delta as u32));
                },
                10 => unsafe {
                    let value = ptr::read_unaligned(target as *const u64);
                    ptr::write_unaligned(target as *mut u64, value.wrapping_add(delta));
                },
                other => return Err(LoadError::UnsupportedRelocation(other)),
            }
        }
        off += block_size;
    }
    Ok(())
}

fn protect_sections(image: &PeImage, base: *mut u8) -> Result<(), LoadError> {
    for section in &image.sections {
        let len = section.mapped_size() as usize;
        if len == 0 {
            continue;
        }
        let start = (base as usize + section.virtual_address as usize) & !(PAGE_SIZE - 1);
        let end = align_up(
            base as usize + section.virtual_address as usize + len,
            PAGE_SIZE,
        );
        let prot = section_protection(section);
        let rc = unsafe { libc::mprotect(start as *mut c_void, end.saturating_sub(start), prot) };
        if rc != 0 {
            let err = std::io::Error::last_os_error();
            return Err(LoadError::Mprotect(err.to_string()));
        }
    }
    Ok(())
}

fn section_protection(section: &Section) -> i32 {
    let c = section.characteristics;
    let mut prot = 0;
    if c & IMAGE_SCN_MEM_READ != 0 {
        prot |= libc::PROT_READ;
    }
    if c & IMAGE_SCN_MEM_WRITE != 0 {
        prot |= libc::PROT_WRITE;
    }
    if c & IMAGE_SCN_MEM_EXECUTE != 0 {
        prot |= libc::PROT_EXEC;
    }
    if prot == 0 {
        libc::PROT_READ
    } else {
        prot
    }
}

fn read_u16(data: &[u8], off: usize) -> Option<u16> {
    if off + 2 > data.len() {
        return None;
    }
    Some(u16::from_le_bytes([data[off], data[off + 1]]))
}

fn read_u32(data: &[u8], off: usize) -> Option<u32> {
    if off + 4 > data.len() {
        return None;
    }
    Some(u32::from_le_bytes([
        data[off],
        data[off + 1],
        data[off + 2],
        data[off + 3],
    ]))
}

fn split_search_path(value: &str) -> Vec<PathBuf> {
    let separator = if value.contains(';') { ';' } else { ':' };
    value
        .split(separator)
        .filter(|part| !part.trim().is_empty())
        .map(PathBuf::from)
        .collect()
}

fn is_builtin_module(dll: &str) -> bool {
    modules::is_builtin(dll) || modules::is_builtin(&winapi::normalize_dll_name(dll))
}

impl Loader {
    fn new(exe: &Path, extra_paths: &[String]) -> Self {
        let mut search_paths = Vec::new();
        if let Some(parent) = exe.parent() {
            search_paths.push(parent.to_path_buf());
        }
        search_paths.push(std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
        if let Some(value) = std::env::var_os("N2NP_DLL_PATH") {
            search_paths.extend(split_search_path(&value.to_string_lossy()));
        }
        for extra in extra_paths {
            search_paths.extend(split_search_path(extra));
        }
        search_paths.push(PathBuf::from("/usr/lib/n2np"));
        search_paths.push(PathBuf::from("/usr/local/lib/n2np"));
        search_paths.push(PathBuf::from("/usr/lib/wine/x86_64-windows"));
        let mut unique = Vec::new();
        for path in search_paths {
            if !unique.contains(&path) {
                unique.push(path);
            }
        }

        Self {
            search_paths: unique,
            modules: Vec::new(),
        }
    }

    fn find_loaded(&self, name: &str) -> Option<&LoadedModule> {
        let name = modules::normalize(name);
        self.modules.iter().find(|module| module.name == name)
    }

    fn find_loaded_mut(&mut self, name: &str) -> Option<&mut LoadedModule> {
        let name = modules::normalize(name);
        self.modules.iter_mut().find(|module| module.name == name)
    }

    fn find_dll(&self, name: &str) -> Option<PathBuf> {
        let name = modules::normalize(name);
        let direct = PathBuf::from(&name);
        if direct.is_file() {
            return Some(direct);
        }
        for directory in &self.search_paths {
            let candidate = directory.join(&name);
            if candidate.is_file() {
                return Some(candidate);
            }
            if let Some(found) = find_case_insensitive(directory, &name) {
                return Some(found);
            }
        }
        None
    }

    fn load_path(&mut self, path: &Path, is_main: bool) -> Result<usize, LoadError> {
        let name = modules::normalize(
            path.file_name()
                .and_then(|value| value.to_str())
                .unwrap_or("unknown.dll"),
        );
        if let Some(module) = self.find_loaded(&name) {
            return Ok(module.handle);
        }

        let image = PeImage::from_path(&path.to_string_lossy())?;
        if image.machine != 0x8664 {
            return Err(LoadError::UnsupportedArch);
        }
        log::info!(
            "[n2np-loader] loading {} at preferred 0x{:x} entry=0x{:x} size=0x{:x}",
            name,
            image.image_base,
            image.entry_point,
            image.size_of_image
        );

        let (base, size, at_preferred) = map_image(&image)?;
        if !at_preferred
            && (image.reloc_rva == 0 || image.reloc_size == 0)
            && !image.sections.is_empty()
        {
            log::warn!(
                "[n2np-loader] {} could not be mapped at its preferred base and has no relocations",
                name
            );
        }
        copy_image(&image, base)?;
        apply_relocations(&image, base)?;

        let handle = base as usize;
        let tls_callbacks = tls_callbacks(&image, handle);
        let tls_index_address = tls_index_address(&image, handle);
        let module_index = self.modules.len();
        self.modules.push(LoadedModule {
            name: name.clone(),
            path: path.to_path_buf(),
            image,
            base: handle,
            size,
            handle,
            is_main,
            initialized: false,
            tls_callbacks,
            tls_index_address,
        });

        modules::register(&name, handle);
        modules::set_path(handle, &path.to_string_lossy());
        if is_main {
            modules::set_main(handle);
            crate::winapi::kernel32::register_main_module(&path.to_string_lossy(), handle);
        }

        let imports = self.modules[module_index].image.imports.clone();
        let delay_imports = self.modules[module_index].image.delay_imports.clone();

        let mut dependencies = Vec::new();
        for descriptor in &imports {
            if is_builtin_module(&descriptor.dll) || self.find_loaded(&descriptor.dll).is_some() {
                continue;
            }
            match self.find_dll(&descriptor.dll) {
                Some(dependency) => dependencies.push(dependency),
                None => log::warn!(
                    "[n2np-loader] dependency {} of {} was not found",
                    descriptor.dll,
                    name
                ),
            }
        }
        for descriptor in &delay_imports {
            if is_builtin_module(&descriptor.dll) || self.find_loaded(&descriptor.dll).is_some() {
                continue;
            }
            match self.find_dll(&descriptor.dll) {
                Some(dependency) => dependencies.push(dependency),
                None => log::warn!(
                    "[n2np-loader] dependency {} of {} was not found",
                    descriptor.dll,
                    name
                ),
            }
        }
        for dependency in dependencies {
            self.load_path(&dependency, false)?;
        }

        self.resolve_imports(module_index, &imports, &delay_imports);
        protect_sections(&self.modules[module_index].image, base)?;
        Ok(handle)
    }

    fn resolve_imports(
        &self,
        module_index: usize,
        imports: &[n2np_pe::ImportDescriptor],
        delay_imports: &[n2np_pe::DelayImportDescriptor],
    ) {
        let module = &self.modules[module_index];
        for descriptor in imports {
            for entry in &descriptor.entries {
                let resolved =
                    self.resolve_symbol(&descriptor.dll, entry.name.as_deref(), entry.ordinal, 0);
                let address = match resolved {
                    Some(address) => address,
                    None => {
                        log::warn!(
                            "[n2np-loader] unresolved thunk {}!{} in {}",
                            descriptor.dll,
                            entry
                                .name
                                .clone()
                                .unwrap_or_else(|| format!("#{}", entry.ordinal.unwrap_or(0))),
                            module.name
                        );
                        winapi::missing_thunk as *const () as usize
                    }
                };
                let slot =
                    unsafe { (module.base as *mut u8).add(entry.iat_rva as usize) as *mut u64 };
                unsafe {
                    ptr::write_unaligned(slot, address as u64);
                }
            }
        }
        for descriptor in delay_imports {
            if descriptor.module_handle_rva != 0
                && descriptor.module_handle_rva < module.image.size_of_image
            {
                let handle = modules::handle_for(&descriptor.dll).unwrap_or(0);
                let slot = unsafe {
                    (module.base as *mut u8).add(descriptor.module_handle_rva as usize)
                        as *mut usize
                };
                unsafe {
                    ptr::write_unaligned(slot, handle);
                }
            }
            for entry in &descriptor.entries {
                let resolved =
                    self.resolve_symbol(&descriptor.dll, entry.name.as_deref(), entry.ordinal, 0);
                let address = match resolved {
                    Some(address) => address,
                    None => {
                        log::warn!(
                            "[n2np-loader] unresolved delay thunk {}!{} in {}",
                            descriptor.dll,
                            entry
                                .name
                                .clone()
                                .unwrap_or_else(|| format!("#{}", entry.ordinal.unwrap_or(0))),
                            module.name
                        );
                        winapi::missing_thunk as *const () as usize
                    }
                };
                let slot =
                    unsafe { (module.base as *mut u8).add(entry.iat_rva as usize) as *mut u64 };
                unsafe {
                    ptr::write_unaligned(slot, address as u64);
                }
            }
        }
    }

    fn resolve_symbol(
        &self,
        dll: &str,
        name: Option<&str>,
        ordinal: Option<u16>,
        depth: usize,
    ) -> Option<usize> {
        if depth > 8 {
            return None;
        }
        if is_builtin_module(dll) {
            return match (name, ordinal) {
                (Some(name), _) => winapi::resolve(dll, name),
                (None, Some(ordinal)) => winapi::resolve_ordinal(dll, ordinal),
                _ => None,
            };
        }
        let module = self.find_loaded(dll)?;
        let export = match (name, ordinal) {
            (Some(name), _) => module.image.export_by_name(name),
            (None, Some(ordinal)) => module.image.export_by_ordinal(u32::from(ordinal)),
            _ => None,
        }?;
        if let Some(forwarder) = &export.forwarder {
            if let Some((target_dll, target_name)) = forwarder.split_once('.') {
                if let Some(rest) = target_name.strip_prefix('#') {
                    let target_ordinal = rest.parse::<u16>().ok();
                    return self.resolve_symbol(target_dll, None, target_ordinal, depth + 1);
                }
                return self.resolve_symbol(target_dll, Some(target_name), None, depth + 1);
            }
            return None;
        }
        Some(module.base + export.rva as usize)
    }

    fn initialize_uninitialized(&mut self) -> Result<(), LoadError> {
        loop {
            let next = self
                .modules
                .iter()
                .rposition(|module| !module.is_main && !module.initialized);
            let Some(index) = next else {
                break;
            };
            self.initialize_module(index)?;
        }
        Ok(())
    }

    fn initialize_module(&mut self, index: usize) -> Result<(), LoadError> {
        let module = &mut self.modules[index];
        if module.initialized {
            return Ok(());
        }
        module.initialized = true;
        if let Some(address) = module.tls_index_address {
            let index = unsafe { crate::winapi::kernel32::tls_alloc() };
            unsafe {
                ptr::write_unaligned(address as *mut u32, index);
            }
        }
        let handle = module.handle as usize;
        let callbacks = module.tls_callbacks.clone();
        let entry = module.image.entry_point;
        let is_dll = module.image.is_dll();
        let name = module.name.clone();

        for callback in callbacks {
            log::debug!("[n2np-loader] TLS callback {callback:#x} for {name}");
            let callback: unsafe extern "win64" fn(usize, u32, usize) =
                unsafe { std::mem::transmute(callback) };
            unsafe {
                callback(handle, DLL_PROCESS_ATTACH, 0);
            }
        }

        if is_dll && entry != 0 {
            let entry: unsafe extern "win64" fn(usize, u32, usize) -> i32 =
                unsafe { std::mem::transmute(module.base + entry as usize) };
            let result = unsafe { entry(handle, DLL_PROCESS_ATTACH, 0) };
            if result == 0 {
                log::warn!("[n2np-loader] DllMain(DLL_PROCESS_ATTACH) failed for {name}");
            }
        }
        Ok(())
    }

    fn initialize_main_tls(&mut self) {
        let Some(index) = self.modules.iter().position(|module| module.is_main) else {
            return;
        };
        let module = &self.modules[index];
        if let Some(address) = module.tls_index_address {
            let tls_index = unsafe { crate::winapi::kernel32::tls_alloc() };
            unsafe {
                ptr::write_unaligned(address as *mut u32, tls_index);
            }
        }
        let callbacks = module.tls_callbacks.clone();
        let handle = module.handle as usize;
        for callback in callbacks {
            let callback: unsafe extern "win64" fn(usize, u32, usize) =
                unsafe { std::mem::transmute(callback) };
            unsafe {
                callback(handle, DLL_PROCESS_ATTACH, 0);
            }
        }
        let _ = DLL_PROCESS_VERIFIER;
    }

    fn main_entry(&self) -> Option<usize> {
        let main = self.modules.iter().find(|module| module.is_main)?;
        if main.image.entry_point == 0 {
            return None;
        }
        Some(main.base + main.image.entry_point as usize)
    }

    fn detach_all(&mut self) {
        for module in self.modules.iter_mut().rev() {
            if module.is_main {
                continue;
            }
            if !module.initialized || !module.image.is_dll() || module.image.entry_point == 0 {
                continue;
            }
            let entry: unsafe extern "win64" fn(usize, u32, usize) -> i32 =
                unsafe { std::mem::transmute(module.base + module.image.entry_point as usize) };
            unsafe {
                entry(module.handle, DLL_PROCESS_DETACH, 0);
            }
        }
    }

    fn handles_snapshot(&self) -> Vec<(usize, usize, bool)> {
        self.modules
            .iter()
            .map(|module| (module.handle, module.base, module.is_main))
            .collect()
    }

    fn unmap_all(&mut self) {
        for module in self.modules.drain(..) {
            unsafe {
                libc::munmap(module.base as *mut c_void, module.size);
            }
        }
    }
}

fn find_case_insensitive(directory: &Path, name: &str) -> Option<PathBuf> {
    let entries = std::fs::read_dir(directory).ok()?;
    for entry in entries.flatten() {
        if entry
            .file_name()
            .to_string_lossy()
            .eq_ignore_ascii_case(name)
        {
            return Some(entry.path());
        }
    }
    None
}

fn va_to_address(image: &PeImage, base: usize, value: usize) -> Option<usize> {
    if value == 0 {
        return None;
    }
    if value >= base && value < base + image.size_of_image as usize {
        return Some(value);
    }
    if value >= image.image_base as usize
        && value < image.image_base as usize + image.size_of_image as usize
    {
        return Some(base + (value - image.image_base as usize));
    }
    Some(value)
}

fn tls_callbacks(image: &PeImage, base: usize) -> Vec<usize> {
    let Some(tls) = &image.tls else {
        return Vec::new();
    };
    let address = match va_to_address(image, base, tls.address_of_callbacks as usize) {
        Some(value) => value,
        None => return Vec::new(),
    };
    let mut callbacks = Vec::new();
    for index in 0..1024 {
        let value = unsafe { ptr::read_unaligned((address + index * 8) as *const usize) };
        if value == 0 {
            break;
        }
        if let Some(callback) = va_to_address(image, base, value) {
            callbacks.push(callback);
        }
    }
    callbacks
}

fn tls_index_address(image: &PeImage, base: usize) -> Option<usize> {
    let tls = image.tls.as_ref()?;
    va_to_address(image, base, tls.address_of_index as usize)
}

fn load_library_hook(name: &str) -> Option<usize> {
    with_active(|loader| {
        if let Some(module) = loader.find_loaded(name) {
            return Some(module.handle);
        }
        let path = loader.find_dll(name)?;
        let handle = loader.load_path(&path, false).ok()?;
        loader.initialize_uninitialized().ok()?;
        Some(handle)
    })
    .flatten()
}

fn resolve_export_hook(handle: usize, name: &str) -> Option<usize> {
    with_active(|loader| {
        let position = loader
            .modules
            .iter()
            .position(|module| module.handle == handle)?;
        let module = &loader.modules[position];
        if let Some(rest) = name.strip_prefix('#') {
            let ordinal = rest.parse::<u16>().ok()?;
            return loader.resolve_symbol(&module.name.clone(), None, Some(ordinal), 0);
        }
        let dll = module.name.clone();
        loader.resolve_symbol(&dll, Some(name), None, 0)
    })
    .flatten()
}

pub fn run(path: &str, dll_paths: &[String]) -> Result<u32, LoadError> {
    let path = PathBuf::from(path);
    let mut loader = Loader::new(&path, dll_paths);
    loader.load_path(&path, true)?;
    loader.initialize_uninitialized()?;
    loader.initialize_main_tls();

    let entry = loader.main_entry().ok_or(LoadError::NoEntryPoint)?;
    let _handles = loader.handles_snapshot();

    if ACTIVE.set(Mutex::new(loader)).is_err() {
        return Err(LoadError::Mmap("loader already active".to_string()));
    }
    modules::set_loader(load_library_hook);
    modules::set_export_resolver(resolve_export_hook);

    log::info!("[n2np-loader] jumping to entry at {entry:#x}");
    let entry_fn: unsafe extern "win64" fn() -> u32 = unsafe { std::mem::transmute(entry) };
    let code = unsafe { entry_fn() };

    with_active(|loader| loader.detach_all());
    Ok(code)
}
