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
pub struct PeImage {
    pub path: String,
    pub data: Vec<u8>,
    pub machine: u16,
    pub image_base: u64,
    pub entry_point: u32,
    pub size_of_image: u32,
    pub size_of_headers: u32,
    pub subsystem: u16,
    pub sections: Vec<Section>,
    pub imports: Vec<ImportDescriptor>,
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
        if read_u32(&data, e_lfanew) != Some(0x00004550) {
            return Err(PeError::InvalidNt);
        }

        let machine = read_u16(&data, e_lfanew + 4).ok_or(PeError::InvalidNt)?;
        if machine != 0x8664 {
            return Err(PeError::NotPe64);
        }
        let num_sections = read_u16(&data, e_lfanew + 6).ok_or(PeError::InvalidNt)? as usize;
        let size_opt = read_u16(&data, e_lfanew + 20).ok_or(PeError::InvalidNt)? as usize;
        let opt_off = e_lfanew + 24;
        if read_u16(&data, opt_off) != Some(0x20B) {
            return Err(PeError::NotPe64);
        }

        let entry_point = read_u32(&data, opt_off + 16).ok_or(PeError::InvalidNt)?;
        let image_base = read_u64(&data, opt_off + 24).ok_or(PeError::InvalidNt)?;
        let size_of_image = read_u32(&data, opt_off + 56).ok_or(PeError::InvalidNt)?;
        let size_of_headers = read_u32(&data, opt_off + 60).ok_or(PeError::InvalidNt)?;
        let subsystem = read_u16(&data, opt_off + 68).ok_or(PeError::InvalidNt)?;

        if size_of_image == 0 || entry_point >= size_of_image {
            return Err(PeError::InvalidNt);
        }

        let data_dir_off = opt_off + 112;
        let import_rva = read_u32(&data, data_dir_off + 8).ok_or(PeError::MissingDataDirectory)?;
        let import_size = read_u32(&data, data_dir_off + 12).ok_or(PeError::MissingDataDirectory)?;
        let reloc_rva = read_u32(&data, data_dir_off + 40).unwrap_or(0);
        let reloc_size = read_u32(&data, data_dir_off + 44).unwrap_or(0);

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
            image_base,
            entry_point,
            size_of_image,
            size_of_headers,
            subsystem,
            sections,
            imports: Vec::new(),
            reloc_rva,
            reloc_size,
        };

        if import_rva != 0 && import_size != 0 {
            image.imports = image.parse_imports(import_rva)?;
        }

        Ok(image)
    }

    pub fn rva_to_offset(&self, rva: u32) -> Option<usize> {
        for s in &self.sections {
            let size = s.mapped_size();
            if size == 0 {
                continue;
            }
            if rva >= s.virtual_address && rva < s.virtual_address.saturating_add(size) {
                let off = s.raw_ptr as usize + (rva - s.virtual_address) as usize;
                if off < self.data.len() {
                    return Some(off);
                }
            }
        }
        None
    }

    fn parse_imports(&self, import_rva: u32) -> Result<Vec<ImportDescriptor>, PeError> {
        let import_off = self
            .rva_to_offset(import_rva)
            .ok_or(PeError::MissingDataDirectory)?;
        let mut out = Vec::new();
        for i in 0..256 {
            let entry = import_off + i * 20;
            if entry + 20 > self.data.len() {
                break;
            }
            let original_first_thunk =
                read_u32(&self.data, entry).ok_or(PeError::InvalidNt)?;
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
            let mut entries = Vec::new();
            if thunk_rva != 0 {
                for t in 0..4096u32 {
                    let some = thunk_rva.checked_add(t * 8);
                    let off = match some.and_then(|rva| self.rva_to_offset(rva)) {
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
                    let iat_rva = iat_base.saturating_add(t * 8);
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
                        let name = read_cstr(&self.data, hint_off + 2);
                        entries.push(ImportEntry {
                            name,
                            ordinal: None,
                            iat_rva,
                        });
                    }
                }
            }
            out.push(ImportDescriptor { dll, entries });
        }
        Ok(out)
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

    Ok(profile)
}
