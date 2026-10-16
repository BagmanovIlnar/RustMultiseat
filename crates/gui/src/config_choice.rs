//! Picks which [`MultiseatConfig`] the dashboard should send for validate/start/save.

use multiseat_core::MultiseatConfig;
use multiseat_daemon::lab_sample_config;

/// Source of the config used for a mutating GUI action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigSource {
    /// Local draft edited via assignment UI.
    Draft,
    /// Daemon-saved config from the last snapshot.
    Saved,
    /// Built-in one-seat lab sample (mock inventory).
    LabSample,
}

/// Returns the config to send and where it came from.
///
/// Prefer a non-empty draft; else saved snapshot seats; else lab sample.
pub fn config_for_action(
    draft: &MultiseatConfig,
    saved: &MultiseatConfig,
) -> (MultiseatConfig, ConfigSource) {
    if !draft.seats.is_empty()
        || draft.emergency_keyboard.is_some()
        || draft.emergency_pointer.is_some()
    {
        (draft.clone(), ConfigSource::Draft)
    } else if !saved.seats.is_empty() {
        (saved.clone(), ConfigSource::Saved)
    } else {
        (lab_sample_config(), ConfigSource::LabSample)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use multiseat_core::{SeatConfig, SeatId, UserSid};

    fn one_seat(id: u32) -> MultiseatConfig {
        MultiseatConfig {
            seats: vec![SeatConfig {
                id: SeatId(id),
                user_sid: UserSid("S-1-5-21-1".into()),
                display_ids: vec![],
                keyboard_ids: vec![],
                pointer_ids: vec![],
                enabled: true,
            }],
            emergency_keyboard: None,
            emergency_pointer: None,
        }
    }

    #[test]
    fn draft_wins_when_present() {
        let draft = one_seat(3);
        let saved = one_seat(9);
        let (cfg, src) = config_for_action(&draft, &saved);
        assert_eq!(src, ConfigSource::Draft);
        assert_eq!(cfg.seats[0].id, SeatId(3));
    }

    #[test]
    fn empty_everything_uses_lab_sample() {
        let (cfg, src) = config_for_action(&MultiseatConfig::default(), &MultiseatConfig::default());
        assert_eq!(src, ConfigSource::LabSample);
        assert!(!cfg.seats.is_empty());
    }

    #[test]
    fn saved_used_when_draft_empty() {
        let saved = one_seat(9);
        let (cfg, src) = config_for_action(&MultiseatConfig::default(), &saved);
        assert_eq!(src, ConfigSource::Saved);
        assert_eq!(cfg.seats[0].id, SeatId(9));
    }
}
