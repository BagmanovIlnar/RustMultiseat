# Input-filter INF / catalog (lab)

Skeleton packages for Stage 0 / Stage 7 planning. **Not signed. Not production.**

| File | SCM service | Class |
|------|-------------|-------|
| `RustMultiseatKbFilter.inf` | `RustMultiseatKbFilter` | Keyboard |
| `RustMultiseatMouFilter.inf` | `RustMultiseatMouFilter` | Mouse |

Both copy the same binary `RustMultiseatInputFilter.sys` (WDK build via
`../sys/RustMultiseatInputFilter.vcxproj`). Control device symlink:
`\\.\RustMultiseatInputFilter` (see `../INTERFACE.md`).

## Lab install (test-signing machine only)

```text
# 1) Build .sys (VS Developer / WDK prompt)
msbuild ..\sys\RustMultiseatInputFilter.vcxproj /p:Configuration=Release /p:Platform=x64
copy /Y <OutDir>\RustMultiseatInputFilter.sys .

# 2) Catalog + test-sign (never on production Secure Boot hosts)
bcdedit /set testsigning on   # reboot once
inf2cat /driver:. /os:10_X64
Signtool sign /v /s PrivateCertStore /n <lab-cert> *.cat RustMultiseatInputFilter.sys

# 3) Install
pnputil /add-driver RustMultiseatKbFilter.inf /install
pnputil /add-driver RustMultiseatMouFilter.inf /install
```

Then: `multiseat-ctl input-probe` / `input-filter-status` / soft or native apply.

## Catalog placeholders

| Catalog | Produced by |
|---------|-------------|
| `RustMultiseatKbFilter.cat` | `inf2cat` on kb INF + `.sys` |
| `RustMultiseatMouFilter.cat` | `inf2cat` on mou INF + `.sys` |

`.cat` files are **not** committed — regenerate per lab build.

## Rollback

1. `pnputil /delete-driver <oem##.inf> /uninstall`
2. Remove UpperFilters value if orphaned
3. Prefer restore point / WinRE notes from `InstallPlan` recovery section
