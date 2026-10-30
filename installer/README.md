# Stage 7 — Installer & signing (scaffold)

This directory holds **install/signing process artifacts**. Kernel drivers and
MSI/WiX packages are not shipped yet; Stage 0 must go first.

## Generate a host plan

```bash
cargo run -p multiseat-daemon --bin multiseat-ctl -- install-plan \
  --out installer/artifacts/latest

# Lab / CI matrix without a Windows host:
cargo run -p multiseat-daemon --bin multiseat-ctl -- install-plan \
  --pretend-version 10.0.22621 --out installer/artifacts/win11
cargo run -p multiseat-daemon --bin multiseat-ctl -- install-plan \
  --pretend-version 6.1.7601 --out installer/artifacts/win7
```

Outputs:

- `install-plan.json` — machine-readable package + signing matrix
- `INSTALL_PLAN.md` — operator checklist

## Payload layout (target)

| Path | Component |
|------|-----------|
| `Program Files\RustMultiseat\` | `multiseat-daemon.exe`, `multiseat-ctl.exe`, `multiseat-gui.exe`, `multiseat-session-agent.exe` |
| SCM | Service `RustMultiseat` (`multiseat-daemon --install`) |
| Named Pipe | `\\.\pipe\RustMultiseatDaemon` |
| Driver store | packages from `drivers/` selected by `OsGeneration` |

## Signing rules (non-negotiable)

- **Win10/11:** attestation / WHCP for kernel packages; Authenticode for user-mode.
- **Win7/8/8.1:** legacy WHCP/HCK — attestation does **not** replace legacy certification.
- Test-signing / test certificates: **lab only**; Secure Boot stays **on** for production evidence.
- Re-read current Microsoft driver signing docs before every release.

See `signing-checklist.md`.

## Recovery

1. Create a restore point (when applicable) before installing kernel drivers.
2. Keep WinRE / offline uninstall notes beside each driver package.
3. Uninstall order: display → input filter → SCM service → user-mode files.
4. Fail-open: service/agent crash must restore host input (already covered by daemon watchdog).

## Related commands

- `multiseat-daemon --install` / `--uninstall` — SCM registration (Windows).
- `multiseat-ctl lab-checklist` — Stage 0 go/no-go evidence.
- `multiseat-ctl native-status` — soft PoC readiness.
