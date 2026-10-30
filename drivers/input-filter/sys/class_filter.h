/* Copyright (c) RustMultiseat contributors.
 * Kbfiltr / Moufiltr-style class connect + service callback stubs.
 *
 * Contract (agent.md):
 *   on_connect — save original ClassService; install RmsIfOnInput
 *   on_input   — route or pass-through; never block / allocate at wrong IRQL
 *
 * Compiles only with WDK (_KERNEL_MODE). Review-friendly without a lab PC.
 */
#pragma once

#include "rms_if_ctx.h"

#ifdef _KERNEL_MODE

#include <ntddkbd.h>
#include <ntddmou.h>
#include <kbdmou.h>

/** Which class stack this filter instance is attached to. */
typedef enum _RMS_IF_FILTER_ROLE {
  RmsIfRoleKeyboard = 1,
  RmsIfRolePointer = 2,
} RMS_IF_FILTER_ROLE;

/**
 * Per-filter-device extension (one instance per kb/mou PDO stack).
 * Shared routes / fail-open live on the software control device
 * (`RmsIfCreateControlDevice` → `RmsIfGetSharedContext`).
 */
typedef struct _RMS_IF_CLASS_CONTEXT {
  RMS_IF_FILTER_ROLE Role;
  /** Cached CONNECT_DATA.ClassDeviceObject from the class driver. */
  PDEVICE_OBJECT ClassDeviceObject;
  /** Original CONNECT_DATA.ClassService — must be called for pass-through. */
  PSERVICE_CALLBACK_ROUTINE ClassService;
  /** True after successful IOCTL_INTERNAL_*_CONNECT hookup. */
  BOOLEAN Connected;
  /**
   * UTF-8 instance id captured at connect (best-effort; may be empty until
   * lab PoC fills it from the device node). Used for SET_ROUTES matching.
   */
  char InstanceId[RMS_IF_INSTANCE_ID_LEN];
  /** Back-pointer to shared filter state (fail-open + routes + stats). */
  RMS_IF_DEVICE_CONTEXT *Shared;
} RMS_IF_CLASS_CONTEXT;

WDF_DECLARE_CONTEXT_TYPE_WITH_NAME(RMS_IF_CLASS_CONTEXT, RmsIfGetClassContext)

/** Publish shared fail-open / routes / stats for class callbacks. */
VOID RmsIfSetSharedContext(_In_opt_ RMS_IF_DEVICE_CONTEXT *Shared);
/** Current shared context (may be NULL before control device exists). */
RMS_IF_DEVICE_CONTEXT *RmsIfGetSharedContext(VOID);

/**
 * Copy DevicePropertyInstanceId into UTF-8 ASCII-upper buffer (user-mode
 * `DeviceId::from_instance` compatible). Empty on failure → never suppress.
 */
NTSTATUS
RmsIfCaptureInstanceId(
    _In_ WDFDEVICE Device,
    _Out_writes_(RMS_IF_INSTANCE_ID_LEN) char *Out);

/** Hook CONNECT: save ClassService, replace with RmsIfOnInput. */
NTSTATUS
RmsIfOnConnect(
    _Inout_ RMS_IF_CLASS_CONTEXT *ClassCtx,
    _Inout_ PCONNECT_DATA ConnectData);

/** Restore ClassService on DISCONNECT (fail-open friendly). */
VOID
RmsIfOnDisconnect(_Inout_ RMS_IF_CLASS_CONTEXT *ClassCtx);

/**
 * Class service callback (DISPATCH_LEVEL).
 * Fail-open or unmatched → call original. SUPPRESS_HOST match → drop + stats.
 */
VOID
RmsIfOnInput(
    _In_ PDEVICE_OBJECT DeviceObject,
    _In_ PKEYBOARD_INPUT_DATA InputDataStart,
    _In_ PKEYBOARD_INPUT_DATA InputDataEnd,
    _Inout_ PULONG InputDataConsumed);

/**
 * Pointer variant (same IRQL rules; Moufiltr CONNECT_DATA).
 * Keyboard and pointer share the suppress decision via Shared + InstanceId.
 */
VOID
RmsIfOnPointerInput(
    _In_ PDEVICE_OBJECT DeviceObject,
    _In_ PMOUSE_INPUT_DATA InputDataStart,
    _In_ PMOUSE_INPUT_DATA InputDataEnd,
    _Inout_ PULONG InputDataConsumed);

/** Internal IOCTL dispatch stub for CONNECT / DISCONNECT. */
EVT_WDF_IO_QUEUE_IO_INTERNAL_DEVICE_CONTROL RmsIfEvtIoInternalDeviceControl;

#endif /* _KERNEL_MODE */
