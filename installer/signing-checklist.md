# Signing & certification checklist

Fill one row per release candidate. Lab test-sign never counts as production.

| Item | Win7 | Win8/8.1 | Win10 | Win11 | Notes |
|------|------|----------|-------|-------|-------|
| Secure Boot ON during evidence | ☐ | ☐ | ☐ | ☐ | Production path |
| User-mode Authenticode (daemon/gui/agent) | ☐ | ☐ | ☐ | ☐ | |
| Input filter HLK/HCK/WHCP | ☐ legacy | ☐ legacy | ☐ | ☐ | |
| Display package HLK/HCK/WHCP | ☐ win7-legacy | ☐ wddm-legacy | ☐ IddCx | ☐ IddCx | |
| Attestation used only where valid | n/a | n/a | ☐ | ☐ | Not a Win7/8 substitute |
| Driver Verifier pass on lab | ☐ | ☐ | ☐ | ☐ | |
| Offline uninstall / WinRE doc attached | ☐ | ☐ | ☐ | ☐ | |
| Microsoft signing docs re-read this release | ☐ | ☐ | ☐ | ☐ | |

## Package selection (from `install-plan`)

- Win10/11 → `user-mode-payload`, `input-filter`, `display-iddcx`
- Win8/8.1 → `user-mode-payload`, `input-filter`, `display-wddm-legacy`
- Win7 → `user-mode-payload`, `input-filter`, `display-win7-legacy`

## Blockers that stop ship

- Stage 0 report is NO-GO
- Any selected kernel package still `Blocked` / `Unknown`
- Test Signing enabled on a “production” evidence machine
