# Drivers (Stage 0 / Stage 4 gated)

Kernel packages live here. **Do not implement suppression or session binding
until Stage 0 proves the corresponding PoC on physical Windows lab hardware.**

| Package | Path | Primary OS | Signing |
|---------|------|------------|---------|
| Input filter | `input-filter/` | all (KMDF PoC) | WHCP/HCK or attestation by OS |
| Virtual input | `virtual-input/` | experimental | same |
| IddCx display | `display/iddcx/` | Win10/11 | attestation/WHCP |
| WDDM legacy | `display/wddm-legacy/` | Win8/8.1 | legacy WHCP/HCK |
| Win7 legacy | `display/win7-legacy/` | Win7 | legacy WHCP/HCK |

User-mode control surfaces (IOCTL stubs, service names) live in
`crates/platform-windows` so CI can compile contracts without the WDK.

Generate the install matrix (lists INF paths for `input-filter`):

```bash
cargo run -p multiseat-daemon --bin multiseat-ctl -- install-plan --pretend-version 10.0.22621 --out artifacts/install-plan
```

Stage 0 checklist also writes `install-plan.json` / `INSTALL_PLAN.md` into its `--out` dir
and runs `scripts/verify_input_filter_package.sh` (INF ↔ `RustMultiseatInputFilter.sys` name).
