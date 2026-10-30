# IddCx display package (Windows 10/11)

Indirect Display Driver Model only. **IddCx alone does not bind a display to a
local interactive session** — Stage 0 must prove session binding separately.

- Candidate selected by `display_candidate_for_os_version` when build ≥ 14393.
- Soft bookkeeping attach: `MULTISEAT_DISPLAY_ATTACH=1` (no real driver IOCTL).
- Production: attestation/WHCP + Driver Verifier + HLK.
