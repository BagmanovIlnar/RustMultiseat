# KMDF control + filter stubs (`sys/`)

Skeleton only — **does not build** without the Windows Driver Kit (WDK) on a
lab machine. Source mirrors the user-mode ABI in `../include/rms_if_ioctl.h`.

## Layout

| File | Role |
|------|------|
| `RustMultiseatInputFilter.sln` | VS solution (x64 / ARM64) |
| `RustMultiseatInputFilter.vcxproj` | KMDF project; `TargetName=RustMultiseatInputFilter` |
| `driver.c` | `DriverEntry` → control device once; `EvtDeviceAdd` = filter only |
| `control.c` | Software control device + user-mode IOCTL queue |
| `class_filter.c` / `.h` | `on_connect` / `on_input` + `InstanceId` capture |
| `rms_if_ctx.h` | Shared fail-open + route table + stats (control device) |
| `sources` | Legacy WDK `build.exe` placeholder |

Binary basename **must** stay `RustMultiseatInputFilter.sys`.

## Architecture

```text
DriverEntry
  └─ RmsIfCreateControlDevice  →  \\Device\\RustMultiseatInputFilter
                                  \\DosDevices\\RustMultiseatInputFilter
                                  (IOCTL only; one symlink, no kb/mou race)

EvtDeviceAdd (per UpperFilters stack)
  └─ WdfFdoInitSetFilter + RMS_IF_CLASS_CONTEXT
     CONNECT → RmsIfOnConnect + RmsIfCaptureInstanceId(DevicePropertyInstanceId)
     input   → RmsIfOnInput / RmsIfOnPointerInput (DISPATCH_LEVEL)
```

## Class-filter contract

| Hook | Behavior |
|------|----------|
| `RmsIfOnConnect` | Save `ClassService`; install filter callback |
| `RmsIfCaptureInstanceId` | ASCII-upper instance id for SET_ROUTES match |
| `RmsIfOnInput` | fail-open / no match → original; `SUPPRESS_HOST` match → drop |
| `RmsIfOnDisconnect` | Clear hook + InstanceId |

Default: **fail-open = 1** (never suppress until user-mode arms routes).

## Build (Windows lab + WDK)

```text
msbuild RustMultiseatInputFilter.vcxproj /p:Configuration=Release /p:Platform=x64
copy /Y <OutDir>\RustMultiseatInputFilter.sys ..\inf\
```

CI without WDK: `scripts/verify_input_filter_package.sh`.
