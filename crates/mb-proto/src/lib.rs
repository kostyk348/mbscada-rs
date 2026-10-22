//! `mb-proto` — dependency-free Modbus implementation.
//!
//! * [`pdu`] — request encoding / response decoding (FC 1-6, 15, 16, 22, 23).
//! * [`mbap`] — Modbus/TCP framing.
//! * [`rtu`] — Modbus RTU framing with CRC-16.
//! * [`crc`] — CRC-16 (poly 0xA001).

pub mod ascii;
pub mod crc;
pub mod error;
pub mod mbap;
pub mod pdu;
pub mod rtu;

pub use error::{ExceptionCode, ProtoError, Result};
pub use pdu::{decode_request, decode_response, encode_response, Request, Response};
