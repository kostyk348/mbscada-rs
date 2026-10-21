//! Modbus PDU (protocol data unit): request encoding and response decoding.

use crate::error::{ExceptionCode, ProtoError, Result};

pub const FC_READ_COILS: u8 = 1;
pub const FC_READ_DISCRETE_INPUTS: u8 = 2;
pub const FC_READ_HOLDING_REGISTERS: u8 = 3;
pub const FC_READ_INPUT_REGISTERS: u8 = 4;
pub const FC_WRITE_SINGLE_COIL: u8 = 5;
pub const FC_WRITE_SINGLE_REGISTER: u8 = 6;
pub const FC_WRITE_MULTIPLE_COILS: u8 = 15;
pub const FC_WRITE_MULTIPLE_REGISTERS: u8 = 16;
pub const FC_MASK_WRITE_REGISTER: u8 = 22;
pub const FC_READ_WRITE_MULTIPLE_REGISTERS: u8 = 23;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    ReadCoils { addr: u16, qty: u16 },
    ReadDiscreteInputs { addr: u16, qty: u16 },
    ReadHoldingRegisters { addr: u16, qty: u16 },
    ReadInputRegisters { addr: u16, qty: u16 },
    WriteSingleCoil { addr: u16, on: bool },
    WriteSingleRegister { addr: u16, value: u16 },
    WriteMultipleCoils { addr: u16, values: Vec<bool> },
    WriteMultipleRegisters { addr: u16, values: Vec<u16> },
    MaskWriteRegister { addr: u16, and_mask: u16, or_mask: u16 },
    ReadWriteMultipleRegisters {
        read_addr: u16,
        read_qty: u16,
        write_addr: u16,
        write_values: Vec<u16>,
    },
}

impl Request {
    pub fn func(&self) -> u8 {
        match self {
            Request::ReadCoils { .. } => FC_READ_COILS,
            Request::ReadDiscreteInputs { .. } => FC_READ_DISCRETE_INPUTS,
            Request::ReadHoldingRegisters { .. } => FC_READ_HOLDING_REGISTERS,
            Request::ReadInputRegisters { .. } => FC_READ_INPUT_REGISTERS,
            Request::WriteSingleCoil { .. } => FC_WRITE_SINGLE_COIL,
            Request::WriteSingleRegister { .. } => FC_WRITE_SINGLE_REGISTER,
            Request::WriteMultipleCoils { .. } => FC_WRITE_MULTIPLE_COILS,
            Request::WriteMultipleRegisters { .. } => FC_WRITE_MULTIPLE_REGISTERS,
            Request::MaskWriteRegister { .. } => FC_MASK_WRITE_REGISTER,
            Request::ReadWriteMultipleRegisters { .. } => FC_READ_WRITE_MULTIPLE_REGISTERS,
        }
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut v = Vec::new();
        v.push(self.func());
        match self {
            Request::ReadCoils { addr, qty }
            | Request::ReadDiscreteInputs { addr, qty }
            | Request::ReadHoldingRegisters { addr, qty }
            | Request::ReadInputRegisters { addr, qty } => {
                v.extend(addr.to_be_bytes());
                v.extend(qty.to_be_bytes());
            }
            Request::WriteSingleCoil { addr, on } => {
                v.extend(addr.to_be_bytes());
                v.extend(if *on { [0xFF, 0x00] } else { [0x00, 0x00] });
            }
            Request::WriteSingleRegister { addr, value } => {
                v.extend(addr.to_be_bytes());
                v.extend(value.to_be_bytes());
            }
            Request::WriteMultipleCoils { addr, values } => {
                v.extend(addr.to_be_bytes());
                v.extend((values.len() as u16).to_be_bytes());
                let packed = pack_bits(values);
                v.push(packed.len() as u8);
                v.extend(&packed);
            }
            Request::WriteMultipleRegisters { addr, values } => {
                v.extend(addr.to_be_bytes());
                v.extend((values.len() as u16).to_be_bytes());
                v.push((values.len() * 2) as u8);
                for w in values {
                    v.extend(w.to_be_bytes());
                }
            }
            Request::MaskWriteRegister {
                addr,
                and_mask,
                or_mask,
            } => {
                v.extend(addr.to_be_bytes());
                v.extend(and_mask.to_be_bytes());
                v.extend(or_mask.to_be_bytes());
            }
            Request::ReadWriteMultipleRegisters {
                read_addr,
                read_qty,
                write_addr,
                write_values,
            } => {
                v.extend(read_addr.to_be_bytes());
                v.extend(read_qty.to_be_bytes());
                v.extend(write_addr.to_be_bytes());
                v.extend((write_values.len() as u16).to_be_bytes());
                v.push((write_values.len() * 2) as u8);
                for w in write_values {
                    v.extend(w.to_be_bytes());
                }
            }
        }
        v
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Response {
    Bits(Vec<bool>),
    Registers(Vec<u16>),
    Written { addr: u16, qty: u16 },
    SingleCoil { addr: u16, on: bool },
    SingleRegister { addr: u16, value: u16 },
    MaskWritten { addr: u16, and_mask: u16, or_mask: u16 },
}

/// Decode a response PDU. Exceptions are surfaced as `ProtoError::Exception`.
pub fn decode_response(pdu: &[u8]) -> Result<Response> {
    let func = *pdu.first().ok_or(ProtoError::Truncated)?;
    if func & 0x80 != 0 {
        let code = *pdu.get(1).ok_or(ProtoError::Truncated)?;
        return Err(ProtoError::Exception(ExceptionCode::from_u8(code)));
    }
    let body = &pdu[1..];
    match func {
        FC_READ_COILS | FC_READ_DISCRETE_INPUTS => {
            let bc = *body.first().ok_or(ProtoError::Truncated)? as usize;
            let bytes = body.get(1..1 + bc).ok_or(ProtoError::BadLength {
                expected: 1 + bc,
                got: body.len(),
            })?;
            Ok(Response::Bits(unpack_bits(bytes)))
        }
        FC_READ_HOLDING_REGISTERS | FC_READ_INPUT_REGISTERS => {
            let bc = *body.first().ok_or(ProtoError::Truncated)? as usize;
            if bc % 2 != 0 {
                return Err(ProtoError::BadValue("odd register byte count".into()));
            }
            let bytes = body.get(1..1 + bc).ok_or(ProtoError::BadLength {
                expected: 1 + bc,
                got: body.len(),
            })?;
            Ok(Response::Registers(bytes_to_regs(bytes)))
        }
        FC_WRITE_SINGLE_COIL => {
            if body.len() < 4 {
                return Err(ProtoError::Truncated);
            }
            let addr = u16::from_be_bytes([body[0], body[1]]);
            let on = body[2] == 0xFF;
            Ok(Response::SingleCoil { addr, on })
        }
        FC_WRITE_SINGLE_REGISTER => {
            if body.len() < 4 {
                return Err(ProtoError::Truncated);
            }
            let addr = u16::from_be_bytes([body[0], body[1]]);
            let value = u16::from_be_bytes([body[2], body[3]]);
            Ok(Response::SingleRegister { addr, value })
        }
        FC_WRITE_MULTIPLE_COILS | FC_WRITE_MULTIPLE_REGISTERS => {
            if body.len() < 4 {
                return Err(ProtoError::Truncated);
            }
            Ok(Response::Written {
                addr: u16::from_be_bytes([body[0], body[1]]),
                qty: u16::from_be_bytes([body[2], body[3]]),
            })
        }
        FC_MASK_WRITE_REGISTER => {
            if body.len() < 6 {
                return Err(ProtoError::Truncated);
            }
            Ok(Response::MaskWritten {
                addr: u16::from_be_bytes([body[0], body[1]]),
                and_mask: u16::from_be_bytes([body[2], body[3]]),
                or_mask: u16::from_be_bytes([body[4], body[5]]),
            })
        }
        FC_READ_WRITE_MULTIPLE_REGISTERS => {
            let bc = *body.first().ok_or(ProtoError::Truncated)? as usize;
            let bytes = body.get(1..1 + bc).ok_or(ProtoError::BadLength {
                expected: 1 + bc,
                got: body.len(),
            })?;
            Ok(Response::Registers(bytes_to_regs(bytes)))
        }
        other => Err(ProtoError::UnknownFunction(other)),
    }
}

/// Decode a request PDU (used by servers / simulators).
pub fn decode_request(pdu: &[u8]) -> Result<Request> {
    let func = *pdu.first().ok_or(ProtoError::Truncated)?;
    let b = &pdu[1..];
    match func {
        FC_READ_COILS | FC_READ_DISCRETE_INPUTS | FC_READ_HOLDING_REGISTERS
        | FC_READ_INPUT_REGISTERS => {
            let addr = be16(b, 0)?;
            let qty = be16(b, 2)?;
            Ok(match func {
                FC_READ_COILS => Request::ReadCoils { addr, qty },
                FC_READ_DISCRETE_INPUTS => Request::ReadDiscreteInputs { addr, qty },
                FC_READ_HOLDING_REGISTERS => Request::ReadHoldingRegisters { addr, qty },
                _ => Request::ReadInputRegisters { addr, qty },
            })
        }
        FC_WRITE_SINGLE_COIL => Ok(Request::WriteSingleCoil {
            addr: be16(b, 0)?,
            on: b.get(2).copied() == Some(0xFF),
        }),
        FC_WRITE_SINGLE_REGISTER => Ok(Request::WriteSingleRegister {
            addr: be16(b, 0)?,
            value: be16(b, 2)?,
        }),
        FC_WRITE_MULTIPLE_COILS => {
            let addr = be16(b, 0)?;
            let qty = be16(b, 2)? as usize;
            let bc = *b.get(4).ok_or(ProtoError::Truncated)? as usize;
            let bytes = b.get(5..5 + bc).ok_or(ProtoError::Truncated)?;
            let mut bits = unpack_bits(bytes);
            bits.truncate(qty);
            Ok(Request::WriteMultipleCoils {
                addr,
                values: bits,
            })
        }
        FC_WRITE_MULTIPLE_REGISTERS => {
            let addr = be16(b, 0)?;
            let qty = be16(b, 2)? as usize;
            let bc = *b.get(4).ok_or(ProtoError::Truncated)? as usize;
            let bytes = b.get(5..5 + bc).ok_or(ProtoError::Truncated)?;
            let mut vals = bytes_to_regs(bytes);
            vals.truncate(qty);
            Ok(Request::WriteMultipleRegisters {
                addr,
                values: vals,
            })
        }
        FC_MASK_WRITE_REGISTER => Ok(Request::MaskWriteRegister {
            addr: be16(b, 0)?,
            and_mask: be16(b, 2)?,
            or_mask: be16(b, 4)?,
        }),
        FC_READ_WRITE_MULTIPLE_REGISTERS => {
            let read_addr = be16(b, 0)?;
            let read_qty = be16(b, 2)?;
            let write_addr = be16(b, 4)?;
            let wq = be16(b, 6)? as usize;
            let bc = *b.get(8).ok_or(ProtoError::Truncated)? as usize;
            let bytes = b.get(9..9 + bc).ok_or(ProtoError::Truncated)?;
            let mut vals = bytes_to_regs(bytes);
            vals.truncate(wq);
            Ok(Request::ReadWriteMultipleRegisters {
                read_addr,
                read_qty,
                write_addr,
                write_values: vals,
            })
        }
        other => Err(ProtoError::UnknownFunction(other)),
    }
}

/// Encode a success response PDU for the given function code.
pub fn encode_response(func: u8, resp: &Response) -> Vec<u8> {
    let mut v = vec![func];
    match resp {
        Response::Bits(bits) => {
            let p = pack_bits(bits);
            v.push(p.len() as u8);
            v.extend(&p);
        }
        Response::Registers(regs) => {
            v.push((regs.len() * 2) as u8);
            for r in regs {
                v.extend(r.to_be_bytes());
            }
        }
        Response::SingleCoil { addr, on } => {
            v.extend(addr.to_be_bytes());
            v.extend(if *on { [0xFF, 0x00] } else { [0x00, 0x00] });
        }
        Response::SingleRegister { addr, value } => {
            v.extend(addr.to_be_bytes());
            v.extend(value.to_be_bytes());
        }
        Response::Written { addr, qty } => {
            v.extend(addr.to_be_bytes());
            v.extend(qty.to_be_bytes());
        }
        Response::MaskWritten {
            addr,
            and_mask,
            or_mask,
        } => {
            v.extend(addr.to_be_bytes());
            v.extend(and_mask.to_be_bytes());
            v.extend(or_mask.to_be_bytes());
        }
    }
    v
}

fn be16(b: &[u8], o: usize) -> Result<u16> {
    b.get(o..o + 2)
        .map(|c| u16::from_be_bytes([c[0], c[1]]))
        .ok_or(ProtoError::Truncated)
}

pub fn pack_bits(bits: &[bool]) -> Vec<u8> {
    let mut out = vec![0u8; (bits.len() + 7) / 8];
    for (i, &b) in bits.iter().enumerate() {
        if b {
            out[i / 8] |= 1 << (i % 8);
        }
    }
    out
}

pub fn unpack_bits(bytes: &[u8]) -> Vec<bool> {
    let mut out = Vec::with_capacity(bytes.len() * 8);
    for &byte in bytes {
        for bit in 0..8 {
            out.push(byte & (1 << bit) != 0);
        }
    }
    out
}

fn bytes_to_regs(bytes: &[u8]) -> Vec<u16> {
    bytes
        .chunks_exact(2)
        .map(|c| u16::from_be_bytes([c[0], c[1]]))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_read_holding() {
        let r = Request::ReadHoldingRegisters {
            addr: 0x006B,
            qty: 3,
        };
        assert_eq!(r.encode(), vec![0x03, 0x00, 0x6B, 0x00, 0x03]);
    }

    #[test]
    fn encode_write_multiple_registers() {
        let r = Request::WriteMultipleRegisters {
            addr: 0x0001,
            values: vec![0x000A, 0x0102],
        };
        assert_eq!(
            r.encode(),
            vec![0x10, 0x00, 0x01, 0x00, 0x02, 0x04, 0x00, 0x0A, 0x01, 0x02]
        );
    }

    #[test]
    fn write_single_coil_encoding() {
        let on = Request::WriteSingleCoil {
            addr: 0x00AC,
            on: true,
        }
        .encode();
        assert_eq!(on, vec![0x05, 0x00, 0xAC, 0xFF, 0x00]);
        let off = Request::WriteSingleCoil {
            addr: 0x00AC,
            on: false,
        }
        .encode();
        assert_eq!(off, vec![0x05, 0x00, 0xAC, 0x00, 0x00]);
    }

    #[test]
    fn decode_registers_and_bits() {
        // 03 04 00 0A 01 02
        let r = decode_response(&[0x03, 0x04, 0x00, 0x0A, 0x01, 0x02]).unwrap();
        assert_eq!(r, Response::Registers(vec![10, 258]));
        // 01 01 05 (bits LSB first: 1,0,1,0,0,0,0,0)
        let b = decode_response(&[0x01, 0x01, 0x05]).unwrap();
        assert_eq!(
            b,
            Response::Bits(vec![true, false, true, false, false, false, false, false])
        );
    }

    #[test]
    fn decode_exception() {
        let e = decode_response(&[0x83, 0x02]).unwrap_err();
        assert_eq!(e, ProtoError::Exception(ExceptionCode::IllegalDataAddress));
    }

    #[test]
    fn bit_pack_roundtrip() {
        let bits = vec![true, false, true, true, false, false, false, true, true];
        let packed = pack_bits(&bits);
        assert_eq!(packed.len(), 2);
        let mut back = unpack_bits(&packed);
        back.truncate(bits.len());
        assert_eq!(back, bits);
    }
}
