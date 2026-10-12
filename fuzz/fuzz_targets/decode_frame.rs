//! libFuzzer entry: `decode_frame` must never panic on arbitrary bytes.
//!
//! Run (nightly + cargo-fuzz):
//! ```text
//! cargo install cargo-fuzz
//! cargo +nightly fuzz run decode_frame
//! ```
//!
//! Or directly:
//! ```text
//! cd fuzz && cargo +nightly fuzz run decode_frame
//! ```

#![no_main]

use bytes::BytesMut;
use libfuzzer_sys::fuzz_target;
use multiseat_ipc::decode_frame;

fuzz_target!(|data: &[u8]| {
    // Cap input so the fuzzer spends time on header edge cases, not huge copies.
    let slice = if data.len() > 4096 { &data[..4096] } else { data };
    let mut buf = BytesMut::from(slice);
    // Drain as many frames as the decoder will accept; errors/partial reads are fine.
    for _ in 0..32 {
        match decode_frame(&mut buf) {
            Ok(Some(_)) => continue,
            Ok(None) | Err(_) => break,
        }
    }
});
