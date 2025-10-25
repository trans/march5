#![allow(unsafe_op_in_unsafe_fn)]

use std::ptr;

use anyhow::{Result, anyhow};
use libc::{
    MAP_ANON, MAP_FAILED, MAP_PRIVATE, PROT_EXEC, PROT_READ, PROT_WRITE, mmap, mprotect, munmap,
};
use once_cell::sync::OnceCell;

#[cfg(all(target_arch = "x86_64", target_os = "linux"))]
type BinFn = unsafe extern "sysv64" fn(i64, i64) -> i64;

#[cfg(not(all(target_arch = "x86_64", target_os = "linux")))]
type BinFn = unsafe extern "C" fn(i64, i64) -> i64;

const ADD_BYTES: &[u8] = &[0x48, 0x89, 0xf8, 0x48, 0x01, 0xf0, 0xc3];
const SUB_BYTES: &[u8] = &[0x48, 0x89, 0xf8, 0x48, 0x29, 0xf0, 0xc3];

static ADD_PTR: OnceCell<BinFn> = OnceCell::new();
static SUB_PTR: OnceCell<BinFn> = OnceCell::new();

pub fn compiled_add() -> Result<BinFn> {
    ADD_PTR
        .get_or_try_init(|| unsafe { load_exec_binary(ADD_BYTES) })
        .map(|f| *f)
}

pub fn compiled_sub() -> Result<BinFn> {
    SUB_PTR
        .get_or_try_init(|| unsafe { load_exec_binary(SUB_BYTES) })
        .map(|f| *f)
}

unsafe fn load_exec_binary(bytes: &[u8]) -> Result<BinFn> {
    let page_size = libc::sysconf(libc::_SC_PAGESIZE) as usize;
    if page_size == 0 {
        return Err(anyhow!("sysconf(_SC_PAGESIZE) returned 0"));
    }
    let len = bytes.len();
    let alloc_len = ((len + page_size - 1) / page_size) * page_size;
    let ptr = mmap(
        std::ptr::null_mut(),
        alloc_len,
        PROT_READ | PROT_WRITE,
        MAP_PRIVATE | MAP_ANON,
        -1,
        0,
    );
    if ptr == MAP_FAILED {
        return Err(anyhow!("mmap failed"));
    }
    ptr::copy_nonoverlapping(bytes.as_ptr(), ptr as *mut u8, len);
    if mprotect(ptr, alloc_len, PROT_READ | PROT_EXEC) != 0 {
        munmap(ptr, alloc_len);
        return Err(anyhow!("mprotect failed"));
    }
    let func: BinFn = std::mem::transmute(ptr);
    Ok(func)
}
