/* Copyright (c) RustMultiseat contributors.
 * DriverEntry / filter device-add for the input-filter KMDF PoC.
 *
 * Control device (software) is created once in DriverEntry.
 * EvtDeviceAdd only attaches as an upper filter on kb/mou stacks — no symlink.
 */
#include "class_filter.h"

#ifdef _KERNEL_MODE

DRIVER_INITIALIZE DriverEntry;
EVT_WDF_DRIVER_DEVICE_ADD RmsIfEvtDeviceAdd;

NTSTATUS
DriverEntry(_In_ PDRIVER_OBJECT DriverObject, _In_ PUNICODE_STRING RegistryPath)
{
  WDF_DRIVER_CONFIG config;
  WDFDRIVER driver;
  NTSTATUS status;

  WDF_DRIVER_CONFIG_INIT(&config, RmsIfEvtDeviceAdd);
  status = WdfDriverCreate(DriverObject, RegistryPath, WDF_NO_OBJECT_ATTRIBUTES,
                           &config, &driver);
  if (!NT_SUCCESS(status)) {
    return status;
  }

  /* Single control device for user-mode IOCTL (not tied to filter instances). */
  status = RmsIfCreateControlDevice(driver);
  if (!NT_SUCCESS(status)) {
    return status;
  }

  return STATUS_SUCCESS;
}

NTSTATUS
RmsIfEvtDeviceAdd(_In_ WDFDRIVER Driver, _Inout_ PWDFDEVICE_INIT DeviceInit)
{
  NTSTATUS status;
  WDFDEVICE device;
  WDF_OBJECT_ATTRIBUTES attrs;
  WDF_IO_QUEUE_CONFIG queueConfig;
  RMS_IF_CLASS_CONTEXT *classCtx;

  UNREFERENCED_PARAMETER(Driver);

  /* Upper filter on keyboard/mouse class stacks (INF UpperFilters). */
  WdfFdoInitSetFilter(DeviceInit);

  /* Filter devices carry class context only — shared state is on control device. */
  WDF_OBJECT_ATTRIBUTES_INIT_CONTEXT_TYPE(&attrs, RMS_IF_CLASS_CONTEXT);
  status = WdfDeviceCreate(&DeviceInit, &attrs, &device);
  if (!NT_SUCCESS(status)) {
    return status;
  }

  classCtx = RmsIfGetClassContext(device);
  RtlZeroMemory(classCtx, sizeof(*classCtx));
  classCtx->Role = RmsIfRoleKeyboard; /* refined on CONNECT */
  classCtx->Shared = RmsIfGetSharedContext();

  WDF_IO_QUEUE_CONFIG_INIT_DEFAULT_QUEUE(&queueConfig, WdfIoQueueDispatchParallel);
  queueConfig.EvtIoInternalDeviceControl = RmsIfEvtIoInternalDeviceControl;
  /* No EvtIoDeviceControl on filter stacks — user-mode talks to control device. */
  status = WdfIoQueueCreate(device, &queueConfig, WDF_NO_OBJECT_ATTRIBUTES, WDF_NO_HANDLE);
  return status;
}

#endif /* _KERNEL_MODE */
