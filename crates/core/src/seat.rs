//! Seat configuration and runtime state machine values.

use serde::{Deserialize, Serialize};

use crate::ids::{DeviceId, DisplayId, SeatId, UserSid};

/// Runtime state of a seat or the whole multiseat session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SeatState {
    /// Seat is present but disabled in configuration.
    Disabled,
    /// Hardware is known; configuration may be edited.
    Idle,
    /// Configuration is being validated.
    Validating,
    /// Backends are applying session/display/input changes.
    Applying,
    /// Multiseat is active.
    Active,
    /// Active with a non-fatal degradation.
    Degraded {
        /// Human-readable reason.
        reason: String,
    },
    /// Compensating actions are running.
    RollingBack,
    /// Terminal failure until recover/stop.
    Failed {
        /// Human-readable error.
        error: String,
    },
}

impl SeatState {
    /// Whether start may be requested from this state.
    pub fn can_start(&self) -> bool {
        matches!(self, Self::Idle | Self::Disabled | Self::Failed { .. })
    }

    /// Whether stop may be requested from this state.
    pub fn can_stop(&self) -> bool {
        matches!(
            self,
            Self::Active | Self::Degraded { .. } | Self::Applying | Self::Failed { .. }
        )
    }
}

/// Per-seat assignment of user, displays and input devices.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SeatConfig {
    /// Seat number.
    pub id: SeatId,
    /// Target user SID.
    pub user_sid: UserSid,
    /// Displays assigned to the seat.
    pub display_ids: Vec<DisplayId>,
    /// Keyboards assigned to the seat.
    pub keyboard_ids: Vec<DeviceId>,
    /// Pointers assigned to the seat.
    pub pointer_ids: Vec<DeviceId>,
    /// Whether the seat participates in start.
    pub enabled: bool,
}

/// Full multiseat configuration including host emergency input.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct MultiseatConfig {
    /// Configured seats.
    pub seats: Vec<SeatConfig>,
    /// Keyboard reserved for host emergency control.
    pub emergency_keyboard: Option<DeviceId>,
    /// Pointer reserved for host emergency control.
    pub emergency_pointer: Option<DeviceId>,
}
