//! Picks which [`MultiseatConfig`] the dashboard should send for validate/start.

use multiseat_core::MultiseatConfig;
use multiseat_daemon::lab_sample_config;

/// Source of the config used for a mutating GUI action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigSource {
    /// Daemon-saved config from the last snapshot.
    Saved,
    /// Built-in one-seat lab sample (mock inventory).
    LabSample,
}

/// Returns the config to send and where it came from.
///
/// Prefer the saved snapshot config when it has at least one seat; otherwise
/// fall back to the lab sample so Validate/Start stay usable against mock backends.
pub fn config_for_action(saved: &MultiseatConfig) -> (MultiseatConfig, ConfigSource) {
    if saved.seats.is_empty() {
        (lab_sample_config(), ConfigSource::LabSample)
    } else {
        (saved.clone(), ConfigSource::Saved)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use multiseat_core::{SeatConfig, SeatId, UserSid};

    #[test]
    fn empty_saved_uses_lab_sample() {
        let (cfg, src) = config_for_action(&MultiseatConfig::default());
        assert_eq!(src, ConfigSource::LabSample);
        assert!(!cfg.seats.is_empty());
    }

    #[test]
    fn non_empty_saved_is_preferred() {
        let saved = MultiseatConfig {
            seats: vec![SeatConfig {
                id: SeatId(9),
                user_sid: UserSid("S-1-5-21-1".into()),
                display_ids: vec![],
                keyboard_ids: vec![],
                pointer_ids: vec![],
                enabled: true,
            }],
            emergency_keyboard: None,
            emergency_pointer: None,
        };
        let (cfg, src) = config_for_action(&saved);
        assert_eq!(src, ConfigSource::Saved);
        assert_eq!(cfg.seats[0].id, SeatId(9));
    }
}
