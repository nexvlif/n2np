use n2np_common::{Import, Module, Profile};
use std::fs;

#[derive(Debug, thiserror::Error)]
pub enum PeError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid dos header")]
    InvalidDos,
    #[error("invalid nt header")]
    InvalidNt,
    #[error("not pe64")]
    NotPe64,
    #[error("truncated")]
    Truncated,
    #[error("missing data directory")]
    MissingDataDirectory,
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

fn read_u64(data: &[u8], off: usize) -> Option<u64> {
    if off + 8 > data.len() {
        return None;
    }
    Some(u64::from_le_bytes([
        data[off],
        data[off + 1],
        data[off + 2],
        data[off + 3],
        data[off + 4],
        data[off + 5],
        data[off + 6],
        data[off + 7],
    ]))
}

fn read_cstr(data: &[u8], mut off: usize) -> Option<String> {
    while off < data.len() && data[off] == 0 {
        off += 1;
    }
    let mut out = Vec::new();
    while off < data.len() {
        let b = data[off];
        if b == 0 {
            break;
        }
        out.push(b);
        off += 1;
        if out.len() > 512 {
            break;
        }
    }
    if out.is_empty() {
        return None;
    }
    Some(String::from_utf8_lossy(&out).into_owned())
}

fn normalize_dll_name(name: &str) -> String {
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

#[derive(Debug, Clone)]
pub struct Section {
    pub name: [u8; 8],
    pub virtual_address: u32,
    pub virtual_size: u32,
    pub raw_ptr: u32,
    pub raw_size: u32,
    pub characteristics: u32,
}

impl Section {
    pub fn mapped_size(&self) -> u32 {
        self.virtual_size.max(self.raw_size)
    }

    pub fn name_string(&self) -> String {
        let end = self.name.iter().position(|b| *b == 0).unwrap_or(8);
        String::from_utf8_lossy(&self.name[..end]).into_owned()
    }

    pub fn is_executable(&self) -> bool {
        self.characteristics & 0x2000_0000 != 0
    }

    pub fn is_writable(&self) -> bool {
        self.characteristics & 0x8000_0000 != 0
    }

    pub fn contains_rva(&self, rva: u32) -> bool {
        let size = self.mapped_size();
        size != 0 && rva >= self.virtual_address && rva < self.virtual_address.saturating_add(size)
    }
}

#[derive(Debug, Clone)]
pub struct ImportEntry {
    pub name: Option<String>,
    pub ordinal: Option<u16>,
    pub iat_rva: u32,
}

#[derive(Debug, Clone)]
pub struct ImportDescriptor {
    pub dll: String,
    pub entries: Vec<ImportEntry>,
}

#[derive(Debug, Clone)]
pub struct DelayImportDescriptor {
    pub dll: String,
    pub module_handle_rva: u32,
    pub entries: Vec<ImportEntry>,
}

#[derive(Debug, Clone)]
pub struct ExportEntry {
    pub name: Option<String>,
    pub ordinal: u32,
    pub rva: u32,
    pub forwarder: Option<String>,
}

#[derive(Debug, Clone)]
pub struct TlsInfo {
    pub start_address: u64,
    pub end_address: u64,
    pub address_of_index: u64,
    pub address_of_callbacks: u64,
    pub size_of_zero_fill: u32,
    pub characteristics: u32,
}

#[derive(Debug, Clone)]
pub struct PeImage {
    pub path: String,
    pub data: Vec<u8>,
    pub machine: u16,
    pub characteristics: u16,
    pub image_base: u64,
    pub entry_point: u32,
    pub size_of_image: u32,
    pub size_of_headers: u32,
    pub subsystem: u16,
    pub sections: Vec<Section>,
    pub imports: Vec<ImportDescriptor>,
    pub delay_imports: Vec<DelayImportDescriptor>,
    pub exports: Vec<ExportEntry>,
    pub tls: Option<TlsInfo>,
    pub export_dir_rva: u32,
    pub export_dir_size: u32,
    pub reloc_rva: u32,
    pub reloc_size: u32,
}

impl PeImage {
    pub fn from_path(path: &str) -> Result<Self, PeError> {
        let data = fs::read(path)?;
        Self::from_bytes(path, data)
    }

    pub fn from_bytes(path: &str, data: Vec<u8>) -> Result<Self, PeError> {
        if data.len() < 0x40 || read_u16(&data, 0) != Some(0x5A4D) {
            return Err(PeError::InvalidDos);
        }
        let e_lfanew = read_u32(&data, 0x3C).ok_or(PeError::InvalidDos)? as usize;
        let nt_end = e_lfanew
            .checked_add(4 + 20 + 240)
            .ok_or(PeError::Truncated)?;
        if nt_end > data.len() {
            return Err(PeError::Truncated);
        }
        if read_u32(&data, e_lfanew) != Some(0x0000_4550) {
            return Err(PeError::InvalidNt);
        }

        let machine = read_u16(&data, e_lfanew + 4).ok_or(PeError::InvalidNt)?;
        if machine != 0x8664 {
            return Err(PeError::NotPe64);
        }
        let num_sections = read_u16(&data, e_lfanew + 6).ok_or(PeError::InvalidNt)? as usize;
        let size_opt = read_u16(&data, e_lfanew + 20).ok_or(PeError::InvalidNt)? as usize;
        let characteristics = read_u16(&data, e_lfanew + 22).ok_or(PeError::InvalidNt)?;
        let opt_off = e_lfanew + 24;
        if read_u16(&data, opt_off) != Some(0x20B) {
            return Err(PeError::NotPe64);
        }

        let entry_point = read_u32(&data, opt_off + 16).ok_or(PeError::InvalidNt)?;
        let image_base = read_u64(&data, opt_off + 24).ok_or(PeError::InvalidNt)?;
        let size_of_image = read_u32(&data, opt_off + 56).ok_or(PeError::InvalidNt)?;
        let size_of_headers = read_u32(&data, opt_off + 60).ok_or(PeError::InvalidNt)?;
        let subsystem = read_u16(&data, opt_off + 68).ok_or(PeError::InvalidNt)?;

        if size_of_image == 0 || (entry_point != 0 && entry_point >= size_of_image) {
            return Err(PeError::InvalidNt);
        }

        let data_dir_off = opt_off + 112;
        let dir = |index: usize| -> (u32, u32) {
            let off = data_dir_off + index * 8;
            (
                read_u32(&data, off).unwrap_or(0),
                read_u32(&data, off + 4).unwrap_or(0),
            )
        };

        let (export_rva, export_size) = dir(0);
        let (import_rva, import_size) = dir(1);
        let (reloc_rva, reloc_size) = dir(5);
        let (tls_rva, tls_size) = dir(9);
        let (delay_rva, delay_size) = dir(13);

        let sec_off = opt_off + size_opt;
        let mut sections = Vec::with_capacity(num_sections);
        for i in 0..num_sections {
            let s = sec_off + i * 40;
            if s + 40 > data.len() {
                return Err(PeError::Truncated);
            }
            let mut name = [0u8; 8];
            name.copy_from_slice(&data[s..s + 8]);
            sections.push(Section {
                name,
                virtual_size: read_u32(&data, s + 8).ok_or(PeError::InvalidNt)?,
                virtual_address: read_u32(&data, s + 12).ok_or(PeError::InvalidNt)?,
                raw_size: read_u32(&data, s + 16).ok_or(PeError::InvalidNt)?,
                raw_ptr: read_u32(&data, s + 20).ok_or(PeError::InvalidNt)?,
                characteristics: read_u32(&data, s + 36).ok_or(PeError::InvalidNt)?,
            });
        }

        let mut image = Self {
            path: path.to_string(),
            data,
            machine,
            characteristics,
            image_base,
            entry_point,
            size_of_image,
            size_of_headers,
            subsystem,
            sections,
            imports: Vec::new(),
            delay_imports: Vec::new(),
            exports: Vec::new(),
            tls: None,
            export_dir_rva: export_rva,
            export_dir_size: export_size,
            reloc_rva,
            reloc_size,
        };

        if import_rva != 0 && import_size != 0 {
            image.imports = image.parse_imports(import_rva, import_size)?;
        }
        if delay_rva != 0 && delay_size != 0 {
            image.delay_imports = image.parse_delay_imports(delay_rva, delay_size);
        }
        if export_rva != 0 && export_size != 0 {
            image.exports = image.parse_exports(export_rva, export_size);
        }
        if tls_rva != 0 && tls_size != 0 {
            image.tls = image.parse_tls(tls_rva);
        }

        Ok(image)
    }

    pub fn is_dll(&self) -> bool {
        self.characteristics & 0x2000 != 0
    }

    pub fn rva_to_offset(&self, rva: u32) -> Option<usize> {
        for s in &self.sections {
            if s.contains_rva(rva) {
                let off = s.raw_ptr as usize + (rva - s.virtual_address) as usize;
                if off < self.data.len() {
                    return Some(off);
                }
            }
        }
        if rva < self.size_of_headers && (rva as usize) < self.data.len() {
            return Some(rva as usize);
        }
        None
    }

    pub fn data_at_rva(&self, rva: u32, len: usize) -> Option<&[u8]> {
        let off = self.rva_to_offset(rva)?;
        let end = off.checked_add(len)?;
        self.data.get(off..end)
    }

    pub fn export_by_name(&self, name: &str) -> Option<&ExportEntry> {
        self.exports
            .iter()
            .find(|e| e.name.as_deref() == Some(name))
    }

    pub fn export_by_ordinal(&self, ordinal: u32) -> Option<&ExportEntry> {
        self.exports.iter().find(|e| e.ordinal == ordinal)
    }

    fn read_thunks(&self, int_rva: u32, iat_rva: u32) -> Vec<ImportEntry> {
        let mut entries = Vec::new();
        if int_rva == 0 {
            return entries;
        }
        for t in 0..4096u32 {
            let off = match int_rva
                .checked_add(t * 8)
                .and_then(|rva| self.rva_to_offset(rva))
            {
                Some(v) => v,
                None => break,
            };
            let value = match read_u64(&self.data, off) {
                Some(v) => v,
                None => break,
            };
            if value == 0 {
                break;
            }
            let iat_rva = iat_rva.saturating_add(t * 8);
            if value & 0x8000_0000_0000_0000 != 0 {
                entries.push(ImportEntry {
                    name: None,
                    ordinal: Some((value & 0xFFFF) as u16),
                    iat_rva,
                });
            } else {
                let hint_name_rva = value as u32;
                let hint_off = match self.rva_to_offset(hint_name_rva) {
                    Some(v) if v + 2 <= self.data.len() => v,
                    _ => break,
                };
                entries.push(ImportEntry {
                    name: read_cstr(&self.data, hint_off + 2),
                    ordinal: None,
                    iat_rva,
                });
            }
        }
        entries
    }

    fn parse_imports(
        &self,
        import_rva: u32,
        import_size: u32,
    ) -> Result<Vec<ImportDescriptor>, PeError> {
        let import_off = self
            .rva_to_offset(import_rva)
            .ok_or(PeError::MissingDataDirectory)?;
        let max_entries = (import_size as usize / 20).max(1).min(4096);
        let mut out = Vec::new();
        for i in 0..max_entries {
            let entry = import_off + i * 20;
            if entry + 20 > self.data.len() {
                break;
            }
            let original_first_thunk = read_u32(&self.data, entry).ok_or(PeError::InvalidNt)?;
            let name_rva = read_u32(&self.data, entry + 12).ok_or(PeError::InvalidNt)?;
            let first_thunk = read_u32(&self.data, entry + 16).ok_or(PeError::InvalidNt)?;
            if original_first_thunk == 0 && name_rva == 0 && first_thunk == 0 {
                break;
            }
            if name_rva == 0 {
                continue;
            }
            let name_off = match self.rva_to_offset(name_rva) {
                Some(v) => v,
                None => continue,
            };
            let dll = match read_cstr(&self.data, name_off) {
                Some(v) => normalize_dll_name(&v),
                None => continue,
            };
            if dll.is_empty() {
                continue;
            }

            let thunk_rva = if original_first_thunk != 0 {
                original_first_thunk
            } else {
                first_thunk
            };
            let iat_base = if first_thunk != 0 {
                first_thunk
            } else {
                thunk_rva
            };
            out.push(ImportDescriptor {
                dll,
                entries: self.read_thunks(thunk_rva, iat_base),
            });
        }
        Ok(out)
    }

    fn parse_delay_imports(&self, delay_rva: u32, delay_size: u32) -> Vec<DelayImportDescriptor> {
        let base = self.image_base;
        let start = match self.rva_to_offset(delay_rva) {
            Some(v) => v,
            None => return Vec::new(),
        };
        let max_entries = (delay_size as usize / 32).max(1).min(256);
        let mut out = Vec::new();
        for i in 0..max_entries {
            let off = start + i * 32;
            if off + 32 > self.data.len() {
                break;
            }
            let read = |o: usize| read_u32(&self.data, o).unwrap_or(0);
            let attributes = read(off);
            let name_value = read(off + 4);
            let module_handle_value = read(off + 8);
            let iat_value = read(off + 12);
            let int_value = read(off + 16);
            if name_value == 0 && iat_value == 0 && int_value == 0 {
                break;
            }
            let uses_va = attributes & 1 == 0;
            let to_rva = |value: u32| -> u32 {
                if uses_va && value as u64 >= base {
                    (value as u64 - base) as u32
                } else {
                    value
                }
            };
            let name_rva = to_rva(name_value);
            if name_rva == 0 {
                continue;
            }
            let dll = match self
                .rva_to_offset(name_rva)
                .and_then(|o| read_cstr(&self.data, o))
            {
                Some(v) => normalize_dll_name(&v),
                None => continue,
            };
            let iat_rva = to_rva(iat_value);
            let int_rva = to_rva(int_value);
            let table_rva = if int_rva != 0 { int_rva } else { iat_rva };
            out.push(DelayImportDescriptor {
                dll,
                module_handle_rva: to_rva(module_handle_value),
                entries: self.read_thunks(table_rva, iat_rva),
            });
        }
        out
    }

    fn parse_exports(&self, export_rva: u32, export_size: u32) -> Vec<ExportEntry> {
        let off = match self.rva_to_offset(export_rva) {
            Some(v) => v,
            None => return Vec::new(),
        };
        let read = |o: usize| read_u32(&self.data, o).unwrap_or(0);
        let base_ordinal = read(off + 16);
        let num_functions = read(off + 20) as usize;
        let num_names = read(off + 24) as usize;
        let functions_rva = read(off + 28);
        let names_rva = read(off + 32);
        let ordinals_rva = read(off + 36);

        let functions_off = match self.rva_to_offset(functions_rva) {
            Some(v) => v,
            None => return Vec::new(),
        };
        let names_off = self.rva_to_offset(names_rva);
        let ordinals_off = self.rva_to_offset(ordinals_rva);

        let limit = num_functions.min(65536);
        let mut named = vec![None::<String>; limit];
        if let (Some(names_off), Some(ordinals_off)) = (names_off, ordinals_off) {
            for i in 0..num_names.min(limit) {
                let name_rva = read(names_off + i * 4);
                let ordinal_index =
                    read_u16(&self.data, ordinals_off + i * 2).unwrap_or(u16::MAX) as usize;
                if ordinal_index >= limit {
                    continue;
                }
                if named[ordinal_index].is_some() {
                    continue;
                }
                let name = self
                    .rva_to_offset(name_rva)
                    .and_then(|o| read_cstr(&self.data, o));
                named[ordinal_index] = name;
            }
        }

        let mut out = Vec::new();
        for (index, name) in named.into_iter().enumerate() {
            let func_rva = read(functions_off + index * 4);
            if func_rva == 0 {
                continue;
            }
            let forwarder =
                if func_rva >= export_rva && func_rva < export_rva.saturating_add(export_size) {
                    self.rva_to_offset(func_rva)
                        .and_then(|o| read_cstr(&self.data, o))
                } else {
                    None
                };
            out.push(ExportEntry {
                name,
                ordinal: base_ordinal.saturating_add(index as u32),
                rva: func_rva,
                forwarder,
            });
        }
        out
    }

    fn parse_tls(&self, tls_rva: u32) -> Option<TlsInfo> {
        let off = self.rva_to_offset(tls_rva)?;
        Some(TlsInfo {
            start_address: read_u64(&self.data, off)?,
            end_address: read_u64(&self.data, off + 8)?,
            address_of_index: read_u64(&self.data, off + 16)?,
            address_of_callbacks: read_u64(&self.data, off + 24)?,
            size_of_zero_fill: read_u32(&self.data, off + 32)?,
            characteristics: read_u32(&self.data, off + 36)?,
        })
    }
}

pub fn parse_pe_imports(path: &str) -> Result<Profile, PeError> {
    let image = PeImage::from_path(path)?;
    let mut profile = Profile::new(
        std::path::Path::new(path)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown"),
    );
    profile.target.path = Some(path.to_string());
    profile.target.arch = "x86_64".to_string();

    for desc in &image.imports {
        if !profile.modules.iter().any(|m| m.name == desc.dll) {
            profile.modules.push(Module {
                name: desc.dll.clone(),
                required: true,
            });
        }
        for entry in &desc.entries {
            profile.imports.push(Import {
                dll: desc.dll.clone(),
                name: entry.name.clone().unwrap_or_default(),
                ordinal: entry.ordinal,
                called: true,
            });
        }
    }

    for desc in &image.delay_imports {
        if !profile.modules.iter().any(|m| m.name == desc.dll) {
            profile.modules.push(Module {
                name: desc.dll.clone(),
                required: true,
            });
        }
        for entry in &desc.entries {
            profile.imports.push(Import {
                dll: desc.dll.clone(),
                name: entry.name.clone().unwrap_or_default(),
                ordinal: entry.ordinal,
                called: false,
            });
        }
    }

    Ok(profile)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn example(relative: &str) -> String {
        format!("{}/../../examples/{}", env!("CARGO_MANIFEST_DIR"), relative)
    }

    #[test]
    fn parses_gui_image() {
        let image = PeImage::from_path(&example("gui/gui.exe")).unwrap();
        assert_eq!(image.machine, 0x8664);
        assert!(!image.is_dll());
        assert_eq!(image.subsystem, 2);
        let imports: Vec<_> = image
            .imports
            .iter()
            .flat_map(|d| {
                d.entries
                    .iter()
                    .map(move |e| (d.dll.clone(), e.name.clone()))
            })
            .collect();
        assert!(imports
            .iter()
            .any(|(dll, name)| dll == "user32.dll" && name.as_deref() == Some("MessageBoxA")));
        assert!(imports
            .iter()
            .any(|(dll, name)| dll == "kernel32.dll" && name.as_deref() == Some("ExitProcess")));
    }

    #[test]
    fn parses_console_image() {
        let image = PeImage::from_path(&example("hello/hello.exe")).unwrap();
        let names: Vec<_> = image
            .imports
            .iter()
            .flat_map(|d| d.entries.iter().filter_map(|e| e.name.clone()))
            .collect();
        assert!(names.contains(&"GetStdHandle".to_string()));
        assert!(names.contains(&"WriteFile".to_string()));
        assert!(names.contains(&"ExitProcess".to_string()));
    }

    #[test]
    fn rva_in_headers_maps_to_file_offset() {
        let image = PeImage::from_path(&example("gui/gui.exe")).unwrap();
        assert_eq!(image.rva_to_offset(0), Some(0));
        assert_eq!(image.data_at_rva(0, 2), Some(&b"MZ"[..]));
    }
}
