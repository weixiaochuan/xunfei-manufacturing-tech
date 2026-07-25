// win7_winrt_shim/winrt_stubs.c
// WinRT initialization stubs for Windows 7 compatibility.
//
// On Windows 7, these functions are not available. We provide minimal
// implementations that return success for initialization and "not found"
// for factory/instance activation.

#include "hstring.h"
#include <windows.h>

// Forward-declare WinRT types (avoid pulling in inspectable.h / restrictererorinfo.h)
// These are opaque pointers — we only pass NULL through them.
#ifndef __IInspectable_FWD_DEFINED__
#define __IInspectable_FWD_DEFINED__
typedef interface IInspectable IInspectable;
#endif

#ifndef __IRestrictedErrorInfo_FWD_DEFINED__
#define __IRestrictedErrorInfo_FWD_DEFINED__
typedef interface IRestrictedErrorInfo IRestrictedErrorInfo;
#endif

// ─── WinRT initialization ─────────────────────────────────────────

HRESULT WINAPI RoInitialize(RO_INIT_TYPE initType)
{
    (void)initType;
    // On Windows 7, COM is managed by the process itself.
    // Tauri already initializes COM (CoInitializeEx) during startup.
    // Return S_OK: the caller believes WinRT is ready.
    // Note: S_FALSE means "already initialized" - we return S_OK
    //       because we don't track state and re-entry should be harmless.
    return S_OK;
}

void WINAPI RoUninitialize(void)
{
    // COM uninitialization is managed by the process.
    // No-op on Windows 7.
}

HRESULT WINAPI RoGetActivationFactory(
    HSTRING  activatableClassId,
    REFIID   iid,
    void   **factory)
{
    (void)activatableClassId;
    (void)iid;
    if (factory) *factory = NULL;
    // WinRT classes don't exist on Windows 7.
    return REGDB_E_CLASSNOTREG; // 0x80040154
}

HRESULT WINAPI RoActivateInstance(
    HSTRING  activatableClassId,
    IInspectable **instance)
{
    (void)activatableClassId;
    if (instance) *instance = NULL;
    return REGDB_E_CLASSNOTREG;
}

HRESULT WINAPI RoGetApartmentIdentifier(
    UINT64 *apartmentIdentifier)
{
    // Return MTA identifier (0)
    if (apartmentIdentifier) *apartmentIdentifier = 0;
    return S_OK;
}

// ─── RoGetErrorReportingFlags / RoSetErrorReportingFlags ──────────
// These are WinRT error reporting functions.
// Stub them out as no-ops.

HRESULT WINAPI RoGetErrorReportingFlags(
    UINT32 *pFlags)
{
    if (pFlags) *pFlags = 0; // RO_ERROR_REPORTING_NONE
    return S_OK;
}

HRESULT WINAPI RoSetErrorReportingFlags(
    UINT32 flags)
{
    (void)flags;
    return S_OK;
}

HRESULT WINAPI RoResolveRestrictedErrorInfoReference(
    PCWSTR reference,
    IRestrictedErrorInfo **info)
{
    (void)reference;
    if (info) *info = NULL;
    return E_NOTIMPL;
}
