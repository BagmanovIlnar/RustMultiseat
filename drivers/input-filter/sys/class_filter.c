/* Copyright (c) RustMultiseat contributors.
 * Class upper-filter callbacks (Kbfiltr / Moufiltr pattern) — Stage 0 stubs.
 *
 * Real USB HID vs PS/2 stacks differ; this PoC defines hook points and
 * fail-open-safe pass-through. Full instance-id → SET_ROUTES matching and
 * WDF device-map lookup land with the lab WDK build.
 */
#include "class_filter.h"

#ifdef _KERNEL_MODE

/* Shared control state until per-device association is fully wired. */
static RMS_IF_DEVICE_CONTEXT *g_RmsIfShared = NULL;

VOID
RmsIfSetSharedContext(_In_opt_ RMS_IF_DEVICE_CONTEXT *Shared)
{
  g_RmsIfShared = Shared;
}

RMS_IF_DEVICE_CONTEXT *
RmsIfGetSharedContext(VOID)
{
  return g_RmsIfShared;
}

static BOOLEAN
RmsIfInstanceMatches(
    _In_reads_(RMS_IF_INSTANCE_ID_LEN) const char *RouteId,
    _In_reads_(RMS_IF_INSTANCE_ID_LEN) const char *DeviceId)
{
  SIZE_T n;

  if (DeviceId[0] == '\0') {
    return FALSE;
  }
  if (RtlCompareMemory(RouteId, DeviceId, RMS_IF_INSTANCE_ID_LEN) ==
      RMS_IF_INSTANCE_ID_LEN) {
    return TRUE;
  }
  n = 0;
  while (n < RMS_IF_INSTANCE_ID_LEN && DeviceId[n] != '\0') {
    ++n;
  }
  if (n == 0) {
    return FALSE;
  }
  if (RtlCompareMemory(RouteId, DeviceId, n) != n) {
    return FALSE;
  }
  return (RouteId[n] == '\0') || (n == RMS_IF_INSTANCE_ID_LEN);
}

static BOOLEAN
RmsIfShouldSuppress(_In_ const RMS_IF_CLASS_CONTEXT *ClassCtx)
{
  RMS_IF_DEVICE_CONTEXT *shared;
  UINT32 i;
  UINT32 want;

  if (ClassCtx == NULL || ClassCtx->Shared == NULL) {
    return FALSE;
  }
  shared = ClassCtx->Shared;

  /* Default safe state: never suppress. */
  if (InterlockedCompareExchange(&shared->FailOpen, 1, 1) == 1) {
    return FALSE;
  }

  want = (ClassCtx->Role == RmsIfRoleKeyboard) ? RMS_IF_ROUTE_F_KEYBOARD
                                               : RMS_IF_ROUTE_F_POINTER;
  if (ClassCtx->InstanceId[0] == '\0') {
    return FALSE; /* unmatched → pass-through */
  }

  for (i = 0; i < shared->Routes.EntryCount && i < RMS_IF_MAX_ROUTES; ++i) {
    const RMS_IF_ROUTE_ENTRY *e = &shared->Routes.Entries[i];
    if ((e->Flags & want) == 0) {
      continue;
    }
    if ((e->Flags & RMS_IF_ROUTE_F_SUPPRESS_HOST) == 0) {
      continue;
    }
    if (RmsIfInstanceMatches(e->InstanceId, ClassCtx->InstanceId)) {
      return TRUE;
    }
  }
  return FALSE;
}

static VOID
RmsIfBumpSeen(_In_opt_ RMS_IF_DEVICE_CONTEXT *Shared)
{
  if (Shared != NULL) {
    InterlockedIncrement64((LONG64 *)&Shared->Stats.PacketsSeen);
  }
}

static VOID
RmsIfBumpPassed(_In_opt_ RMS_IF_DEVICE_CONTEXT *Shared)
{
  if (Shared != NULL) {
    InterlockedIncrement64((LONG64 *)&Shared->Stats.PacketsPassed);
  }
}

static VOID
RmsIfBumpSuppressed(_In_opt_ RMS_IF_DEVICE_CONTEXT *Shared)
{
  if (Shared != NULL) {
    InterlockedIncrement64((LONG64 *)&Shared->Stats.PacketsSuppressed);
  }
}

NTSTATUS
RmsIfCaptureInstanceId(
    _In_ WDFDEVICE Device,
    _Out_writes_(RMS_IF_INSTANCE_ID_LEN) char *Out)
{
  WDFMEMORY mem = NULL;
  NTSTATUS status;
  PVOID buf;
  size_t byteLen = 0;
  size_t i;
  const WCHAR *w;

  RtlZeroMemory(Out, RMS_IF_INSTANCE_ID_LEN);

  status = WdfDeviceAllocAndQueryProperty(
      Device,
      DevicePropertyInstanceId,
      NonPagedPoolNx,
      WDF_NO_OBJECT_ATTRIBUTES,
      &mem);
  if (!NT_SUCCESS(status)) {
    return status;
  }

  buf = WdfMemoryGetBuffer(mem, &byteLen);
  if (buf == NULL || byteLen < sizeof(WCHAR)) {
    WdfObjectDelete(mem);
    return STATUS_INVALID_DEVICE_STATE;
  }

  w = (const WCHAR *)buf;
  /* byteLen includes trailing NUL; copy ASCII-upper into fixed UTF-8 pad. */
  for (i = 0; i < RMS_IF_INSTANCE_ID_LEN && (i * sizeof(WCHAR)) < byteLen; ++i) {
    WCHAR c = w[i];
    if (c == L'\0') {
      break;
    }
    if (c >= L'a' && c <= L'z') {
      c = (WCHAR)(c - L'a' + L'A');
    }
    if (c < 0x80) {
      Out[i] = (char)c;
    } else {
      Out[i] = '?';
    }
  }

  WdfObjectDelete(mem);
  return STATUS_SUCCESS;
}

NTSTATUS
RmsIfOnConnect(
    _Inout_ RMS_IF_CLASS_CONTEXT *ClassCtx,
    _Inout_ PCONNECT_DATA ConnectData)
{
  if (ClassCtx == NULL || ConnectData == NULL) {
    return STATUS_INVALID_PARAMETER;
  }
  if (ClassCtx->Connected) {
    return STATUS_SHARING_VIOLATION;
  }

  ClassCtx->ClassDeviceObject = ConnectData->ClassDeviceObject;
  ClassCtx->ClassService = ConnectData->ClassService;
  ClassCtx->Connected = TRUE;
  if (ClassCtx->Shared == NULL) {
    ClassCtx->Shared = g_RmsIfShared;
  }

  /*
   * Install our service callback. Class driver invokes it at DISPATCH_LEVEL
   * with (ClassDeviceObject, InputDataStart, InputDataEnd, Consumed).
   */
  if (ClassCtx->Role == RmsIfRolePointer) {
    ConnectData->ClassService =
        (PSERVICE_CALLBACK_ROUTINE)(ULONG_PTR)RmsIfOnPointerInput;
  } else {
    ConnectData->ClassService =
        (PSERVICE_CALLBACK_ROUTINE)(ULONG_PTR)RmsIfOnInput;
  }

  return STATUS_SUCCESS;
}

VOID
RmsIfOnDisconnect(_Inout_ RMS_IF_CLASS_CONTEXT *ClassCtx)
{
  if (ClassCtx == NULL) {
    return;
  }
  ClassCtx->Connected = FALSE;
  ClassCtx->ClassService = NULL;
  ClassCtx->ClassDeviceObject = NULL;
  RtlZeroMemory(ClassCtx->InstanceId, sizeof(ClassCtx->InstanceId));
}

VOID
RmsIfOnInput(
    _In_ PDEVICE_OBJECT DeviceObject,
    _In_ PKEYBOARD_INPUT_DATA InputDataStart,
    _In_ PKEYBOARD_INPUT_DATA InputDataEnd,
    _Inout_ PULONG InputDataConsumed)
{
  WDFDEVICE hDevice;
  RMS_IF_CLASS_CONTEXT *ctx;
  PSERVICE_CALLBACK_ROUTINE original;
  ULONG count;

  count = (ULONG)(InputDataEnd - InputDataStart);

  /*
   * Kbfiltr pattern: DeviceObject is the filter DO; map to WDFDEVICE + context.
   * If mapping fails → consume without suppress (host must stay usable).
   */
  hDevice = WdfWdmDeviceGetWdfDeviceHandle(DeviceObject);
  if (hDevice == NULL) {
    if (InputDataConsumed) {
      *InputDataConsumed = count;
    }
    RmsIfBumpSeen(g_RmsIfShared);
    RmsIfBumpPassed(g_RmsIfShared);
    return;
  }

  ctx = RmsIfGetClassContext(hDevice);
  RmsIfBumpSeen(ctx->Shared ? ctx->Shared : g_RmsIfShared);

  if (!ctx->Connected || ctx->ClassService == NULL) {
    if (InputDataConsumed) {
      *InputDataConsumed = count;
    }
    RmsIfBumpPassed(ctx->Shared);
    return;
  }

  if (RmsIfShouldSuppress(ctx)) {
    if (InputDataConsumed) {
      *InputDataConsumed = count;
    }
    RmsIfBumpSuppressed(ctx->Shared);
    return;
  }

  original = ctx->ClassService;
  (*(PSERVICE_CALLBACK_ROUTINE)(ULONG_PTR)original)(
      ctx->ClassDeviceObject, InputDataStart, InputDataEnd, InputDataConsumed);
  RmsIfBumpPassed(ctx->Shared);
}

VOID
RmsIfOnPointerInput(
    _In_ PDEVICE_OBJECT DeviceObject,
    _In_ PMOUSE_INPUT_DATA InputDataStart,
    _In_ PMOUSE_INPUT_DATA InputDataEnd,
    _Inout_ PULONG InputDataConsumed)
{
  WDFDEVICE hDevice;
  RMS_IF_CLASS_CONTEXT *ctx;
  PSERVICE_CALLBACK_ROUTINE original;
  ULONG count;

  count = (ULONG)(InputDataEnd - InputDataStart);
  hDevice = WdfWdmDeviceGetWdfDeviceHandle(DeviceObject);
  if (hDevice == NULL) {
    if (InputDataConsumed) {
      *InputDataConsumed = count;
    }
    RmsIfBumpSeen(g_RmsIfShared);
    RmsIfBumpPassed(g_RmsIfShared);
    return;
  }

  ctx = RmsIfGetClassContext(hDevice);
  RmsIfBumpSeen(ctx->Shared ? ctx->Shared : g_RmsIfShared);

  if (!ctx->Connected || ctx->ClassService == NULL) {
    if (InputDataConsumed) {
      *InputDataConsumed = count;
    }
    RmsIfBumpPassed(ctx->Shared);
    return;
  }

  if (RmsIfShouldSuppress(ctx)) {
    if (InputDataConsumed) {
      *InputDataConsumed = count;
    }
    RmsIfBumpSuppressed(ctx->Shared);
    return;
  }

  original = ctx->ClassService;
  (*(PSERVICE_CALLBACK_ROUTINE)(ULONG_PTR)original)(
      ctx->ClassDeviceObject, InputDataStart, InputDataEnd, InputDataConsumed);
  RmsIfBumpPassed(ctx->Shared);
}

VOID
RmsIfEvtIoInternalDeviceControl(
    _In_ WDFQUEUE Queue,
    _In_ WDFREQUEST Request,
    _In_ size_t OutputBufferLength,
    _In_ size_t InputBufferLength,
    _In_ ULONG IoControlCode)
{
  WDFDEVICE device = WdfIoQueueGetDevice(Queue);
  RMS_IF_CLASS_CONTEXT *classCtx = RmsIfGetClassContext(device);
  NTSTATUS status = STATUS_NOT_SUPPORTED;

  UNREFERENCED_PARAMETER(OutputBufferLength);
  UNREFERENCED_PARAMETER(InputBufferLength);

  switch (IoControlCode) {
  case IOCTL_INTERNAL_KEYBOARD_CONNECT:
  case IOCTL_INTERNAL_MOUSE_CONNECT: {
    PCONNECT_DATA connect = NULL;
    status = WdfRequestRetrieveInputBuffer(
        Request, sizeof(CONNECT_DATA), (PVOID *)&connect, NULL);
    if (!NT_SUCCESS(status)) {
      break;
    }
    classCtx->Role = (IoControlCode == IOCTL_INTERNAL_MOUSE_CONNECT)
                         ? RmsIfRolePointer
                         : RmsIfRoleKeyboard;
    classCtx->Shared = g_RmsIfShared;
    status = RmsIfOnConnect(classCtx, connect);
    if (NT_SUCCESS(status)) {
      /* Best-effort; empty InstanceId ⇒ never SUPPRESS_HOST (safe). */
      (void)RmsIfCaptureInstanceId(device, classCtx->InstanceId);
    }
    WdfRequestComplete(Request, status);
    return;
  }
  case IOCTL_INTERNAL_KEYBOARD_DISCONNECT:
  case IOCTL_INTERNAL_MOUSE_DISCONNECT:
    RmsIfOnDisconnect(classCtx);
    WdfRequestComplete(Request, STATUS_SUCCESS);
    return;
  default: {
    /* Forward unrelated internal IOCTLs down the stack. */
    WDFIOTARGET target = WdfDeviceGetIoTarget(device);
    WDF_REQUEST_SEND_OPTIONS options;
    WDF_REQUEST_SEND_OPTIONS_INIT(&options, WDF_REQUEST_SEND_OPTION_SEND_AND_FORGET);
    if (!WdfRequestSend(Request, target, &options)) {
      status = WdfRequestGetStatus(Request);
      WdfRequestComplete(Request, status);
    }
    return;
  }
  }

  WdfRequestComplete(Request, status);
}

#endif /* _KERNEL_MODE */
