// win7_winrt_shim/bcrypt_stubs.c
// bcryptprimitives.dll — unified Win7 compatibility shim.
//
// Exports:
//   ProcessPrng       → forwards to advapi32!RtlGenRandom (XP+)
//   WaitOnAddress     → Win7 poll-based fallback (Win8+ futex)
//   WakeByAddressSingle → no-op (poll-based WaitOnAddress)
//   WakeByAddressAll    → no-op
//
// Used by:
//   - ring crate          → bcryptprimitives!ProcessPrng (direct import)
//   - Rust std / parking_lot → WaitOnAddress via delay-load
//     (PE delay-load table is patched at build time to reference bcryptprimitives.dll)

#include <windows.h>

// ─── ProcessPrng (ring crate → direct import) ──────────────────

#define STATUS_SUCCESS          0x00000000
#define STATUS_UNSUCCESSFUL     0xC0000001

static HMODULE get_advapi32(void)
{
    static HMODULE h = NULL;
    if (!h) h = LoadLibraryW(L"advapi32.dll");
    return h;
}

int WINAPI ProcessPrng(void *pbData, unsigned long long cbData)
{
    if (cbData > 0xFFFFFFFFull) {
        return (int)0xC000000D; // STATUS_INVALID_PARAMETER
    }

    HMODULE h = get_advapi32();
    if (!h) return STATUS_UNSUCCESSFUL;

    typedef BOOLEAN (WINAPI *fn)(void *, ULONG);
    fn RtlGenRandom = (fn)GetProcAddress(h, "SystemFunction036");
    if (!RtlGenRandom) return STATUS_UNSUCCESSFUL;

    return RtlGenRandom(pbData, (ULONG)cbData) ? STATUS_SUCCESS : STATUS_UNSUCCESSFUL;
}

// ─── WaitOnAddress / WakeByAddressSingle (Win8+ futex → Win7 poll) ───

static int compare_bytes(const void *a, const void *b, SIZE_T size)
{
    switch (size) {
    case 1: return *(const unsigned char *)a != *(const unsigned char *)b;
    case 2: return *(const unsigned short *)a != *(const unsigned short *)b;
    case 4: return *(const unsigned long *)a != *(const unsigned long *)b;
    case 8: return memcmp(a, b, 8) != 0;
    default: return -1;
    }
}

BOOL WINAPI WaitOnAddress(
    volatile VOID *Address,
    PVOID CompareAddress,
    SIZE_T AddressSize,
    DWORD dwMilliseconds
)
{
    if (AddressSize != 1 && AddressSize != 2 && AddressSize != 4 && AddressSize != 8) {
        SetLastError(ERROR_INVALID_PARAMETER);
        return FALSE;
    }

    DWORD start = GetTickCount();

    for (;;) {
        int changed = compare_bytes((const void *)Address, CompareAddress, AddressSize);
        if (changed < 0) {
            SetLastError(ERROR_INVALID_PARAMETER);
            return FALSE;
        }
        if (changed) return TRUE;

        if (dwMilliseconds != INFINITE) {
            DWORD elapsed = GetTickCount() - start;
            if (elapsed >= dwMilliseconds) {
                SetLastError(ERROR_TIMEOUT);
                return FALSE;
            }
        }
        Sleep(1);
    }
}

VOID WINAPI WakeByAddressSingle(PVOID Address)
{
    (void)Address;
}

VOID WINAPI WakeByAddressAll(PVOID Address)
{
    (void)Address;
}

// ─── DllMain ───────────────────────────────────────────────────

BOOL WINAPI DllMain(HINSTANCE hinst, DWORD reason, LPVOID reserved)
{
    (void)reserved;
    if (reason == DLL_PROCESS_ATTACH) {
        DisableThreadLibraryCalls(hinst);
    }
    return TRUE;
}
