/* Copyright (c) RustMultiseat contributors.
 * Shared filter state + control-device helpers (KMDF PoC stub).
 * Requires WDK headers to compile; kept for ABI / review without a lab PC.
 */
#pragma once

#include "../include/rms_if_ioctl.h"

#ifdef _KERNEL_MODE
#include <ntddk.h>
#include <wdf.h>
#endif

/** In-memory filter state on the **control** device only (IOCTL + routes). */
typedef struct _RMS_IF_DEVICE_CONTEXT {
  /** 1 = pass-through (default); 0 = routes armed. */
  volatile LONG FailOpen;
  /** Last validated SET_ROUTES payload (replace-all). */
  RMS_IF_ROUTE_TABLE Routes;
  /** Counters exposed by QUERY_STATS (no scan codes / text). */
  RMS_IF_STATS Stats;
  /** Control WDFDEVICE handle (for lifetime diagnostics). */
#ifdef _KERNEL_MODE
  WDFDEVICE ControlDevice;
#endif
} RMS_IF_DEVICE_CONTEXT;

#ifdef _KERNEL_MODE
WDF_DECLARE_CONTEXT_TYPE_WITH_NAME(RMS_IF_DEVICE_CONTEXT, RmsIfGetContext)

/** Create software control device + \\DosDevices\\RustMultiseatInputFilter once. */
NTSTATUS
RmsIfCreateControlDevice(_In_ WDFDRIVER Driver);

/** User-mode IOCTL dispatch (control device queue only). */
EVT_WDF_IO_QUEUE_IO_DEVICE_CONTROL RmsIfControlEvtIoDeviceControl;
#endif

/** Caps: bit0 fail-open default, bit1 routes supported. */
#define RMS_IF_CAPS_FAIL_OPEN_DEFAULT 0x1u
#define RMS_IF_CAPS_ROUTES 0x2u
