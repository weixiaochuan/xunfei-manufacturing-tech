//! Win7 兼容性 stub 函数 —— 替代 win7_stubs.c
//!
//! 提供 api-ms-win-core-synch-l1-2-0、combase、bcryptprimitives 等 Win8+ DLL
//! 中函数的 stub 实现。通过 #[no_mangle] extern "system" 导出。
//!
//! 注意：CoTaskMemFree/CoTaskMemAlloc/CoCreateFreeThreadedMarshaler 在 Win8+ SDK
//! 中被 windows crate 从 combase.dll 导入。Win7 上 combase.dll 不存在，必须 stub
//! 并转发到 ole32.dll。ole32.lib 始终链接导致的 LNK1169 冲突由 build.rs 的
//! /FORCE:MULTIPLE 解决（我们的 .o 排在 .rlib 前面，先出现者赢）。
//!
//! 运行时行为：
//! - Win8+: 真实 DLL 存在，stub 加载并转发到真实实现
//! - Win7:  DLL 不存在，使用安全 fallback
//!
//! # Safety
//! 所有 #[no_mangle] 函数均为 `unsafe extern "system"`，直接暴露给 linker/OS。
//! 参数名保留 Windows API 原始 CamelCase（非 Rust snake_case），需全局允许 non_snake_case。

#![allow(non_snake_case, non_camel_case_types)]

use std::ffi::c_void;
use std::sync::Once;

// ─── Windows API 类型 ───

type PVOID = *mut c_void;
type HMODULE = *mut c_void;
type DWORD = u32;
type SIZE_T = u64;
type ULONGLONG = u64;
type CO_MTA_USAGE_COOKIE = ULONGLONG;
type BOOL = i32;

const INFINITE: DWORD = 0xFFFFFFFF;
const S_OK: i32 = 0;

// ─── FFI imports (kernel32 / advapi32，所有 Windows 版本均存在) ───

extern "system" {
    fn LoadLibraryW(lpLibFileName: *const u16) -> HMODULE;
    fn GetProcAddress(hModule: HMODULE, lpProcName: *const u8) -> *mut c_void;
    fn Sleep(dwMilliseconds: DWORD);
    fn SystemFunction036(RandomBuffer: PVOID, RandomBufferLength: u32) -> u8;
    fn GetSystemTimeAsFileTime(lpSystemTimeAsFileTime: *mut FILETIME);
}

/// FILETIME 结构体定义（与 Windows API 兼容）
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub(crate) struct FILETIME {
    pub(crate) dw_low_date_time: u32,
    pub(crate) dw_high_date_time: u32,
}

/// LPFILETIME 类型别名
type LPFILETIME = *mut FILETIME;

// ─── 辅助函数 ───

unsafe fn load_library(name: &str) -> HMODULE {
    let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe { LoadLibraryW(wide.as_ptr()) }
}

// ═══════════════════════════════════════════════════════
// api-ms-win-core-synch-l1-2-0.dll (Win8+)
// ═══════════════════════════════════════════════════════

static SYNCH_INIT: Once = Once::new();
static mut SYNCH_DLL: HMODULE = std::ptr::null_mut();

unsafe fn synch_init() {
    SYNCH_INIT.call_once(|| {
        unsafe {
            SYNCH_DLL = load_library("api-ms-win-core-synch-l1-2-0.dll");
        }
    });
}

#[no_mangle]
pub unsafe extern "system" fn WakeByAddressSingle(Address: PVOID) {
    unsafe {
        synch_init();
        if !SYNCH_DLL.is_null() {
            let f: Option<unsafe extern "system" fn(PVOID)> =
                std::mem::transmute(GetProcAddress(SYNCH_DLL, b"WakeByAddressSingle\0".as_ptr()));
            if let Some(f) = f {
                f(Address);
                return;
            }
        }
    }
    let _ = Address;
}

#[no_mangle]
pub unsafe extern "system" fn WakeByAddressAll(Address: PVOID) {
    unsafe {
        synch_init();
        if !SYNCH_DLL.is_null() {
            let f: Option<unsafe extern "system" fn(PVOID)> =
                std::mem::transmute(GetProcAddress(SYNCH_DLL, b"WakeByAddressAll\0".as_ptr()));
            if let Some(f) = f {
                f(Address);
                return;
            }
        }
    }
    let _ = Address;
}

#[no_mangle]
pub unsafe extern "system" fn WaitOnAddress(
    Address: *mut c_void,
    CompareAddress: *mut c_void,
    AddressSize: SIZE_T,
    dwMilliseconds: DWORD,
) -> BOOL {
    unsafe {
        synch_init();
        if !SYNCH_DLL.is_null() {
            let f: Option<
                unsafe extern "system" fn(*mut c_void, *mut c_void, SIZE_T, DWORD) -> BOOL,
            > = std::mem::transmute(GetProcAddress(SYNCH_DLL, b"WaitOnAddress\0".as_ptr()));
            if let Some(f) = f {
                return f(Address, CompareAddress, AddressSize, dwMilliseconds);
            }
        }
    }
    let _ = Address;
    let _ = CompareAddress;
    let _ = AddressSize;
    let ms = if dwMilliseconds == INFINITE { 1000 } else { dwMilliseconds };
    unsafe { Sleep(ms) };
    1
}

// ═══════════════════════════════════════════════════════
// combase.dll (Win8+) — CoIncrementMTAUsage
// ═══════════════════════════════════════════════════════

#[no_mangle]
pub unsafe extern "system" fn CoIncrementMTAUsage(pCookie: *mut CO_MTA_USAGE_COOKIE) -> i32 {
    static COMBASE_INIT: Once = Once::new();
    static mut COMBASE_DLL: HMODULE = std::ptr::null_mut();

    unsafe {
        COMBASE_INIT.call_once(|| {
            COMBASE_DLL = load_library("combase.dll");
        });

        if !COMBASE_DLL.is_null() {
            let f: Option<unsafe extern "system" fn(*mut CO_MTA_USAGE_COOKIE) -> i32> =
                std::mem::transmute(GetProcAddress(
                    COMBASE_DLL,
                    b"CoIncrementMTAUsage\0".as_ptr(),
                ));
            if let Some(f) = f {
                return f(pCookie);
            }
        }
    }

    // Win7 stub
    if !pCookie.is_null() {
        unsafe { *pCookie = 1 as CO_MTA_USAGE_COOKIE };
    }
    S_OK
}

// ═══════════════════════════════════════════════════════
// combase.dll (Win8+) — CoTaskMemAlloc / CoTaskMemFree
//   / CoCreateFreeThreadedMarshaler → 转发到 ole32.dll
// Win8+ SDK 将这三个函数从 ole32.dll 移到了 combase.dll。
// Win7 上 combase.dll 不存在，但 ole32.dll 自 XP 起就有这些导出。
// ═══════════════════════════════════════════════════════

static OLE32_INIT: Once = Once::new();
static mut OLE32_DLL: HMODULE = std::ptr::null_mut();

unsafe fn ole32_get(name: &[u8]) -> *mut c_void {
    OLE32_INIT.call_once(|| unsafe {
        OLE32_DLL = load_library("ole32.dll");
    });
    if OLE32_DLL.is_null() {
        return std::ptr::null_mut();
    }
    unsafe { GetProcAddress(OLE32_DLL, name.as_ptr()) }
}

#[no_mangle]
pub unsafe extern "system" fn CoTaskMemAlloc(cb: SIZE_T) -> PVOID {
    unsafe {
        let f: Option<unsafe extern "system" fn(SIZE_T) -> PVOID> =
            std::mem::transmute(ole32_get(b"CoTaskMemAlloc\0"));
        if let Some(f) = f {
            return f(cb);
        }
    }
    std::ptr::null_mut()
}

#[no_mangle]
pub unsafe extern "system" fn CoTaskMemFree(pv: PVOID) {
    unsafe {
        let f: Option<unsafe extern "system" fn(PVOID)> =
            std::mem::transmute(ole32_get(b"CoTaskMemFree\0"));
        if let Some(f) = f {
            f(pv);
        }
    }
}

#[no_mangle]
pub unsafe extern "system" fn CoCreateFreeThreadedMarshaler(
    punkOuter: *mut c_void,
    ppunkMarshal: *mut *mut c_void,
) -> i32 {
    unsafe {
        let f: Option<unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> i32> =
            std::mem::transmute(ole32_get(b"CoCreateFreeThreadedMarshaler\0"));
        if let Some(f) = f {
            return f(punkOuter, ppunkMarshal);
        }
    }
    if !ppunkMarshal.is_null() {
        unsafe { *ppunkMarshal = std::ptr::null_mut() };
    }
    0x8007_000E_u32 as i32 // E_OUTOFMEMORY
}

// ═══════════════════════════════════════════════════════
// bcryptprimitives.dll (Win8+) — ProcessPrng
// 转发到 RtlGenRandom/SystemFunction036 (advapi32.dll, XP+)
// ═══════════════════════════════════════════════════════

#[no_mangle]
pub unsafe extern "system" fn ProcessPrng(pbData: PVOID, cbData: ULONGLONG) -> BOOL {
    if cbData > 0xFFFF_FFFF {
        return 0xC000_000D_u32 as i32; // STATUS_INVALID_PARAMETER
    }
    let ok = unsafe { SystemFunction036(pbData, cbData as u32) };
    if ok != 0 {
        0 // STATUS_SUCCESS
    } else {
        0xC000_000F_u32 as i32 // STATUS_UNSUCCESSFUL
    }
}

// ═══════════════════════════════════════════════════════
// kernel32.dll (Win8+) — GetSystemTimePreciseAsFileTime
// 转发到 GetSystemTimeAsFileTime（Win7 可用）
// ═══════════════════════════════════════════════════════

static KERNEL32_INIT: Once = Once::new();
static mut KERNEL32_DLL: HMODULE = std::ptr::null_mut();

#[no_mangle]
pub unsafe extern "system" fn GetSystemTimePreciseAsFileTime(lpSystemTimeAsFileTime: LPFILETIME) {
    unsafe {
        KERNEL32_INIT.call_once(|| {
            KERNEL32_DLL = load_library("kernel32.dll");
        });

        if !KERNEL32_DLL.is_null() {
            let f: Option<unsafe extern "system" fn(LPFILETIME)> =
                std::mem::transmute(GetProcAddress(
                    KERNEL32_DLL,
                    b"GetSystemTimePreciseAsFileTime\0".as_ptr(),
                ));
            if let Some(f) = f {
                f(lpSystemTimeAsFileTime);
                return;
            }
        }
    }

    // Win7 fallback: 使用 GetSystemTimeAsFileTime
    unsafe {
        GetSystemTimeAsFileTime(lpSystemTimeAsFileTime);
    }
}
