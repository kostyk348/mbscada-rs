//! `mb-sim` — an in-memory Modbus slave.
//!
//! Applies request PDUs to four standard data areas (coils, discrete inputs,
//! holding registers, input registers) and produces response PDUs. Can also
//! serve Modbus/TCP on a background thread for integration tests.

use mb_proto::error::ExceptionCode;
use mb_proto::pdu::{decode_request, encode_response, Request, Response};
use mb_proto::{mbap, Result as ProtoResult};

pub mod net;

/// Default size of each register/bits area.
pub const DEFAULT_SIZE: usize = 65536;

/// A simulated Modbus device.
#[derive(Debug, Clone)]
pub struct Sim {
    pub unit: u8,
    pub coils: Vec<bool>,
    pub discrete: Vec<bool>,
    pub holding: Vec<u16>,
    pub input: Vec<u16>,
}

impl Sim {
    pub fn new(unit: u8) -> Self {
        Self {
            unit,
            coils: vec![false; DEFAULT_SIZE],
            discrete: vec![false; DEFAULT_SIZE],
            holding: vec![0; DEFAULT_SIZE],
            input: vec![0; DEFAULT_SIZE],
        }
    }

    fn in_range(len: usize, addr: u16, qty: u16) -> std::result::Result<usize, ExceptionCode> {
        if qty == 0 {
            return Err(ExceptionCode::IllegalDataValue);
        }
        let a = addr as usize;
        let end = a + qty as usize;
        if end > len {
            return Err(ExceptionCode::IllegalDataAddress);
        }
        Ok(a)
    }

    /// Apply a request, returning either a response or an exception code.
    pub fn apply(&mut self, req: &Request) -> std::result::Result<Response, ExceptionCode> {
        match req {
            Request::ReadCoils { addr, qty } => {
                let a = Self::in_range(self.coils.len(), *addr, *qty)?;
                Ok(Response::Bits(
                    self.coils[a..a + *qty as usize].to_vec(),
                ))
            }
            Request::ReadDiscreteInputs { addr, qty } => {
                let a = Self::in_range(self.discrete.len(), *addr, *qty)?;
                Ok(Response::Bits(
                    self.discrete[a..a + *qty as usize].to_vec(),
                ))
            }
            Request::ReadHoldingRegisters { addr, qty } => {
                let a = Self::in_range(self.holding.len(), *addr, *qty)?;
                Ok(Response::Registers(
                    self.holding[a..a + *qty as usize].to_vec(),
                ))
            }
            Request::ReadInputRegisters { addr, qty } => {
                let a = Self::in_range(self.input.len(), *addr, *qty)?;
                Ok(Response::Registers(
                    self.input[a..a + *qty as usize].to_vec(),
                ))
            }
            Request::WriteSingleCoil { addr, on } => {
                let a = Self::in_range(self.coils.len(), *addr, 1)?;
                self.coils[a] = *on;
                Ok(Response::SingleCoil {
                    addr: *addr,
                    on: *on,
                })
            }
            Request::WriteSingleRegister { addr, value } => {
                let a = Self::in_range(self.holding.len(), *addr, 1)?;
                self.holding[a] = *value;
                Ok(Response::SingleRegister {
                    addr: *addr,
                    value: *value,
                })
            }
            Request::WriteMultipleCoils { addr, values } => {
                let a = Self::in_range(self.coils.len(), *addr, values.len() as u16)?;
                self.coils[a..a + values.len()].copy_from_slice(values);
                Ok(Response::Written {
                    addr: *addr,
                    qty: values.len() as u16,
                })
            }
            Request::WriteMultipleRegisters { addr, values } => {
                let a = Self::in_range(self.holding.len(), *addr, values.len() as u16)?;
                self.holding[a..a + values.len()].copy_from_slice(values);
                Ok(Response::Written {
                    addr: *addr,
                    qty: values.len() as u16,
                })
            }
            Request::MaskWriteRegister {
                addr,
                and_mask,
                or_mask,
            } => {
                let a = Self::in_range(self.holding.len(), *addr, 1)?;
                let cur = self.holding[a];
                self.holding[a] = (cur & and_mask) | (or_mask & !and_mask);
                Ok(Response::MaskWritten {
                    addr: *addr,
                    and_mask: *and_mask,
                    or_mask: *or_mask,
                })
            }
            Request::ReadWriteMultipleRegisters {
                read_addr,
                read_qty,
                write_addr,
                write_values,
            } => {
                let wa = Self::in_range(self.holding.len(), *write_addr, write_values.len() as u16)?;
                self.holding[wa..wa + write_values.len()].copy_from_slice(write_values);
                let ra = Self::in_range(self.holding.len(), *read_addr, *read_qty)?;
                Ok(Response::Registers(
                    self.holding[ra..ra + *read_qty as usize].to_vec(),
                ))
            }
        }
    }

    /// Handle a raw PDU, returning a raw response PDU (exceptions encoded).
    pub fn handle_pdu(&mut self, pdu: &[u8]) -> Vec<u8> {
        match decode_request(pdu) {
            Ok(req) => {
                let func = req.func();
                match self.apply(&req) {
                    Ok(resp) => encode_response(func, &resp),
                    Err(ex) => vec![func | 0x80, ex as u8],
                }
            }
            Err(_) => {
                let func = pdu.first().copied().unwrap_or(0);
                vec![func | 0x80, ExceptionCode::IllegalFunction as u8]
            }
        }
    }

    /// Handle a full Modbus/TCP frame, returning a response frame.
    pub fn handle_mbap(&mut self, frame: &mbap::MbapFrame) -> ProtoResult<mbap::MbapFrame> {
        let pdu = self.handle_pdu(&frame.pdu);
        Ok(mbap::MbapFrame {
            transaction: frame.transaction,
            unit: frame.unit,
            pdu,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mb_proto::pdu::{decode_response, Request};

    #[test]
    fn write_then_read_roundtrip() {
        let mut sim = Sim::new(1);
        sim.apply(&Request::WriteMultipleRegisters {
            addr: 10,
            values: vec![111, 222, 333],
        })
        .unwrap();
        let r = sim
            .apply(&Request::ReadHoldingRegisters { addr: 10, qty: 3 })
            .unwrap();
        assert_eq!(r, Response::Registers(vec![111, 222, 333]));
    }

    #[test]
    fn out_of_range_is_exception() {
        let mut sim = Sim::new(1);
        let e = sim
            .apply(&Request::ReadHoldingRegisters {
                addr: 65535,
                qty: 10,
            })
            .unwrap_err();
        assert_eq!(e, ExceptionCode::IllegalDataAddress);
    }

    #[test]
    fn handle_pdu_roundtrip() {
        let mut sim = Sim::new(1);
        sim.holding[0] = 0xBEEF;
        let req = Request::ReadHoldingRegisters { addr: 0, qty: 1 }.encode();
        let resp = sim.handle_pdu(&req);
        assert_eq!(
            decode_response(&resp).unwrap(),
            Response::Registers(vec![0xBEEF])
        );
    }

    #[test]
    fn mask_write_register() {
        let mut sim = Sim::new(1);
        sim.holding[5] = 0x0012;
        // AND 0x00F2, OR 0x0025 -> (0x0012 & 0x00F2) | (0x0025 & !0x00F2) = 0x0012|0x0005 = 0x0017
        sim.apply(&Request::MaskWriteRegister {
            addr: 5,
            and_mask: 0x00F2,
            or_mask: 0x0025,
        })
        .unwrap();
        assert_eq!(sim.holding[5], 0x0017);
    }
}
