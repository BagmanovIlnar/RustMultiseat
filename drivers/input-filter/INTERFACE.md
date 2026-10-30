# Input filter ↔ daemon IOCTL contract (v1)

Versioned control device. All buffers are fixed-size; the driver **must**
reject wrong `InputBufferLength` / `OutputBufferLength`.

## Canonical definitions

| Artifact | Path |
|----------|------|
| C header (WDK / docs) | `include/rms_if_ioctl.h` |
| Rust mirror | `crates/platform-windows/src/input/ioctl.rs` |

Device type `RMS_IF_DEVICE_TYPE = 0x8A50`. Codes use `METHOD_BUFFERED`.

## Device interface

- Symbolic hint: `\\.\RustMultiseatInputFilter` (`RMS_IF_CONTROL_DEVICE_NAME`)
- GUID: TBD with INF (placeholder until package exists)

## IOCTL codes

| Name | Function | Access | Purpose |
|------|----------|--------|---------|
| `IOCTL_RMS_IF_GET_VERSION` | 0x800 | READ | → `RMS_IF_VERSION_INFO` |
| `IOCTL_RMS_IF_SET_FAIL_OPEN` | 0x801 | WRITE | ← `RMS_IF_FAIL_OPEN_REQ` |
| `IOCTL_RMS_IF_SET_ROUTES` | 0x802 | WRITE | ← opaque validated blob |
| `IOCTL_RMS_IF_QUERY_STATS` | 0x803 | READ | → `RMS_IF_STATS` |

Default power-on / surprise-remove / service-down state: **fail-open**.

## SET_ROUTES blob

Fixed `RMS_IF_ROUTE_TABLE` (see header):

- `AbiVersion` must equal `RMS_IF_ABI_VERSION`
- `EntryCount` ∈ `0..=RMS_IF_MAX_ROUTES` (32)
- Each `InstanceId` is UTF-8, NUL-padded to 128 bytes
- Flags: `KEYBOARD` / `POINTER` / `SUPPRESS_HOST`; table `REPLACE_ALL`

User-mode builders:

- `RmsIfRouteTable::from_entries` — low-level rows
- `route_table_from_config` / `route_table_from_assignments` — from `MultiseatConfig`
  (`crates/platform-windows/src/input/routes.rs`)

## Rust control stub

```text
crates/platform-windows/src/input/filter_control.rs
multiseat-ctl input-fail-open
multiseat-ctl input-filter-status
multiseat-ctl input-filter-ping
multiseat-ctl input-routes-preview [--config path.json] [--ping]
multiseat-ctl input-routes-apply [--config path.json] [--dry-run] [--no-arm]
multiseat-ctl input-soft-e2e [--config path.json] [--out DIR]
multiseat-ctl smoke --with-soft-input
```

`input-routes-apply` calls `set_routes` then optionally `SET_FAIL_OPEN(0)`.
Requires present control (`MULTISEAT_INPUT_FILTER=1` soft or native). Prefer
`--no-arm` first on lab kits, then arm after `input-filter-status` looks sane.

`input-soft-e2e` runs preview → apply(--no-arm) → apply(arm) → ping → fail-open and
writes `soft_input_e2e.json` (Stage 0 checklist always runs this with soft env).

Until the KMDF binary exists, `InputFilterControl::open` is absent (or
soft-present with `MULTISEAT_INPUT_FILTER=1`). `set_fail_open` remains safe
no-op success when absent.

## Daemon Active watchdog

While multiseat is Active, `DaemonState` arms `FilterWatchdog` and periodically
calls `filter_heartbeat` (~200ms). Unhealthy pings → `EmergencyFailOpen` + stop
(same fail-open contract as session-agent heartbeat miss).

Lab injection: `MULTISEAT_INPUT_FILTER_HEARTBEAT_FAIL=1` or
`multiseat_daemon::set_inject_unhealthy(true)`.

## Class-filter hooks (`sys/class_filter.c`)

| Hook | IOCTL / callback | Notes |
|------|------------------|-------|
| `RmsIfCreateControlDevice` | DriverEntry | Software PDO + `\\.\RustMultiseatInputFilter` (once) |
| `RmsIfOnConnect` | `IOCTL_INTERNAL_KEYBOARD/MOUSE_CONNECT` | Saves class `ClassService` |
| `RmsIfCaptureInstanceId` | after CONNECT | `DevicePropertyInstanceId` → ASCII-upper pad |
| `RmsIfOnInput` | Class service callback | DISPATCH_LEVEL; suppress only if armed + route match |
| `RmsIfOnDisconnect` | `*_DISCONNECT` | Clears hook + InstanceId |

Filter stacks do **not** own the DOS symlink (avoids kb/mou race). Pass-through
is the default until `SET_FAIL_OPEN(0)` after validated `SET_ROUTES`.
