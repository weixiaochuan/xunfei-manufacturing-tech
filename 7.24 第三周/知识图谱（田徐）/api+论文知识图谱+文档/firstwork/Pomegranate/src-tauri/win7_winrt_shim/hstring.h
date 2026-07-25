// win7_winrt_shim/hstring.h
// HSTRING memory layout compatible with windows crate (windows-core)
//
// HSTRING is a heap-allocated, immutable, reference-counted UTF-16 string.
// It is layout-compatible with BSTR: the pointer points to the data,
// and the 4 bytes immediately before the data contain the byte length (not character count).
//
// For our shim, we use a simplified layout:
//   [refcount:4] [length_chars:4] [padding:4] [padding:4] [data: wchar_t[]]
//
// The windows crate's HSTRING type uses a different internal layout managed by
// the OS heap, but the ABI is identical: HSTRING is just wchar_t*.

#ifndef WIN7_WINRT_HSTRING_H
#define WIN7_WINRT_HSTRING_H

#include <windows.h>

#ifdef __cplusplus
extern "C" {
#endif

// HSTRING is a pointer to the string buffer (wchar_t*)
// The actual Windows HSTRING has a reference-counted header before the data.
typedef WCHAR *HSTRING;

// HSTRING_HEADER for stack-allocated "reference" strings
typedef struct {
    union {
        void *Reserved1;
        WCHAR Reserved2;
    } u1;
    union {
        void *Reserved3;
        WCHAR Reserved4;
    } u2;
    UINT32 Length;
    UINT32 Padding16;
} HSTRING_HEADER;

// RO_INIT_TYPE for RoInitialize
typedef enum {
    RO_INIT_SINGLETHREADED = 0,
    RO_INIT_MULTITHREADED  = 1
} RO_INIT_TYPE;

// Internal header placed before HSTRING data in our heap allocation
typedef struct {
    LONG    refcount;       // reference count (unused, kept for layout compat)
    UINT32  length;         // character count (not including null terminator)
    UINT32  padding1;
    UINT32  padding2;
    // WCHAR data[length + 1] follows
} HSTRING_BUFFER_HEADER;

// Helper to get the internal header from an HSTRING pointer
static inline HSTRING_BUFFER_HEADER *hstring_get_header(HSTRING h) {
    if (!h) return NULL;
    return ((HSTRING_BUFFER_HEADER *)h) - 1;
}

// Helper to get data pointer from header
static inline HSTRING hstring_from_header(HSTRING_BUFFER_HEADER *hdr) {
    return (HSTRING)(hdr + 1);
}

#ifdef __cplusplus
}
#endif

#endif // WIN7_WINRT_HSTRING_H
