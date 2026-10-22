use std::fmt;

/// Protocol exception codes (function code | 0x80 response).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExceptionCode {
    IllegalFunction = 1,
    IllegalDataAddress = 2,
    IllegalDataValue = 3,
    SlaveDeviceFailure = 4,
    Acknowledge = 5,
    SlaveDeviceBusy = 6,
    MemoryParityError = 8,
    GatewayPathUnavailable = 10,
    GatewayTargetNoResponse = 11,
}

impl ExceptionCode {
    pub fn from_u8(v: u8) -> ExceptionCode {
        match v {
            1 => ExceptionCode::IllegalFunction,
            2 => ExceptionCode::IllegalDataAddress,
            3 => ExceptionCode::IllegalDataValue,
            4 => ExceptionCode::SlaveDeviceFailure,
            5 => ExceptionCode::Acknowledge,
            6 => ExceptionCode::SlaveDeviceBusy,
            8 => ExceptionCode::MemoryParityError,
            10 => ExceptionCode::GatewayPathUnavailable,
            _ => ExceptionCode::GatewayTargetNoResponse,
        }
    }
}

impl fmt::Display for ExceptionCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            ExceptionCode::IllegalFunction => "illegal function",
            ExceptionCode::IllegalDataAddress => "illegal data address",
            ExceptionCode::IllegalDataValue => "illegal data value",
            ExceptionCode::SlaveDeviceFailure => "slave device failure",
            ExceptionCode::Acknowledge => "acknowledge",
            ExceptionCode::SlaveDeviceBusy => "slave device busy",
            ExceptionCode::MemoryParityError => "memory parity error",
            ExceptionCode::GatewayPathUnavailable => "gateway path unavailable",
            ExceptionCode::GatewayTargetNoResponse => "gateway target no response",
        };
        write!(f, "{s}")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProtoError {
    Truncated,
    BadLength { expected: usize, got: usize },
    UnknownFunction(u8),
    Exception(ExceptionCode),
    Crc { expected: u16, got: u16 },
    Lrc { expected: u8, got: u8 },
    NotAsciiFrame,
    BadAsciiHex(u8),
    BadProtocolId(u16),
    BadValue(String),
    Io(String),
}

impl fmt::Display for ProtoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProtoError::Truncated => write!(f, "truncated frame"),
            ProtoError::BadLength { expected, got } => {
                write!(f, "bad length: expected {expected}, got {got}")
            }
            ProtoError::UnknownFunction(fc) => write!(f, "unknown function code {fc}"),
            ProtoError::Exception(e) => write!(f, "modbus exception: {e}"),
            ProtoError::Crc { expected, got } => {
                write!(f, "crc mismatch: expected {expected:#06x}, got {got:#06x}")
            }
            ProtoError::Lrc { expected, got } => {
                write!(f, "lrc mismatch: expected {expected:#04x}, got {got:#04x}")
            }
            ProtoError::NotAsciiFrame => write!(f, "not an ASCII frame (no colon)"),
            ProtoError::BadAsciiHex(c) => write!(f, "bad hex char {:#04x} in ASCII frame", c),
            ProtoError::BadProtocolId(id) => write!(f, "bad protocol id {id}"),
            ProtoError::BadValue(m) => write!(f, "bad value: {m}"),
            ProtoError::Io(m) => write!(f, "io error: {m}"),
        }
    }
}

impl std::error::Error for ProtoError {}

pub type Result<T> = std::result::Result<T, ProtoError>;
