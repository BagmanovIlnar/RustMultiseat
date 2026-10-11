//! OS-family → display-model candidate mapping for Stage 0.

use multiseat_core::DisplayProbeResult;

/// Probes which display backend candidate applies and whether attach is ready.
///
/// `attach_supported` stays false until a real IddCx/legacy driver package is
/// installed and Stage 0 proves session binding — IddCx alone is insufficient.
pub fn probe_display(physical_outputs: usize) -> DisplayProbeResult {
    let (backend_name, attach_supported) = detect_candidate();
    DisplayProbeResult::evaluate(backend_name, attach_supported, physical_outputs)
}

fn detect_candidate() -> (&'static str, bool) {
    #[cfg(windows)]
    {
        use crate::probe::windows_version_numbers;

        match windows_version_numbers() {
            Some((major, minor, build)) => {
                // Win10/11 report major=10; IddCx from 1607 (build 14393).
                if major == 10 && build >= 14393 {
                    return ("iddcx", false);
                }
                // Windows 8 / 8.1
                if major == 6 && (minor == 2 || minor == 3) {
                    return ("wddm-legacy", false);
                }
                // Windows 7
                if major == 6 && minor == 1 {
                    return ("win7-legacy", false);
                }
                ("none", false)
            }
            None => ("none", false),
        }
    }

    #[cfg(not(windows))]
    {
        ("none", false)
    }
}
