//! Stage 7 install / signing plan models (OS-gated driver packages).
//!
//! Pure planning only — no Win32 install side effects. The daemon/ctl emit a
//! plan; a future MSI/WiX package consumes it after Stage 0 go.

use serde::{Deserialize, Serialize};

use crate::feasibility::CheckStatus;
use crate::hardware::PlatformCapabilities;

/// Coarse Windows generation used to select driver packages and signing flow.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OsGeneration {
    /// Windows 7 (6.1).
    Win7,
    /// Windows 8 / 8.1 (6.2 / 6.3).
    Win8Family,
    /// Windows 10 (10.0, builds before Win11 branding).
    Win10,
    /// Windows 11 (10.0 with build ≥ 22000).
    Win11,
    /// Non-Windows or unrecognized host (dev/CI).
    Other,
}

impl OsGeneration {
    /// Maps version numbers from `GetVersionExW` / lab hints.
    ///
    /// ```
    /// use multiseat_core::OsGeneration;
    ///
    /// assert_eq!(OsGeneration::from_windows_version(6, 1, 7601), OsGeneration::Win7);
    /// assert_eq!(OsGeneration::from_windows_version(10, 0, 19045), OsGeneration::Win10);
    /// assert_eq!(OsGeneration::from_windows_version(10, 0, 22621), OsGeneration::Win11);
    /// ```
    pub fn from_windows_version(major: u32, minor: u32, build: u32) -> Self {
        if major == 10 {
            if build >= 22000 {
                return Self::Win11;
            }
            return Self::Win10;
        }
        if major == 6 && minor == 1 {
            return Self::Win7;
        }
        if major == 6 && (minor == 2 || minor == 3) {
            return Self::Win8Family;
        }
        let _ = build;
        Self::Other
    }

    /// Human label for reports.
    pub fn label(self) -> &'static str {
        match self {
            Self::Win7 => "Windows 7",
            Self::Win8Family => "Windows 8/8.1",
            Self::Win10 => "Windows 10",
            Self::Win11 => "Windows 11",
            Self::Other => "other/non-Windows",
        }
    }
}

/// Driver / component package id matching `agent.md` layout under `drivers/`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DriverPackageId {
    /// KMDF keyboard/mouse filter (`drivers/input-filter`).
    InputFilter,
    /// Experimental virtual HID (`drivers/virtual-input`).
    VirtualInput,
    /// IddCx indirect display (`drivers/display/iddcx`) — Win10/11 only.
    IddCx,
    /// WDDM legacy research (`drivers/display/wddm-legacy`) — Win8/8.1.
    WddmLegacy,
    /// Win7 display research (`drivers/display/win7-legacy`).
    Win7Legacy,
    /// User-mode service + GUI + session-agent payload (always planned).
    UserModePayload,
}

impl DriverPackageId {
    /// Stable machine id for JSON / checklists.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::InputFilter => "input-filter",
            Self::VirtualInput => "virtual-input",
            Self::IddCx => "display-iddcx",
            Self::WddmLegacy => "display-wddm-legacy",
            Self::Win7Legacy => "display-win7-legacy",
            Self::UserModePayload => "user-mode-payload",
        }
    }

    /// Repo-relative source tree for this package (`agent.md` layout).
    pub fn source_tree(self) -> &'static str {
        match self {
            Self::InputFilter => "drivers/input-filter",
            Self::VirtualInput => "drivers/virtual-input",
            Self::IddCx => "drivers/display/iddcx",
            Self::WddmLegacy => "drivers/display/wddm-legacy",
            Self::Win7Legacy => "drivers/display/win7-legacy",
            Self::UserModePayload => "crates/",
        }
    }

    /// Repo-relative packaging artifacts (INF / sys / headers). Empty if none yet.
    pub fn packaging_artifacts(self) -> &'static [&'static str] {
        match self {
            Self::InputFilter => &[
                "drivers/input-filter/inf/RustMultiseatKbFilter.inf",
                "drivers/input-filter/inf/RustMultiseatMouFilter.inf",
                "drivers/input-filter/sys/RustMultiseatInputFilter.vcxproj",
                "drivers/input-filter/sys/RustMultiseatInputFilter.sln",
                "drivers/input-filter/include/rms_if_ioctl.h",
            ],
            Self::VirtualInput => &["drivers/virtual-input/README.md"],
            Self::IddCx => &["drivers/display/iddcx/README.md"],
            Self::WddmLegacy => &["drivers/display/wddm-legacy/README.md"],
            Self::Win7Legacy => &["drivers/display/win7-legacy/README.md"],
            Self::UserModePayload => &["crates/daemon/", "crates/gui/", "crates/session-agent/"],
        }
    }

    /// SCM / filter service names expected after a successful lab install.
    pub fn service_names(self) -> &'static [&'static str] {
        match self {
            Self::InputFilter => &["RustMultiseatKbFilter", "RustMultiseatMouFilter"],
            _ => &[],
        }
    }

    /// Lab control device DOS name (without `\\.\`), if any.
    pub fn control_device_name(self) -> Option<&'static str> {
        match self {
            Self::InputFilter => Some("RustMultiseatInputFilter"),
            _ => None,
        }
    }
}

/// How the package must be signed for production (lab test-sign is never production).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SigningPath {
    /// Windows 10+ attestation / WHCP modern flow.
    AttestationOrWhcp,
    /// Windows 7/8/8.1 legacy WHCP/HCK — attestation does **not** replace this.
    LegacyWhcpHck,
    /// User-mode Authenticode (service/GUI/agent).
    Authenticode,
    /// Lab-only test certificate / test signing mode.
    LabTestOnly,
}

impl SigningPath {
    /// Short label for reports.
    pub fn label(self) -> &'static str {
        match self {
            Self::AttestationOrWhcp => "attestation/WHCP (Win10+)",
            Self::LegacyWhcpHck => "legacy WHCP/HCK (Win7/8/8.1)",
            Self::Authenticode => "Authenticode (user-mode)",
            Self::LabTestOnly => "lab test-sign only",
        }
    }
}

/// One row in the install plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlannedPackage {
    /// Package id.
    pub id: DriverPackageId,
    /// Whether this package is selected for the host generation.
    pub selected: bool,
    /// Signing path required for production install.
    pub signing: SigningPath,
    /// Checklist status for this package (usually Unknown until lab/HLK).
    pub status: CheckStatus,
    /// Why selected/skipped and next action.
    pub detail: String,
}

/// Full Stage 7 install plan for a host snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstallPlan {
    /// Detected OS generation.
    pub os_generation: OsGeneration,
    /// Caps used when building the plan.
    pub capabilities: PlatformCapabilities,
    /// Planned packages in install order.
    pub packages: Vec<PlannedPackage>,
    /// True when every **selected** package is `Pass` (never true until lab/HLK).
    pub ready_to_ship: bool,
    /// Short summary.
    pub summary: String,
    /// Ordered operator actions.
    pub next_actions: Vec<String>,
}

impl InstallPlan {
    /// Builds a plan from capabilities + optional Windows version numbers.
    pub fn from_capabilities(
        caps: &PlatformCapabilities,
        version: Option<(u32, u32, u32)>,
    ) -> Self {
        let os_generation = match version {
            Some((maj, min, build)) => OsGeneration::from_windows_version(maj, min, build),
            None if caps.os.eq_ignore_ascii_case("windows") => OsGeneration::Other,
            None => OsGeneration::Other,
        };

        let mut packages = Vec::new();
        packages.push(PlannedPackage {
            id: DriverPackageId::UserModePayload,
            selected: true,
            signing: SigningPath::Authenticode,
            status: CheckStatus::Unknown,
            detail:
                "daemon + ctl + gui + session-agent; SCM install via multiseat-daemon --install"
                    .into(),
        });

        packages.push(PlannedPackage {
            id: DriverPackageId::InputFilter,
            selected: true,
            signing: signing_for_drivers(os_generation),
            status: CheckStatus::Blocked,
            detail: format!(
                "INF skeleton ready ({}); .sys/.cat not built; services={:?}; control=\\\\.\\{}; Stage 0 input PoC + WDK build required",
                DriverPackageId::InputFilter
                    .packaging_artifacts()
                    .iter()
                    .filter(|p| p.ends_with(".inf"))
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", "),
                DriverPackageId::InputFilter.service_names(),
                DriverPackageId::InputFilter
                    .control_device_name()
                    .unwrap_or("?")
            ),
        });

        packages.push(PlannedPackage {
            id: DriverPackageId::VirtualInput,
            selected: false,
            signing: signing_for_drivers(os_generation),
            status: CheckStatus::Blocked,
            detail: "experimental; only if Stage 0 proves session-targeted injection".into(),
        });

        match os_generation {
            OsGeneration::Win10 | OsGeneration::Win11 => {
                packages.push(PlannedPackage {
                    id: DriverPackageId::IddCx,
                    selected: true,
                    signing: SigningPath::AttestationOrWhcp,
                    status: CheckStatus::Blocked,
                    detail: "IddCx candidate; session-bind PoC + HLK still required".into(),
                });
                packages.push(PlannedPackage {
                    id: DriverPackageId::WddmLegacy,
                    selected: false,
                    signing: SigningPath::LegacyWhcpHck,
                    status: CheckStatus::Blocked,
                    detail: "not for Win10/11 primary path".into(),
                });
                packages.push(PlannedPackage {
                    id: DriverPackageId::Win7Legacy,
                    selected: false,
                    signing: SigningPath::LegacyWhcpHck,
                    status: CheckStatus::Blocked,
                    detail: "not for Win10/11".into(),
                });
            }
            OsGeneration::Win8Family => {
                packages.push(PlannedPackage {
                    id: DriverPackageId::IddCx,
                    selected: false,
                    signing: SigningPath::AttestationOrWhcp,
                    status: CheckStatus::Blocked,
                    detail: "IddCx unavailable before Windows 10".into(),
                });
                packages.push(PlannedPackage {
                    id: DriverPackageId::WddmLegacy,
                    selected: true,
                    signing: SigningPath::LegacyWhcpHck,
                    status: CheckStatus::Blocked,
                    detail: "WDDM legacy research package; attestation signing is not a substitute"
                        .into(),
                });
                packages.push(PlannedPackage {
                    id: DriverPackageId::Win7Legacy,
                    selected: false,
                    signing: SigningPath::LegacyWhcpHck,
                    status: CheckStatus::Blocked,
                    detail: "use wddm-legacy for 8/8.1".into(),
                });
            }
            OsGeneration::Win7 => {
                packages.push(PlannedPackage {
                    id: DriverPackageId::IddCx,
                    selected: false,
                    signing: SigningPath::AttestationOrWhcp,
                    status: CheckStatus::Blocked,
                    detail: "IddCx unavailable on Windows 7".into(),
                });
                packages.push(PlannedPackage {
                    id: DriverPackageId::WddmLegacy,
                    selected: false,
                    signing: SigningPath::LegacyWhcpHck,
                    status: CheckStatus::Blocked,
                    detail: "prefer win7-legacy package for Windows 7".into(),
                });
                packages.push(PlannedPackage {
                    id: DriverPackageId::Win7Legacy,
                    selected: true,
                    signing: SigningPath::LegacyWhcpHck,
                    status: CheckStatus::Blocked,
                    detail: "Win7 display research; legacy WHCP/HCK required for production".into(),
                });
            }
            OsGeneration::Other => {
                packages.push(PlannedPackage {
                    id: DriverPackageId::IddCx,
                    selected: false,
                    signing: SigningPath::LabTestOnly,
                    status: CheckStatus::Blocked,
                    detail: "non-Windows / unknown host — plan is scaffolding only".into(),
                });
            }
        }

        let mut next_actions = vec![
            "Complete Stage 0 lab checklist (dual_session / display / input) before packaging drivers"
                .into(),
            "WDK: msbuild drivers/input-filter/sys/RustMultiseatInputFilter.vcxproj /p:Configuration=Release /p:Platform=x64; copy .sys next to INF; inf2cat + lab test-sign"
                .into(),
            "Never ship with Secure Boot off or production test-signing".into(),
            "Document restore point / WinRE offline rollback for driver uninstall".into(),
        ];
        match os_generation {
            OsGeneration::Win7 | OsGeneration::Win8Family => {
                next_actions.push(
                    "Book legacy WHCP/HCK for selected kernel packages (attestation ≠ legacy cert)"
                        .into(),
                );
            }
            OsGeneration::Win10 | OsGeneration::Win11 => {
                next_actions.push(
                    "Plan WHCP/attestation for IddCx + input-filter; verify current Microsoft signing docs before release"
                        .into(),
                );
            }
            OsGeneration::Other => {
                next_actions.push(
                    "Re-run install-plan on a Windows lab host to select real driver packages"
                        .into(),
                );
            }
        }
        if caps.secure_boot_expected {
            next_actions.push(
                "Verify Secure Boot remains enabled after install (signature enforcement on)"
                    .into(),
            );
        }

        let ready_to_ship = packages
            .iter()
            .filter(|p| p.selected)
            .all(|p| p.status == CheckStatus::Pass);
        let selected = packages.iter().filter(|p| p.selected).count();
        let summary = if ready_to_ship {
            format!(
                "READY: {} selected packages Pass on {}",
                selected,
                os_generation.label()
            )
        } else {
            format!(
                "NOT READY: {} selected package(s) on {} — Stage 0 + HLK/signing pending",
                selected,
                os_generation.label()
            )
        };

        Self {
            os_generation,
            capabilities: caps.clone(),
            packages,
            ready_to_ship,
            summary,
            next_actions,
        }
    }

    /// Markdown report for `installer/` artifacts.
    pub fn to_markdown(&self) -> String {
        let mut out = String::new();
        out.push_str("# Stage 7 install plan\n\n");
        out.push_str(&format!("{}\n\n", self.summary));
        out.push_str(&format!(
            "- OS generation: **{}**\n- caps.os=`{}` `{}` secure_boot_expected={}\n\n",
            self.os_generation.label(),
            self.capabilities.os,
            self.capabilities.os_version,
            self.capabilities.secure_boot_expected
        ));
        out.push_str("| package | selected | signing | status | detail |\n|---|---|---|---|---|\n");
        for p in &self.packages {
            let detail = p.detail.replace('|', "\\|");
            out.push_str(&format!(
                "| `{}` | {} | {} | {:?} | {} |\n",
                p.id.as_str(),
                p.selected,
                p.signing.label(),
                p.status,
                detail
            ));
        }
        out.push_str("\n## Selected package artifacts\n\n");
        for p in self.packages.iter().filter(|p| p.selected) {
            out.push_str(&format!("### `{}`\n\n", p.id.as_str()));
            out.push_str(&format!("- source tree: `{}`\n", p.id.source_tree()));
            if !p.id.service_names().is_empty() {
                out.push_str(&format!(
                    "- services: `{}`\n",
                    p.id.service_names().join("`, `")
                ));
            }
            if let Some(dev) = p.id.control_device_name() {
                out.push_str(&format!("- control device: `\\\\.\\{dev}`\n"));
            }
            for art in p.id.packaging_artifacts() {
                out.push_str(&format!("- `{art}`\n"));
            }
            out.push('\n');
        }
        out.push_str("## Next actions\n\n");
        for a in &self.next_actions {
            out.push_str(&format!("- [ ] {a}\n"));
        }
        out.push_str(
            "\n## Recovery (operator)\n\n\
             - Prefer documented restore point before kernel driver install.\n\
             - Keep WinRE / offline uninstall notes with each driver package.\n\
             - Uninstall order: display → input filter → service → user-mode payload.\n\
             - Input filter lab: see `drivers/input-filter/inf/README.md` (pnputil / UpperFilters).\n",
        );
        out
    }
}

fn signing_for_drivers(os: OsGeneration) -> SigningPath {
    match os {
        OsGeneration::Win10 | OsGeneration::Win11 => SigningPath::AttestationOrWhcp,
        OsGeneration::Win7 | OsGeneration::Win8Family => SigningPath::LegacyWhcpHck,
        OsGeneration::Other => SigningPath::LabTestOnly,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn caps(os: &str, secure_boot: bool) -> PlatformCapabilities {
        PlatformCapabilities {
            os: os.into(),
            os_version: "test".into(),
            display_backend_available: false,
            session_backend_available: false,
            input_router_available: false,
            secure_boot_expected: secure_boot,
        }
    }

    #[test]
    fn win11_selects_iddcx_and_attestation() {
        let plan = InstallPlan::from_capabilities(&caps("windows", true), Some((10, 0, 22621)));
        assert_eq!(plan.os_generation, OsGeneration::Win11);
        assert!(!plan.ready_to_ship);
        let iddcx = plan
            .packages
            .iter()
            .find(|p| p.id == DriverPackageId::IddCx)
            .unwrap();
        assert!(iddcx.selected);
        assert_eq!(iddcx.signing, SigningPath::AttestationOrWhcp);
        assert!(
            !plan
                .packages
                .iter()
                .find(|p| p.id == DriverPackageId::Win7Legacy)
                .unwrap()
                .selected
        );
    }

    #[test]
    fn win7_selects_legacy_signing() {
        let plan = InstallPlan::from_capabilities(&caps("windows", true), Some((6, 1, 7601)));
        assert_eq!(plan.os_generation, OsGeneration::Win7);
        let win7 = plan
            .packages
            .iter()
            .find(|p| p.id == DriverPackageId::Win7Legacy)
            .unwrap();
        assert!(win7.selected);
        assert_eq!(win7.signing, SigningPath::LegacyWhcpHck);
        assert!(
            !plan
                .packages
                .iter()
                .find(|p| p.id == DriverPackageId::IddCx)
                .unwrap()
                .selected
        );
        assert!(plan.next_actions.iter().any(|a| a.contains("WHCP/HCK")));
    }

    #[test]
    fn non_windows_not_ready() {
        let plan = InstallPlan::from_capabilities(&caps("macos", false), None);
        assert_eq!(plan.os_generation, OsGeneration::Other);
        assert!(!plan.ready_to_ship);
        assert!(plan.summary.contains("NOT READY"));
        let md = plan.to_markdown();
        assert!(md.contains("Stage 7"));
        assert!(md.contains("Recovery"));
    }

    #[test]
    fn input_filter_detail_points_at_inf_and_services() {
        let plan = InstallPlan::from_capabilities(&caps("windows", true), Some((10, 0, 19045)));
        let row = plan
            .packages
            .iter()
            .find(|p| p.id == DriverPackageId::InputFilter)
            .unwrap();
        assert!(row.detail.contains("RustMultiseatKbFilter.inf"));
        assert!(row.detail.contains("RustMultiseatMouFilter.inf"));
        assert!(row.detail.contains("RustMultiseatKbFilter"));
        assert_eq!(
            DriverPackageId::InputFilter.source_tree(),
            "drivers/input-filter"
        );
        assert_eq!(
            DriverPackageId::InputFilter.service_names(),
            &["RustMultiseatKbFilter", "RustMultiseatMouFilter"]
        );
        assert_eq!(
            DriverPackageId::InputFilter.control_device_name(),
            Some("RustMultiseatInputFilter")
        );
        let md = plan.to_markdown();
        assert!(md.contains("Selected package artifacts"));
        assert!(md.contains("drivers/input-filter/inf/RustMultiseatKbFilter.inf"));
        assert!(md.contains("drivers/input-filter/sys/RustMultiseatInputFilter.vcxproj"));
        assert!(DriverPackageId::InputFilter
            .packaging_artifacts()
            .iter()
            .any(|p| p.ends_with("RustMultiseatInputFilter.vcxproj")));
    }
}
