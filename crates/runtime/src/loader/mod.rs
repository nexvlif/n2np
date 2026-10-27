use crate::winapi;
use n2np_pe::{PeImage, Section};
use std::ffi::c_void;
use std::ptr;

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
}

const PAGE_SIZE: usize = 4096;
const IMAGE_SCN_MEM_EXECUTE: u32 = 0x2000_0000;
const IMAGE_SCN_MEM_READ: u32 = 0x4000_0000;
const IMAGE_SCN_MEM_WRITE: u32 = 0x8000_0000;

fn align_up(value: usize, align: usize) -> usize {
    (value + align - 1) & !(align - 1)
}

fn map_image(image: &PeImage) -> Result<(*mut u8, usize, bool), LoadError> {
    let size = align_up(image.size_of_image as usize, PAGE_SIZE);
    let preferred = image.image_base as *mut c_void;
    let prot = libc::PROT_READ | libc::PROT_WRITE | libc::PROT_EXEC;

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

fn resolve_imports(image: &PeImage, base: *mut u8) -> Result<(), LoadError> {
    for desc in &image.imports {
        for entry in &desc.entries {
            let resolved = match (&entry.name, entry.ordinal) {
                (Some(name), _) => winapi::resolve(&desc.dll, name),
                (None, Some(ordinal)) => winapi::resolve_ordinal(&desc.dll, ordinal),
                _ => None,
            };
            let address = match resolved {
                Some(address) => address,
                None => {
                    let name = entry
                        .name
                        .clone()
                        .unwrap_or_else(|| format!("#{}", entry.ordinal.unwrap_or(0)));
                    log::warn!(
                        "[n2np-loader] unresolved thunk {}!{} (iat rva 0x{:x})",
                        desc.dll,
                        name,
                        entry.iat_rva
                    );
                    winapi::missing_thunk as *const () as usize
                }
            };
            let slot = unsafe { base.add(entry.iat_rva as usize) as *mut u64 };
            unsafe {
                ptr::write_unaligned(slot, address as u64);
            }
        }
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
        let end = align_up(base as usize + section.virtual_address as usize + len, PAGE_SIZE);
        let prot = section_protection(section);
        let rc = unsafe {
            libc::mprotect(
                start as *mut c_void,
                end.saturating_sub(start),
                prot,
            )
        };
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

pub fn run(path: &str) -> Result<u32, LoadError> {
    let image = PeImage::from_path(path)?;
    if image.machine != 0x8664 {
        return Err(LoadError::UnsupportedArch);
    }

    log::info!(
        "[n2np-loader] {} image_base=0x{:x} entry=0x{:x} size=0x{:x}",
        image.path,
        image.image_base,
        image.entry_point,
        image.size_of_image
    );

    let (base, size, _at_preferred) = map_image(&image)?;
    copy_image(&image, base)?;
    apply_relocations(&image, base)?;
    resolve_imports(&image, base)?;
    protect_sections(&image, base)?;

    let entry = unsafe { base.add(image.entry_point as usize) };
    log::info!("[n2np-loader] jumping to entry at {:?}", entry);
    let entry_fn: unsafe extern "win64" fn() -> u32 = unsafe { std::mem::transmute(entry) };
    let code = unsafe { entry_fn() };

    unsafe {
        libc::munmap(base as *mut c_void, size);
    }
    Ok(code)
}
