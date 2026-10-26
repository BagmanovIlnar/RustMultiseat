//! Sample configuration matching [`multiseat_platform_windows::MockHardwareEnumerator`].

use multiseat_core::{DeviceId, DisplayId, MultiseatConfig, SeatConfig, SeatId, UserSid};

/// One-seat lab configuration used by CLI demos and tests.
///
/// Device ids intentionally use instance-only form; daemon `save`/`start` and
/// hot-plug rematch enrich them with interface/container from the mock tree.
pub fn lab_sample_config() -> MultiseatConfig {
    MultiseatConfig {
        seats: vec![SeatConfig {
            id: SeatId(1),
            user_sid: UserSid("S-1-5-21-1000-1000-1000-1001".into()),
            display_ids: vec![DisplayId("\\\\.\\DISPLAY2".into())],
            keyboard_ids: vec![DeviceId::from_instance("HID\\VID_BBBB&PID_0001\\SEAT1_KBD")],
            pointer_ids: vec![DeviceId::from_instance("HID\\VID_BBBB&PID_0002\\SEAT1_PTR")],
            enabled: true,
        }],
        emergency_keyboard: Some(DeviceId::from_instance("HID\\VID_AAAA&PID_0001\\HOST_KBD")),
        emergency_pointer: Some(DeviceId::from_instance("HID\\VID_AAAA&PID_0002\\HOST_PTR")),
    }
}
