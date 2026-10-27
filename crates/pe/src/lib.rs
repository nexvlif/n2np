use n2np_common::{Import, Module, Profile};
use std::fs;

#[derive(Debug, thiserror::Error)]
pub enum PeError {
    #[error("io error")]
    Io(#[from] std::io::Error),
    #[error("invalid dos header")]
    InvalidDos,
    #[error("invalid nt header")]
    InvalidNt,
    #[error("not pe64")]
    NotPe64,
    #[error("no import dir")]
    NoImportDir,
    #[error("truncated")]
    Truncated,
}

fn read_u16(data: &[u8], off: usize) -> Option<u16> {
    if off + 2 > data.len() { return None; }
    Some(u16::from_le_bytes([data[off], data[off+1]]))
}

fn read_u32(data: &[u8], off: usize) -> Option<u32> {
    if off + 4 > data.len() { return None; }
    Some(u32::from_le_bytes([data[off], data[off+1], data[off+2], data[off+3]]))
}

fn read_u64(data: &[u8], off: usize) -> Option<u64> {
    if off + 8 > data.len() { return None; }
    Some(u64::from_le_bytes([data[off], data[off+1], data[off+2], data[off+3], data[off+4], data[off+5], data[off+6], data[off+7]]))
}

fn rva_to_offset(sections: &[(u32,u32,u32,u32)], rva: u32) -> Option<usize> {
    for s in sections {
        let virt_addr = s.0;
        let virt_size = s.1;
        let raw_ptr = s.2;
        if rva >= virt_addr && rva < virt_addr + virt_size {
            return Some((raw_ptr + (rva - virt_addr)) as usize);
        }
    }
    None
}

fn read_cstr(data: &[u8], mut off: usize) -> Option<String> {
    let mut out = Vec::new();
    while off < data.len() {
        let b = data[off];
        if b == 0 { break; }
        out.push(b);
        off += 1;
        if out.len() > 256 { break; }
    }
    String::from_utf8(out).ok()
}

pub fn parse_pe_imports(path: &str) -> Result<Profile, PeError> {
    let data = fs::read(path)?;
    if data.len() < 0x40 { return Err(PeError::InvalidDos); }
    if read_u16(&data, 0) != Some(0x5A4D) { return Err(PeError::InvalidDos); }
    let e_lfanew = read_u32(&data, 0x3C).ok_or(PeError::InvalidDos)? as usize;
    if e_lfanew + 4 + 20 + 240 > data.len() { return Err(PeError::Truncated); }
    if read_u32(&data, e_lfanew) != Some(0x00004550) { return Err(PeError::InvalidNt); }
    let machine = read_u16(&data, e_lfanew + 4).ok_or(PeError::InvalidNt)?;
    if machine != 0x8664 { return Err(PeError::NotPe64); }
    let num_sections = read_u16(&data, e_lfanew + 6).ok_or(PeError::InvalidNt)? as usize;
    let size_opt = read_u16(&data, e_lfanew + 20).ok_or(PeError::InvalidNt)? as usize;
    let opt_off = e_lfanew + 24;
    if read_u16(&data, opt_off) != Some(0x20B) { return Err(PeError::NotPe64); }
    let data_dir_off = opt_off + 112;
    let import_rva = read_u32(&data, data_dir_off + 8).ok_or(PeError::NoImportDir)?;
    if import_rva == 0 { return Err(PeError::NoImportDir); }
    let sec_off = opt_off + size_opt;
    let mut sections = Vec::new();
    for i in 0..num_sections {
        let s = sec_off + i * 40;
        if s + 40 > data.len() { break; }
        let virt_addr = read_u32(&data, s + 12).ok_or(PeError::InvalidNt)?;
        let virt_size = read_u32(&data, s + 8).ok_or(PeError::InvalidNt)?;
        let raw_ptr = read_u32(&data, s + 20).ok_or(PeError::InvalidNt)?;
        let raw_size = read_u32(&data, s + 16).ok_or(PeError::InvalidNt)?;
        sections.push((virt_addr, virt_size, raw_ptr, raw_size));
    }
    let import_off = rva_to_offset(&sections, import_rva).ok_or(PeError::NoImportDir)?;
    let mut profile = Profile::new(std::path::Path::new(path).file_stem().and_then(|s| s.to_str()).unwrap_or("unknown"));
    profile.target.path = Some(path.to_string());
    let mut i = 0;
    loop {
        let entry = import_off + i * 20;
        if entry + 20 > data.len() { break; }
        let orig_first_thunk = read_u32(&data, entry + 0).ok_or(PeError::InvalidNt)?;
        let _time = read_u32(&data, entry + 4).ok_or(PeError::InvalidNt)?;
        let _fwd = read_u32(&data, entry + 8).ok_or(PeError::InvalidNt)?;
        let name_rva = read_u32(&data, entry + 12).ok_or(PeError::InvalidNt)?;
        let first_thunk = read_u32(&data, entry + 16).ok_or(PeError::InvalidNt)?;
        if orig_first_thunk == 0 && first_thunk == 0 { break; }
        if name_rva == 0 { i += 1; continue; }
        let name_off = match rva_to_offset(&sections, name_rva) { Some(v) => v, None => { i += 1; continue; } };
        let dll_name = match read_cstr(&data, name_off) { Some(v) => v, None => { i += 1; continue; } };
        if dll_name.is_empty() { i += 1; continue; }
        profile.modules.push(Module { name: dll_name.clone(), required: true });
        let thunk_rva = if orig_first_thunk != 0 { orig_first_thunk } else { first_thunk };
        if let Some(thunk_off) = rva_to_offset(&sections, thunk_rva) {
            let mut t = 0;
            loop {
                let to = thunk_off + t * 8;
                if to + 8 > data.len() { break; }
                let val = read_u64(&data, to).ok_or(PeError::InvalidNt)?;
                if val == 0 { break; }
                if val & 0x8000000000000000 != 0 { t += 1; if t > 512 { break; } continue; }
                let hint_name_rva = val as u32;
                if let Some(hn_off) = rva_to_offset(&sections, hint_name_rva) {
                    if hn_off + 2 < data.len() {
                        if let Some(fname) = read_cstr(&data, hn_off + 2) {
                            profile.imports.push(Import { dll: dll_name.clone(), name: fname, ordinal: None, called: true });
                        }
                    }
                }
                t += 1;
                if t > 512 { break; }
            }
        }
        i += 1;
        if i > 256 { break; }
    }
    Ok(profile)
}