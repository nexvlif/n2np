use std::alloc::{alloc_zeroed, dealloc, realloc as alloc_realloc, Layout};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

pub const MEM_COMMIT: u32 = 0x1000;
pub const MEM_RESERVE: u32 = 0x2000;
pub const MEM_DECOMMIT: u32 = 0x4000;
pub const MEM_RELEASE: u32 = 0x8000;

pub const PAGE_NOACCESS: u32 = 0x01;
pub const PAGE_READONLY: u32 = 0x02;
pub const PAGE_READWRITE: u32 = 0x04;
pub const PAGE_WRITECOPY: u32 = 0x08;
pub const PAGE_EXECUTE: u32 = 0x10;
pub const PAGE_EXECUTE_READ: u32 = 0x20;
pub const PAGE_EXECUTE_READWRITE: u32 = 0x40;
pub const PAGE_EXECUTE_WRITECOPY: u32 = 0x80;

pub const HEAP_ZERO_MEMORY: u32 = 0x0000_0008;

#[derive(Clone, Copy)]
struct Block {
    size: usize,
    align: usize,
    protect: u32,
}

static BLOCKS: OnceLock<Mutex<HashMap<usize, Block>>> = OnceLock::new();

fn blocks() -> &'static Mutex<HashMap<usize, Block>> {
    BLOCKS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn layout(size: usize, align: usize) -> Layout {
    Layout::from_size_align(size.max(1), align.max(1)).expect("invalid allocation layout")
}

pub fn alloc(size: usize, align: usize, zero: bool, protect: u32) -> *mut u8 {
    let align = align.max(16).next_power_of_two();
    let layout = layout(size, align);
    let ptr = unsafe {
        if zero {
            alloc_zeroed(layout)
        } else {
            std::alloc::alloc(layout)
        }
    };
    if ptr.is_null() {
        return std::ptr::null_mut();
    }
    blocks().lock().unwrap().insert(
        ptr as usize,
        Block {
            size: size.max(1),
            align,
            protect,
        },
    );
    ptr
}

pub fn free(ptr: *mut u8) -> bool {
    if ptr.is_null() {
        return false;
    }
    let block = match blocks().lock().unwrap().remove(&(ptr as usize)) {
        Some(block) => block,
        None => return false,
    };
    unsafe {
        dealloc(ptr, layout(block.size, block.align));
    }
    true
}

pub fn realloc(ptr: *mut u8, size: usize) -> *mut u8 {
    if ptr.is_null() {
        return alloc(size, 16, false, PAGE_READWRITE);
    }
    if size == 0 {
        free(ptr);
        return std::ptr::null_mut();
    }
    let block = match blocks().lock().unwrap().get(&(ptr as usize)).copied() {
        Some(block) => block,
        None => return std::ptr::null_mut(),
    };
    let new_ptr = unsafe { alloc_realloc(ptr, layout(block.size, block.align), size) };
    if new_ptr.is_null() {
        return std::ptr::null_mut();
    }
    if new_ptr != ptr {
        let mut map = blocks().lock().unwrap();
        map.remove(&(ptr as usize));
        map.insert(
            new_ptr as usize,
            Block {
                size,
                align: block.align,
                protect: block.protect,
            },
        );
    } else {
        blocks()
            .lock()
            .unwrap()
            .insert(ptr as usize, Block { size, ..block });
    }
    new_ptr
}

pub fn block_size(ptr: *mut u8) -> Option<usize> {
    blocks()
        .lock()
        .unwrap()
        .get(&(ptr as usize))
        .map(|block| block.size)
}

pub fn block_info(ptr: usize) -> Option<(usize, u32)> {
    blocks()
        .lock()
        .unwrap()
        .get(&ptr)
        .map(|block| (block.size, block.protect))
}

pub fn protect(ptr: *mut u8, size: usize, protect: u32) -> bool {
    if ptr.is_null() || size == 0 {
        return false;
    }
    {
        let mut map = blocks().lock().unwrap();
        if let Some(block) = map.get_mut(&(ptr as usize)) {
            block.protect = protect;
        } else {
            return false;
        }
    }
    protect_pages(ptr, size, protect)
}

#[cfg(all(unix, not(target_os = "macos")))]
fn protect_pages(ptr: *mut u8, size: usize, protect: u32) -> bool {
    let page = 4096usize;
    let start = (ptr as usize) & !(page - 1);
    let end = (ptr as usize + size + page - 1) & !(page - 1);
    let prot = match protect & 0xFF {
        PAGE_NOACCESS => libc::PROT_NONE,
        PAGE_READONLY | PAGE_EXECUTE | PAGE_EXECUTE_READ => libc::PROT_READ,
        PAGE_READWRITE | PAGE_WRITECOPY | PAGE_EXECUTE_READWRITE | PAGE_EXECUTE_WRITECOPY => {
            libc::PROT_READ | libc::PROT_WRITE
        }
        _ => libc::PROT_READ | libc::PROT_WRITE,
    };
    let prot = if protect & 0xF0 != 0 || protect == PAGE_EXECUTE {
        prot | libc::PROT_EXEC
    } else {
        prot
    };
    unsafe { libc::mprotect(start as *mut libc::c_void, end - start, prot) == 0 }
}

#[cfg(not(all(unix, not(target_os = "macos"))))]
fn protect_pages(_ptr: *mut u8, _size: usize, _protect: u32) -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alloc_tracks_and_frees_blocks() {
        let ptr = alloc(64, 16, true, PAGE_READWRITE);
        assert!(!ptr.is_null());
        assert_eq!(block_size(ptr), Some(64));
        assert!(block_info(ptr as usize).is_some());
        assert!(free(ptr));
        assert_eq!(block_size(ptr), None);
        assert!(!free(ptr));
    }

    #[test]
    fn realloc_preserves_contents() {
        let ptr = alloc(8, 16, false, PAGE_READWRITE);
        unsafe {
            std::ptr::copy_nonoverlapping(b"n2np".as_ptr(), ptr, 4);
        }
        let ptr = realloc(ptr, 4096);
        assert!(!ptr.is_null());
        assert_eq!(block_size(ptr), Some(4096));
        unsafe {
            assert_eq!(std::slice::from_raw_parts(ptr, 4), b"n2np");
        }
        assert!(free(ptr));
    }
}
