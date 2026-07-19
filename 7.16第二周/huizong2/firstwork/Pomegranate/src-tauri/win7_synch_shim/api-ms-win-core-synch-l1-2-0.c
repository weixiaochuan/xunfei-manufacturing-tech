// api-ms-win-core-synch-l1-2-0.dll shim for Windows 7
// Replaces the Win8+ API Set DLL with local implementations.
// Sleep/SleepEx forwarded to kernel32 via .def (NO dllimport needed).
// WaitOnAddress/WakeByAddress stubbed locally.
// ALL dependencies resolved via GetProcAddress - zero API Set imports.

#define WINAPI __stdcall
#define INFINITE 0xFFFFFFFF
#define DLL_PROCESS_ATTACH 1

typedef void *PVOID;
typedef void *LPVOID;
typedef unsigned long DWORD;
typedef unsigned long long SIZE_T;
typedef int BOOL;
typedef PVOID HINSTANCE;
typedef PVOID HMODULE;

__declspec(dllimport) PVOID WINAPI GetProcAddress(PVOID hModule, const char *lpProcName);
__declspec(dllimport) HMODULE WINAPI GetModuleHandleA(const char *lpModuleName);
__declspec(dllimport) BOOL WINAPI DisableThreadLibraryCalls(HINSTANCE hinstDLL);

static HMODULE get_k32(void) { return GetModuleHandleA("kernel32.dll"); }

// ─── Sleep/SleepEx ─────────────────────────────────
// Exported via exports.def as forwarders: Sleep=kernel32.Sleep
// No local dllimport - forwarding is purely at the .def/linker level.
// Internal stubs for when the .def forwarder is not sufficient.

void WINAPI Sleep(DWORD dwMilliseconds)
{
    HMODULE k32 = get_k32();
    if (k32) {
        typedef void (WINAPI *Fn)(DWORD);
        Fn fn = (Fn)GetProcAddress(k32, "Sleep");
        if (fn) fn(dwMilliseconds);
    }
}

DWORD WINAPI SleepEx(DWORD dwMilliseconds, int bAlertable)
{
    HMODULE k32 = get_k32();
    if (k32) {
        typedef DWORD (WINAPI *Fn)(DWORD, int);
        Fn fn = (Fn)GetProcAddress(k32, "SleepEx");
        if (fn) return fn(dwMilliseconds, bAlertable);
    }
    return 0;
}

// ─── WaitOnAddress (stub) ──────────────────────────
int WINAPI WaitOnAddress(
    void volatile *Address,
    void *CompareAddress,
    SIZE_T AddressSize,
    DWORD dwMilliseconds)
{
    (void)Address;
    (void)CompareAddress;
    (void)AddressSize;

    if (dwMilliseconds == INFINITE) dwMilliseconds = 1000;

    HMODULE k32 = get_k32();
    typedef DWORD (WINAPI *FnGT)(void);
    FnGT pGT = k32 ? (FnGT)GetProcAddress(k32, "GetTickCount") : 0;

    if (pGT) {
        DWORD end = pGT() + dwMilliseconds;
        while (pGT() < end) {
            Sleep(0); // thread yield via our own Sleep export
        }
    }
    return 1;
}

// ─── WakeByAddressSingle (stub) ────────────────────
void WINAPI WakeByAddressSingle(void *Address)
{
    (void)Address;
}

// ─── WakeByAddressAll (stub) ───────────────────────
void WINAPI WakeByAddressAll(void *Address)
{
    (void)Address;
}

// ─── DllMain ──────────────────────────────────────
BOOL WINAPI DllMain(HINSTANCE hinstDLL, DWORD fdwReason, LPVOID lpvReserved)
{
    (void)lpvReserved;
    if (fdwReason == DLL_PROCESS_ATTACH) {
        DisableThreadLibraryCalls(hinstDLL);
    }
    return 1;
}
