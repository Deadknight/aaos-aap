//! AAP FIRST / MIDDLE / LAST fragmentation helpers.
//!
//! This mirrors the aa-proxy-rs packet_fragment.rs / pkt_debug behavior:
//! large application/plaintext payloads are split before TLS encryption. Each
//! fragment is then encrypted separately and carried by one encrypted AA frame.
//! On multi-fragment messages the FIRST frame carries `final_length`, which is
//! the total plaintext/application payload length, not the encrypted TLS length.

use std::collections::HashMap;

use super::{AapFrame, FRAME_TYPE_FIRST, FRAME_TYPE_LAST, FRAME_TYPE_MASK};

/// Default first-fragment plaintext payload size observed in AA streams.
pub const DEFAULT_FIRST_FRAGMENT_PAYLOAD_BYTES: usize = 16_120;
pub const MIN_FIRST_FRAGMENT_PAYLOAD_BYTES: usize = 512;
pub const MAX_FIRST_FRAGMENT_PAYLOAD_BYTES: usize = 16_120;

/// Continuation fragments are 4 bytes larger because the FIRST frame has the
/// extra 4-byte final_length header on the wire.
pub const DEFAULT_CONTINUATION_FRAGMENT_PAYLOAD_BONUS: usize = 4;

#[derive(Clone, Copy, Debug)]
pub struct PlainPayloadFragmentOptions {
    pub channel: u8,
    /// Frame-type bits are ignored. Non-frame bits are preserved.
    pub base_flags: u8,
    pub first_fragment_payload_bytes: usize,
    pub continuation_fragment_payload_bytes: usize,
    /// Total plaintext/application payload length for a multi-fragment message.
    pub first_final_length: Option<u32>,
}

pub fn frame_base_flags(flags: u8) -> u8 {
    flags & !FRAME_TYPE_MASK
}

pub fn clamp_first_fragment_payload_bytes(requested: usize) -> usize {
    requested.clamp(MIN_FIRST_FRAGMENT_PAYLOAD_BYTES, MAX_FIRST_FRAGMENT_PAYLOAD_BYTES)
}

pub fn openauto_continuation_fragment_payload_bytes(first_fragment_payload_bytes: usize) -> usize {
    first_fragment_payload_bytes.saturating_add(DEFAULT_CONTINUATION_FRAGMENT_PAYLOAD_BONUS)
}

/// Split a plaintext/application payload into AA frame-sized fragments.
/// Returned frame payloads are still plaintext. Encrypt each returned frame
/// independently and keep the original `flags` / `final_length` metadata.
pub fn fragment_plain_payload(payload: &[u8], opts: PlainPayloadFragmentOptions) -> Vec<AapFrame> {
    let first_chunk = opts.first_fragment_payload_bytes.max(1);
    let continuation_chunk = opts.continuation_fragment_payload_bytes.max(1);
    let base_flags = frame_base_flags(opts.base_flags);

    if payload.len() <= first_chunk {
        return vec![AapFrame {
            channel: opts.channel,
            flags: base_flags | FRAME_TYPE_FIRST | FRAME_TYPE_LAST,
            final_length: None,
            payload: payload.to_vec(),
        }];
    }

    let mut frames = Vec::with_capacity((payload.len() / continuation_chunk).saturating_add(2));
    frames.push(AapFrame {
        channel: opts.channel,
        flags: base_flags | FRAME_TYPE_FIRST,
        final_length: opts.first_final_length.or(Some(payload.len() as u32)),
        payload: payload[..first_chunk].to_vec(),
    });

    let mut pos = first_chunk;
    while pos < payload.len() {
        let remaining = payload.len() - pos;
        let take = remaining.min(continuation_chunk);
        let last = pos + take >= payload.len();
        frames.push(AapFrame {
            channel: opts.channel,
            flags: base_flags | if last { FRAME_TYPE_LAST } else { 0 },
            final_length: None,
            payload: payload[pos..pos + take].to_vec(),
        });
        pos += take;
    }

    frames
}

#[derive(Default)]
pub struct FragmentAssembler {
    buffers: HashMap<u8, FragmentBuffer>,
}

#[derive(Default)]
struct FragmentBuffer {
    expected_len: Option<usize>,
    data: Vec<u8>,
    fragments: usize,
}

#[derive(Debug, Clone)]
pub enum ReassemblyResult {
    Complete {
        payload: Vec<u8>,
        fragments: usize,
        expected_len: Option<usize>,
    },
    Waiting {
        fragments: usize,
        expected_len: Option<usize>,
        actual_len: usize,
    },
    Dropped(String),
}

impl FragmentAssembler {
    /// Reassemble already-decrypted AA plaintext fragments. `frame.payload` must
    /// be the plaintext fragment bytes and `frame.flags` should still contain
    /// the original FIRST/LAST bits; ENCRYPTED/CONTROL bits are ignored.
    pub fn push(&mut self, frame: &AapFrame) -> ReassemblyResult {
        match frame.flags & FRAME_TYPE_MASK {
            flags if flags == (FRAME_TYPE_FIRST | FRAME_TYPE_LAST) => {
                // Standalone packet; do not clear in-progress fragmented data
                // because AA may interleave standalone control packets on the
                // same channel while media fragments are active.
                ReassemblyResult::Complete {
                    payload: frame.payload.clone(),
                    fragments: 1,
                    expected_len: None,
                }
            }
            flags if flags == FRAME_TYPE_FIRST => {
                let expected_len = frame.final_length.map(|len| len as usize);
                let replaced = self
                    .buffers
                    .insert(
                        frame.channel,
                        FragmentBuffer {
                            expected_len,
                            data: frame.payload.clone(),
                            fragments: 1,
                        },
                    )
                    .is_some();
                let mut actual_len = frame.payload.len();
                if let Some(buf) = self.buffers.get(&frame.channel) {
                    actual_len = buf.data.len();
                }
                if replaced {
                    ReassemblyResult::Dropped(format!(
                        "replaced incomplete fragmented payload on channel 0x{:02x}",
                        frame.channel
                    ))
                } else {
                    ReassemblyResult::Waiting {
                        fragments: 1,
                        expected_len,
                        actual_len,
                    }
                }
            }
            flags if flags == FRAME_TYPE_LAST => {
                let Some(mut buffer) = self.buffers.remove(&frame.channel) else {
                    return ReassemblyResult::Dropped(format!(
                        "last fragment without first on channel 0x{:02x}",
                        frame.channel
                    ));
                };
                buffer.data.extend_from_slice(&frame.payload);
                buffer.fragments = buffer.fragments.saturating_add(1);
                if let Some(expected_len) = buffer.expected_len {
                    if buffer.data.len() != expected_len {
                        return ReassemblyResult::Dropped(format!(
                            "fragmented payload length mismatch on channel 0x{:02x}: expected {} actual {}",
                            frame.channel,
                            expected_len,
                            buffer.data.len()
                        ));
                    }
                }
                ReassemblyResult::Complete {
                    payload: buffer.data,
                    fragments: buffer.fragments,
                    expected_len: buffer.expected_len,
                }
            }
            _ => {
                let Some(buffer) = self.buffers.get_mut(&frame.channel) else {
                    return ReassemblyResult::Dropped(format!(
                        "middle fragment without first on channel 0x{:02x}",
                        frame.channel
                    ));
                };
                buffer.data.extend_from_slice(&frame.payload);
                buffer.fragments = buffer.fragments.saturating_add(1);
                ReassemblyResult::Waiting {
                    fragments: buffer.fragments,
                    expected_len: buffer.expected_len,
                    actual_len: buffer.data.len(),
                }
            }
        }
    }
}

pub fn smoke_summary() -> String {
    let payload = vec![0xAA; 55_251];
    let first = DEFAULT_FIRST_FRAGMENT_PAYLOAD_BYTES;
    let cont = openauto_continuation_fragment_payload_bytes(first);
    let frames = fragment_plain_payload(
        &payload,
        PlainPayloadFragmentOptions {
            channel: 0x08,
            base_flags: 0,
            first_fragment_payload_bytes: first,
            continuation_fragment_payload_bytes: cont,
            first_final_length: Some(payload.len() as u32),
        },
    );
    let sizes: Vec<usize> = frames.iter().map(|f| f.payload.len()).collect();
    format!(
        "fragment OK: total={} frames={} sizes={:?} first_final_length={:?}",
        payload.len(),
        frames.len(),
        sizes,
        frames.first().and_then(|f| f.final_length)
    )
}
