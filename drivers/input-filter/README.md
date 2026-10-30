# Input filter (KMDF PoC)

Documented starting point: [Kbfiltr](https://learn.microsoft.com/samples/microsoft/windows-driver-samples/keyboard-input-wdf-filter-driver-kbfiltr/)
and Moufiltr patterns. PS/2 and USB HID are **different stacks** — a PS/2 sample
is not a generic HID filter.

## Service names (must match user-mode probe)

| Role | SCM / filter service name |
|------|---------------------------|
| Keyboard | `RustMultiseatKbFilter` |
| Pointer | `RustMultiseatMouFilter` |

`multiseat-ctl input-probe` / `NativePocStatus` look for these names (or
`MULTISEAT_INPUT_FILTER=1` lab override).

## Required behavior (`agent.md`)

| Method | Contract |
|--------|----------|
| `on_connect` | Save original class callback |
| `on_input` | Route or pass-through; never block / allocate at wrong IRQL |
| `set_routes` | Atomic replace of validated route table |
| `set_fail_open` | Immediate host input restore (default safe state) |
| `query_stats` | Counters only — **no** scan codes / text / passwords in release |

Fail-open triggers: heartbeat timeout, service crash, bad config, queue overflow.

## User-mode IOCTL ABI

See `INTERFACE.md` and Rust stub
`multiseat_platform_windows::input_filter_control`.

## Packaging

| Path | Role |
|------|------|
| `sys/RustMultiseatInputFilter.vcxproj` | KMDF project → `RustMultiseatInputFilter.sys` |
| `sys/` | `DriverEntry` + IOCTL stubs (needs WDK to compile) |
| `inf/` | kb/mou INF + lab catalog / pnputil steps |
| `include/rms_if_ioctl.h` | Shared IOCTL ABI |

`InstallPlan` lists these via `DriverPackageId::packaging_artifacts()`.
CI consistency: `scripts/verify_input_filter_package.sh`.

## Build (Windows lab + WDK)

Skeleton under `sys/` (`DriverEntry`, control-device IOCTL dispatch, fail-open
default). **Does not compile without WDK** — see `sys/README.md`.

Still pending for a real PoC:

1. Lab WDK build (`control` software device + class hooks + InstanceId capture sketched).
2. Verify suppress on physical kits against `input-routes-preview` instance ids.
3. `inf2cat` / lab test-sign (see `inf/README.md`).
4. HLK/HCK/WHCP per OS generation before any production ship.
5. Driver Verifier on the Stage 0 lab PCs.

## Security

- No keylogging telemetry in release builds.
- Secure Desktop / CAD / UAC / login path are **out of scope** for filter bypass.
- Prefer fail-open over a stuck suppression table.
