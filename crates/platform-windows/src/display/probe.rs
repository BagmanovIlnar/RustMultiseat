//! OS-family → display-model candidate mapping for Stage 0.

use multiseat_core::DisplayProbeResult;

/// Lab override: force a candidate backend name (`iddcx`, `wddm-legacy`, …).
pub const DISPLAY_BACKEND_ENV: &str = "MULTISEAT_DISPLAY_BACKEND";
/// Lab override: claim attach is implemented (`1`) — still needs real PoC proof.
pub const DISPLAY_ATTACH_ENV: &str = "MULTISEAT_DISPLAY_ATTACH";

/// Probes which display backend candidate applies and whether attach is ready.
///
/// `attach_supported` stays false until a real IddCx/legacy driver package is
/// installed and Stage 0 proves session binding — IddCx alone is insufficient.
pub fn probe_display(physical_outputs: usize) -> DisplayProbeResult {
    let (backend_name, attach_supported) = detect_candidate();
    DisplayProbeResult::evaluate(backend_name, attach_supported, physical_outputs)
}

/// Maps Windows version numbers to a display-model candidate.
///
/// ```
/// use multiseat_platform_windows::display_candidate_for_os_version;
///
/// assert_eq!(display_candidate_for_os_version(10, 0, 19041), ("iddcx", false));
/// assert_eq!(display_candidate_for_os_version(6, 1, 7601), ("win7-legacy", false));
/// assert_eq!(display_candidate_for_os_version(5, 1, 2600), ("none", false));
/// ```
pub fn display_candidate_for_os_version(
    major: u32,
    minor: u32,
    build: u32,
) -> (&'static str, bool) {
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
    let _ = build;
    ("none", false)
}

fn detect_candidate() -> (String, bool) {
    if let Ok(name) = std::env::var(DISPLAY_BACKEND_ENV) {
        if !name.is_empty() {
            let attach = std::env::var_os(DISPLAY_ATTACH_ENV).is_some_and(|v| v == "1");
            return (name, attach);
        }
    }

    #[cfg(windows)]
    {
        use crate::probe::windows_version_numbers;

        match windows_version_numbers() {
            Some((major, minor, build)) => {
                let (name, attach) = display_candidate_for_os_version(major, minor, build);
                (name.to_string(), attach)
            }
            None => ("none".into(), false),
        }
    }

    #[cfg(not(windows))]
    {
        ("none".into(), false)
    }
}

#[cfg(test)]
mod candidate_tests {
    use super::*;

    #[test]
    fn maps_known_windows_families() {
        assert_eq!(
            display_candidate_for_os_version(10, 0, 14393),
            ("iddcx", false)
        );
        assert_eq!(
            display_candidate_for_os_version(10, 0, 22621),
            ("iddcx", false)
        );
        assert_eq!(
            display_candidate_for_os_version(6, 3, 9600),
            ("wddm-legacy", false)
        );
        assert_eq!(
            display_candidate_for_os_version(6, 2, 9200),
            ("wddm-legacy", false)
        );
        assert_eq!(
            display_candidate_for_os_version(6, 1, 7601),
            ("win7-legacy", false)
        );
        assert_eq!(
            display_candidate_for_os_version(6, 0, 6001),
            ("none", false)
        );
    }
}
