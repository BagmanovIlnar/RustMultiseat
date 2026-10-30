/* Copyright (c) RustMultiseat contributors.
 * Software control device + IOCTL dispatch for RustMultiseatInputFilter.
 *
 * One control device per driver load (not per kb/mou filter instance), so the
 * \\DosDevices\\RustMultiseatInputFilter symlink has no UpperFilters race.
 */
#include "class_filter.h"

#ifdef _KERNEL_MODE

/* SDDL: SYSTEM + Administrators full; Everyone read/write (lab). Tighten for production. */
DECLARE_CONST_UNICODE_STRING(
    RmsIfControlSddl,
    L"D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;GRGW;;;WD)");

DECLARE_CONST_UNICODE_STRING(
    RmsIfControlNtName,
    L"\\Device\\RustMultiseatInputFilter");

DECLARE_CONST_UNICODE_STRING(
    RmsIfControlDosName,
    L"\\DosDevices\\RustMultiseatInputFilter");

NTSTATUS
RmsIfCreateControlDevice(_In_ WDFDRIVER Driver)
{
  NTSTATUS status;
  PWDFDEVICE_INIT init = NULL;
  WDFDEVICE device;
  WDF_OBJECT_ATTRIBUTES attrs;
  WDF_IO_QUEUE_CONFIG queueConfig;
  RMS_IF_DEVICE_CONTEXT *ctx;

  if (RmsIfGetSharedContext() != NULL) {
    return STATUS_SUCCESS; /* already created */
  }

  init = WdfControlDeviceInitAllocate(Driver, &RmsIfControlSddl);
  if (init == NULL) {
    return STATUS_INSUFFICIENT_RESOURCES;
  }

  WdfDeviceInitSetExclusive(init, FALSE);
  status = WdfDeviceInitAssignName(init, &RmsIfControlNtName);
  if (!NT_SUCCESS(status)) {
    WdfDeviceInitFree(init);
    return status;
  }

  WDF_OBJECT_ATTRIBUTES_INIT_CONTEXT_TYPE(&attrs, RMS_IF_DEVICE_CONTEXT);
  status = WdfDeviceCreate(&init, &attrs, &device);
  if (!NT_SUCCESS(status)) {
    /* init is freed by framework on failure after create attempt */
    return status;
  }

  ctx = RmsIfGetContext(device);
  RtlZeroMemory(ctx, sizeof(*ctx));
  InterlockedExchange(&ctx->FailOpen, 1);
  ctx->Stats.FailOpen = 1;
  ctx->ControlDevice = device;
  RmsIfSetSharedContext(ctx);

  status = WdfDeviceCreateSymbolicLink(device, &RmsIfControlDosName);
  if (!NT_SUCCESS(status)) {
    RmsIfSetSharedContext(NULL);
    return status;
  }

  WDF_IO_QUEUE_CONFIG_INIT_DEFAULT_QUEUE(&queueConfig, WdfIoQueueDispatchSequential);
  queueConfig.EvtIoDeviceControl = RmsIfControlEvtIoDeviceControl;
  status = WdfIoQueueCreate(device, &queueConfig, WDF_NO_OBJECT_ATTRIBUTES, WDF_NO_HANDLE);
  if (!NT_SUCCESS(status)) {
    RmsIfSetSharedContext(NULL);
    return status;
  }

  WdfControlFinishInitializing(device);
  return STATUS_SUCCESS;
}

NTSTATUS
RmsIfControlEvtIoDeviceControl(
    _In_ WDFQUEUE Queue,
    _In_ WDFREQUEST Request,
    _In_ size_t OutputBufferLength,
    _In_ size_t InputBufferLength,
    _In_ ULONG IoControlCode)
{
  WDFDEVICE device = WdfIoQueueGetDevice(Queue);
  RMS_IF_DEVICE_CONTEXT *ctx = RmsIfGetContext(device);
  NTSTATUS status = STATUS_INVALID_DEVICE_REQUEST;
  size_t info = 0;

  UNREFERENCED_PARAMETER(OutputBufferLength);
  UNREFERENCED_PARAMETER(InputBufferLength);

  switch (IoControlCode) {
  case IOCTL_RMS_IF_GET_VERSION: {
    PRMS_IF_VERSION_INFO out = NULL;
    status = WdfRequestRetrieveOutputBuffer(
        Request, sizeof(*out), (PVOID *)&out, NULL);
    if (!NT_SUCCESS(status)) {
      break;
    }
    out->AbiVersion = RMS_IF_ABI_VERSION;
    out->DriverBuild = 0;
    out->CapsFlags = RMS_IF_CAPS_FAIL_OPEN_DEFAULT | RMS_IF_CAPS_ROUTES;
    info = sizeof(*out);
    status = STATUS_SUCCESS;
    break;
  }

  case IOCTL_RMS_IF_SET_FAIL_OPEN: {
    PRMS_IF_FAIL_OPEN_REQ in = NULL;
    status = WdfRequestRetrieveInputBuffer(
        Request, sizeof(*in), (PVOID *)&in, NULL);
    if (!NT_SUCCESS(status)) {
      break;
    }
    InterlockedExchange(&ctx->FailOpen, in->Enabled ? 1 : 0);
    ctx->Stats.FailOpen = (UINT32)ctx->FailOpen;
    status = STATUS_SUCCESS;
    break;
  }

  case IOCTL_RMS_IF_SET_ROUTES: {
    PRMS_IF_ROUTE_TABLE in = NULL;
    status = WdfRequestRetrieveInputBuffer(
        Request, sizeof(*in), (PVOID *)&in, NULL);
    if (!NT_SUCCESS(status)) {
      break;
    }
    if (in->AbiVersion != RMS_IF_ABI_VERSION ||
        in->EntryCount > RMS_IF_MAX_ROUTES) {
      status = STATUS_INVALID_PARAMETER;
      break;
    }
    RtlCopyMemory(&ctx->Routes, in, sizeof(*in));
    status = STATUS_SUCCESS;
    break;
  }

  case IOCTL_RMS_IF_QUERY_STATS: {
    PRMS_IF_STATS out = NULL;
    status = WdfRequestRetrieveOutputBuffer(
        Request, sizeof(*out), (PVOID *)&out, NULL);
    if (!NT_SUCCESS(status)) {
      break;
    }
    *out = ctx->Stats;
    out->FailOpen = (UINT32)ctx->FailOpen;
    info = sizeof(*out);
    status = STATUS_SUCCESS;
    break;
  }

  default:
    status = STATUS_INVALID_DEVICE_REQUEST;
    break;
  }

  WdfRequestCompleteWithInformation(Request, status, info);
  return status;
}

#endif /* _KERNEL_MODE */
