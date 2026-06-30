// win7_winrt_shim/hstring.c
// HSTRING function implementations for Windows 7 compatibility.
//
// These functions replicate the Windows 8+ HSTRING API using standard
// Win32 HeapAlloc/HeapFree. The windows crate uses HSTRING internally
// for COM interface name operations, GUID strings, and BSTR conversions.
//
// Reference: https://learn.microsoft.com/en-us/windows/win32/winrt/hstring

#include "hstring.h"
#include <windows.h>

// Global heap handle for HSTRING allocations
// The real WinRT uses a dedicated heap, but our simple allocator is fine.
static HANDLE g_hstring_heap = NULL;

static HANDLE get_hstring_heap(void) {
    if (g_hstring_heap) return g_hstring_heap;
    g_hstring_heap = GetProcessHeap();
    // Fallback: create a dedicated heap
    if (!g_hstring_heap) {
        g_hstring_heap = HeapCreate(0, 0, 0);
    }
    return g_hstring_heap;
}

// ─── HSTRING functions ────────────────────────────────────────────

HRESULT WINAPI WindowsCreateString(
    LPCWSTR sourceString,
    UINT32  length,
    HSTRING *outString)
{
    if (!outString) return E_INVALIDARG;
    *outString = NULL;

    if (length == 0 && sourceString) {
        // Empty string with non-null source: still treat as empty
    }
    if (length > 0 && !sourceString) return E_INVALIDARG;

    HANDLE heap = get_hstring_heap();
    if (!heap) return E_OUTOFMEMORY;

    SIZE_T allocSize = sizeof(HSTRING_BUFFER_HEADER) + (length + 1) * sizeof(WCHAR);
    HSTRING_BUFFER_HEADER *hdr = (HSTRING_BUFFER_HEADER *)HeapAlloc(heap, HEAP_ZERO_MEMORY, allocSize);
    if (!hdr) return E_OUTOFMEMORY;

    hdr->refcount = 1;
    hdr->length = length;

    HSTRING h = hstring_from_header(hdr);
    if (sourceString && length > 0) {
        memcpy(h, sourceString, length * sizeof(WCHAR));
    }
    h[length] = 0; // null-terminate

    *outString = h;
    return S_OK;
}

HRESULT WINAPI WindowsCreateStringReference(
    LPCWSTR         sourceString,
    UINT32          length,
    HSTRING_HEADER *hstringHeader,
    HSTRING        *outString)
{
    if (!hstringHeader || !outString) return E_INVALIDARG;

    *outString = NULL;
    memset(hstringHeader, 0, sizeof(HSTRING_HEADER));

    if (!sourceString && length > 0) return E_INVALIDARG;

    hstringHeader->u1.Reserved1 = NULL;
    hstringHeader->u2.Reserved3 = NULL;
    hstringHeader->Length = length;

    // "Reference" means we don't allocate — we just point to the source.
    // Windows documentation says sourceString must remain valid for the
    // lifetime of the HSTRING. We store it directly.
    *outString = (HSTRING)sourceString;
    return S_OK;
}

HRESULT WINAPI WindowsDeleteString(HSTRING string)
{
    if (!string) return S_OK;

    HANDLE heap = get_hstring_heap();
    if (!heap) return E_OUTOFMEMORY;

    HSTRING_BUFFER_HEADER *hdr = hstring_get_header(string);
    if (!hdr) return E_INVALIDARG;

    HeapFree(heap, 0, hdr);
    return S_OK;
}

HRESULT WINAPI WindowsDuplicateString(
    HSTRING  string,
    HSTRING *outString)
{
    if (!outString) return E_INVALIDARG;
    if (!string) {
        *outString = NULL;
        return S_OK;
    }

    HSTRING_BUFFER_HEADER *hdr = hstring_get_header(string);
    if (!hdr) return E_INVALIDARG;

    return WindowsCreateString(string, hdr->length, outString);
}

UINT32 WINAPI WindowsGetStringLen(HSTRING string)
{
    if (!string) return 0;
    HSTRING_BUFFER_HEADER *hdr = hstring_get_header(string);
    if (!hdr) return 0;
    return hdr->length;
}

LPCWSTR WINAPI WindowsGetStringRawBuffer(
    HSTRING  string,
    UINT32  *length)
{
    if (!string) {
        if (length) *length = 0;
        return L"";
    }
    if (length) {
        HSTRING_BUFFER_HEADER *hdr = hstring_get_header(string);
        *length = hdr ? hdr->length : 0;
    }
    return string;
}

BOOL WINAPI WindowsIsStringEmpty(HSTRING string)
{
    return WindowsGetStringLen(string) == 0;
}

HRESULT WINAPI WindowsStringHasEmbeddedNull(
    HSTRING string,
    BOOL   *hasEmbeddedNull)
{
    if (!hasEmbeddedNull) return E_INVALIDARG;
    *hasEmbeddedNull = FALSE;

    if (!string) return S_OK;

    HSTRING_BUFFER_HEADER *hdr = hstring_get_header(string);
    if (!hdr) return S_OK;

    for (UINT32 i = 0; i < hdr->length; i++) {
        if (string[i] == 0) {
            *hasEmbeddedNull = TRUE;
            return S_OK;
        }
    }
    return S_OK;
}

HRESULT WINAPI WindowsCompareStringOrdinal(
    HSTRING string1,
    HSTRING string2,
    INT32  *result)
{
    if (!result) return E_INVALIDARG;

    if (string1 == string2) {
        *result = 0;
        return S_OK;
    }
    if (!string1) {
        *result = -1;
        return S_OK;
    }
    if (!string2) {
        *result = 1;
        return S_OK;
    }

    UINT32 len1 = WindowsGetStringLen(string1);
    UINT32 len2 = WindowsGetStringLen(string2);
    UINT32 minLen = len1 < len2 ? len1 : len2;

    int cmp = memcmp(string1, string2, minLen * sizeof(WCHAR));
    if (cmp == 0) {
        // Shorter string is "less"
        *result = (INT32)(len1 - len2);
    } else {
        // memcmp returns <0 if first differing byte in s1 < s2
        // For WCHAR comparison (little-endian), this is correct for ordinal comparison
        *result = cmp;
    }
    return S_OK;
}

HRESULT WINAPI WindowsConcatString(
    HSTRING  string1,
    HSTRING  string2,
    HSTRING *outString)
{
    if (!outString) return E_INVALIDARG;

    UINT32 len1 = WindowsGetStringLen(string1);
    UINT32 len2 = WindowsGetStringLen(string2);
    UINT32 totalLen = len1 + len2;

    HANDLE heap = get_hstring_heap();
    if (!heap) return E_OUTOFMEMORY;

    SIZE_T allocSize = sizeof(HSTRING_BUFFER_HEADER) + (totalLen + 1) * sizeof(WCHAR);
    HSTRING_BUFFER_HEADER *hdr = (HSTRING_BUFFER_HEADER *)HeapAlloc(heap, HEAP_ZERO_MEMORY, allocSize);
    if (!hdr) return E_OUTOFMEMORY;

    hdr->refcount = 1;
    hdr->length = totalLen;

    HSTRING h = hstring_from_header(hdr);
    if (string1 && len1 > 0) {
        memcpy(h, string1, len1 * sizeof(WCHAR));
    }
    if (string2 && len2 > 0) {
        memcpy(h + len1, string2, len2 * sizeof(WCHAR));
    }
    h[totalLen] = 0;

    *outString = h;
    return S_OK;
}

HRESULT WINAPI WindowsSubstring(
    HSTRING  string,
    UINT32   startIndex,
    HSTRING *outString)
{
    if (!outString) return E_INVALIDARG;
    if (!string) return E_INVALIDARG;

    UINT32 len = WindowsGetStringLen(string);
    return WindowsSubstringWithSpecifiedLength(string, startIndex, len - startIndex, outString);
}

HRESULT WINAPI WindowsSubstringWithSpecifiedLength(
    HSTRING  string,
    UINT32   startIndex,
    UINT32   length,
    HSTRING *outString)
{
    if (!outString) return E_INVALIDARG;
    if (!string) return E_INVALIDARG;

    UINT32 totalLen = WindowsGetStringLen(string);
    if (startIndex > totalLen) return E_BOUNDS;
    if (startIndex + length > totalLen) return E_BOUNDS;

    return WindowsCreateString(string + startIndex, length, outString);
}

HRESULT WINAPI WindowsReplaceString(
    HSTRING  string,
    HSTRING  stringReplaced,
    HSTRING  stringReplaceWith,
    HSTRING *outString)
{
    if (!outString) return E_INVALIDARG;
    if (!string || !stringReplaced || !stringReplaceWith) return E_INVALIDARG;

    UINT32 srcLen = WindowsGetStringLen(string);
    UINT32 replacedLen = WindowsGetStringLen(stringReplaced);
    UINT32 withLen = WindowsGetStringLen(stringReplaceWith);

    *outString = NULL;

    // Find first occurrence of stringReplaced in string
    if (replacedLen == 0 || replacedLen > srcLen) {
        // No replacement possible, duplicate original
        return WindowsDuplicateString(string, outString);
    }

    LPCWSTR src = WindowsGetStringRawBuffer(string, NULL);
    LPCWSTR rep = WindowsGetStringRawBuffer(stringReplaced, NULL);

    // Simple linear search
    INT32 foundAt = -1;
    for (UINT32 i = 0; i <= srcLen - replacedLen; i++) {
        if (memcmp(src + i, rep, replacedLen * sizeof(WCHAR)) == 0) {
            foundAt = (INT32)i;
            break;
        }
    }

    if (foundAt < 0) {
        return WindowsDuplicateString(string, outString);
    }

    UINT32 newLen = srcLen - replacedLen + withLen;
    HANDLE heap = get_hstring_heap();
    if (!heap) return E_OUTOFMEMORY;

    SIZE_T allocSize = sizeof(HSTRING_BUFFER_HEADER) + (newLen + 1) * sizeof(WCHAR);
    HSTRING_BUFFER_HEADER *hdr = (HSTRING_BUFFER_HEADER *)HeapAlloc(heap, HEAP_ZERO_MEMORY, allocSize);
    if (!hdr) return E_OUTOFMEMORY;

    hdr->refcount = 1;
    hdr->length = newLen;
    HSTRING h = hstring_from_header(hdr);

    // Copy before match
    if (foundAt > 0) {
        memcpy(h, src, foundAt * sizeof(WCHAR));
    }
    // Copy replacement
    LPCWSTR with = WindowsGetStringRawBuffer(stringReplaceWith, NULL);
    memcpy(h + foundAt, with, withLen * sizeof(WCHAR));
    // Copy after match
    UINT32 afterStart = foundAt + replacedLen;
    if (afterStart < srcLen) {
        memcpy(h + foundAt + withLen, src + afterStart, (srcLen - afterStart) * sizeof(WCHAR));
    }
    h[newLen] = 0;

    *outString = h;
    return S_OK;
}

HRESULT WINAPI WindowsTrimStringStart(
    HSTRING  string,
    HSTRING  trimString,
    HSTRING *outString)
{
    if (!outString) return E_INVALIDARG;
    if (!string) return E_INVALIDARG;

    LPCWSTR src = WindowsGetStringRawBuffer(string, NULL);
    UINT32 srcLen = WindowsGetStringLen(string);

    LPCWSTR trim = L" \t\r\n"; // default whitespace
    UINT32 trimLen = 4;
    if (trimString) {
        trim = WindowsGetStringRawBuffer(trimString, &trimLen);
    }

    UINT32 start = 0;
    while (start < srcLen) {
        BOOL found = FALSE;
        for (UINT32 i = 0; i < trimLen; i++) {
            if (src[start] == trim[i]) {
                found = TRUE;
                break;
            }
        }
        if (!found) break;
        start++;
    }

    return WindowsCreateString(src + start, srcLen - start, outString);
}

HRESULT WINAPI WindowsTrimStringEnd(
    HSTRING  string,
    HSTRING  trimString,
    HSTRING *outString)
{
    if (!outString) return E_INVALIDARG;
    if (!string) return E_INVALIDARG;

    LPCWSTR src = WindowsGetStringRawBuffer(string, NULL);
    UINT32 srcLen = WindowsGetStringLen(string);

    LPCWSTR trim = L" \t\r\n";
    UINT32 trimLen = 4;
    if (trimString) {
        trim = WindowsGetStringRawBuffer(trimString, &trimLen);
    }

    UINT32 end = srcLen;
    while (end > 0) {
        BOOL found = FALSE;
        for (UINT32 i = 0; i < trimLen; i++) {
            if (src[end - 1] == trim[i]) {
                found = TRUE;
                break;
            }
        }
        if (!found) break;
        end--;
    }

    return WindowsCreateString(src, end, outString);
}

// Preallocation APIs — used for building HSTRINGs incrementally
HRESULT WINAPI WindowsPreallocateStringBuffer(
    UINT32   length,
    WCHAR  **charBuffer,
    HSTRING *bufferHandle)
{
    if (!charBuffer || !bufferHandle) return E_INVALIDARG;
    *charBuffer = NULL;
    *bufferHandle = NULL;

    if (length == 0) return S_OK;

    HANDLE heap = get_hstring_heap();
    if (!heap) return E_OUTOFMEMORY;

    SIZE_T allocSize = sizeof(HSTRING_BUFFER_HEADER) + (length + 1) * sizeof(WCHAR);
    HSTRING_BUFFER_HEADER *hdr = (HSTRING_BUFFER_HEADER *)HeapAlloc(heap, HEAP_ZERO_MEMORY, allocSize);
    if (!hdr) return E_OUTOFMEMORY;

    hdr->refcount = 1;
    hdr->length = 0; // Will be set by WindowsPromoteStringBuffer

    HSTRING h = hstring_from_header(hdr);
    *bufferHandle = h;
    *charBuffer = h;
    return S_OK;
}

HRESULT WINAPI WindowsPromoteStringBuffer(
    HSTRING  bufferHandle,
    UINT32   length,
    HSTRING *outString)
{
    if (!outString) return E_INVALIDARG;
    if (!bufferHandle) {
        *outString = NULL;
        return S_OK;
    }

    HSTRING_BUFFER_HEADER *hdr = hstring_get_header(bufferHandle);
    if (!hdr) return E_INVALIDARG;

    hdr->length = length;
    bufferHandle[length] = 0;

    *outString = bufferHandle;
    return S_OK;
}

HRESULT WINAPI WindowsDeleteStringBuffer(HSTRING bufferHandle)
{
    if (!bufferHandle) return S_OK;
    return WindowsDeleteString(bufferHandle);
}
