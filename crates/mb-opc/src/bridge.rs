//! Bridge core: project file -> OPC UA address space, engine snapshots ->
//! variable values.
//!
//! The binary wires this up with a CLI; tests drive the same functions
//! in-process.

use mb_engine::model::{func_is_read, Poll};
use mb_engine::{ComputedTag, Engine, Fmt};
use opcua::server::callbacks::AttributeSetter;
use opcua::server::prelude::*;
use opcua::sync::RwLock;
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// The subset of the desktop app's project JSON the bridge needs. Unknown
/// keys are ignored, `rules`/`computed` default to empty.
#[derive(Deserialize)]
pub struct Project {
    /// Modbus connections.
    #[serde(default)]
    pub connections: Vec<mb_engine::Connection>,
    /// Poll windows (rows are tags).
    #[serde(default)]
    pub polls: Vec<Poll>,
    /// Extra styling rules (accepted but not needed by the bridge).
    #[serde(default)]
    pub rules: Vec<mb_engine::Rule>,
    /// Computed (virtual) tags.
    #[serde(default)]
    pub computed: Vec<ComputedTag>,
}

/// Where a tag node finds its value in the snapshot.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct RowKey {
    pub conn: String,
    pub slave: u8,
    pub func: u8,
    pub addr: u16,
}

pub enum NodeKind {
    /// A poll row: value from the snapshot row map. `wr` is present when the
    /// row is client-writable (holding-register / coil polls) and carries the
    /// scaling needed to convert an OPC UA value back to raw registers.
    Tag {
        key: RowKey,
        is_bits: bool,
        wr: Option<WriteParams>,
    },
    /// A computed tag, matched by name.
    Calc {
        name: String,
    },
    /// Connection status variables.
    Online {
        conn: String,
    },
    Tx {
        conn: String,
    },
    Errors {
        conn: String,
    },
}

pub struct NodeRef {
    pub id: NodeId,
    pub kind: NodeKind,
}

/// Everything a client write needs to become a Modbus write.
pub struct WriteParams {
    pub fmt: Fmt,
    pub mult: f64,
    pub div: f64,
    pub offset: f64,
}

/// OPC UA client wrote a tag: unscale the value, resolve the engine poll
/// covering the address (coalescing may have folded the original poll into a
/// wider one) and enqueue a Modbus write (FC5 coil / FC6 register / FC16
/// multi-register for 32- and 64-bit formats).
struct TagWriter {
    engine: Arc<Engine>,
    conn: String,
    slave: u8,
    func: u8,
    addr: u16,
    fmt: Fmt,
    mult: f64,
    div: f64,
    offset: f64,
    is_bits: bool,
}

impl AttributeSetter for TagWriter {
    fn set(
        &mut self,
        _node_id: &NodeId,
        attribute_id: AttributeId,
        _index_range: NumericRange,
        data_value: DataValue,
    ) -> Result<(), StatusCode> {
        if attribute_id != AttributeId::Value {
            return Ok(());
        }
        let Some(v) = data_value.value else {
            return Err(StatusCode::BadTypeMismatch);
        };
        let scaled = match v {
            Variant::Double(x) => x,
            Variant::Float(x) => x as f64,
            Variant::Boolean(b) => {
                if b {
                    1.0
                } else {
                    0.0
                }
            }
            Variant::SByte(x) => x as f64,
            Variant::Byte(x) => x as f64,
            Variant::Int16(x) => x as f64,
            Variant::UInt16(x) => x as f64,
            Variant::Int32(x) => x as f64,
            Variant::UInt32(x) => x as f64,
            Variant::Int64(x) => x as f64,
            Variant::UInt64(x) => x as f64,
            _ => return Err(StatusCode::BadTypeMismatch),
        };
        let div = if self.div == 0.0 { 1.0 } else { self.div };
        let mult = if self.mult == 0.0 { 1.0 } else { self.mult };
        // inverse of `scaled = raw * mult / div + offset`
        let rawf = (scaled - self.offset) * div / mult;

        let snap = self.engine.snapshot();
        let poll = snap.polls.iter().find(|p| {
            p.conn_id == self.conn
                && p.func == self.func
                && p.slave == self.slave
                && self.addr >= p.start
                && self.addr < p.start.saturating_add(p.qty)
        });
        let Some(p) = poll else {
            return Err(StatusCode::BadNotWritable);
        };

        let res = if self.is_bits {
            self.engine
                .write(p.id, self.addr, if rawf >= 0.5 { 1 } else { 0 })
        } else {
            match self.fmt.regs_per_value() {
                1 => {
                    let raw: u16 = match self.fmt {
                        Fmt::I16 => {
                            rawf.round().clamp(i16::MIN as f64, i16::MAX as f64) as i16 as u16
                        }
                        Fmt::U16Swapped => {
                            let x = rawf.round().clamp(0.0, 65535.0) as u16;
                            (((x & 0xFF) << 8) | (x >> 8)) as u16
                        }
                        _ => rawf.round().clamp(0.0, 65535.0) as u16,
                    };
                    self.engine.write(p.id, self.addr, raw)
                }
                n => {
                    let mut words = encode_words(rawf, self.fmt, n);
                    words.resize(n, 0);
                    self.engine.write_multi(p.id, self.addr, words)
                }
            }
        };
        res.map_err(|_| StatusCode::BadNotWritable)
    }
}

/// Split an unscaled value into Modbus words in the byte order of `fmt`.
fn encode_words(rawf: f64, fmt: Fmt, regs: usize) -> Vec<u16> {
    let swap16 = |x: u16| -> u16 { (x << 8) | (x >> 8) };
    if fmt == Fmt::F64 {
        // four words, high word first (c[0]<<48 | c[1]<<32 | c[2]<<16 | c[3])
        let bits = rawf.to_bits();
        (0..4)
            .rev()
            .map(|i| ((bits >> (16 * i)) & 0xFFFF) as u16)
            .collect()
    } else if fmt.is_float() {
        let bits = (rawf as f32).to_bits();
        let (hi, lo) = ((bits >> 16) as u16, bits as u16);
        match fmt {
            Fmt::F32Cdab => vec![lo, hi],
            Fmt::F32Badc => vec![swap16(hi), swap16(lo)],
            Fmt::F32Dcba => vec![swap16(lo), swap16(hi)],
            _ => vec![hi, lo],
        }
    } else {
        // signed formats keep their sign through the two's-complement cast
        let bits: u64 = match fmt {
            Fmt::I32 => (rawf.round().clamp(i32::MIN as f64, i32::MAX as f64) as i32) as u32 as u64,
            Fmt::I64 => (rawf.round().clamp(i64::MIN as f64, i64::MAX as f64) as i64) as u64,
            _ => rawf.round().clamp(0.0, u64::MAX as f64) as u64,
        };
        (0..regs)
            .rev()
            .map(|i| ((bits >> (16 * i)) & 0xFFFF) as u16)
            .collect()
    }
}

/// Build the OPC UA server and its address space from the project. Returns
/// the server (not yet running), the node table for [`pump`] and a handle to
/// the address space so the caller can pump snapshots from any thread.
pub fn build_server_and_space(
    project: &Project,
    listen: &str,
    engine: &Arc<Engine>,
) -> (Server, Vec<NodeRef>, Arc<RwLock<AddressSpace>>) {
    let (host, port) = match listen.rsplit_once(':') {
        Some((h, p)) => (
            h.to_string(),
            p.parse().expect("--listen port must be numeric"),
        ),
        None => (listen.to_string(), 4840),
    };
    let server = ServerBuilder::new_anonymous("mbSCADA OPC bridge")
        .application_uri(crate::NAMESPACE_URI)
        .product_uri(crate::NAMESPACE_URI)
        .host_and_port(host, port)
        .discovery_urls(vec![format!("opc.tcp://{listen}/")])
        .server()
        .expect("valid server builder");
    let asp = server.address_space();

    let mut nodes: Vec<NodeRef> = Vec::new();
    {
        let mut sp = asp.write();
        let ns = sp
            .register_namespace(crate::NAMESPACE_URI)
            .expect("namespace not registered yet");
        let root = NodeId::new(ns, "mbscada");
        assert!(
            sp.add_folder_with_id(&root, "mbSCADA", "mbSCADA", &NodeId::objects_folder_id()),
            "root folder"
        );

        // One folder per readable poll, one variable per row.
        for p in project
            .polls
            .iter()
            .filter(|p| p.enabled && func_is_read(p.func))
        {
            let folder = NodeId::new(ns, format!("p{}", p.id));
            sp.add_folder_with_id(&folder, p.name.as_str(), p.name.as_str(), &root);
            let is_bits = p.is_bits();
            for addr in p.row_addrs() {
                let id = NodeId::new(ns, format!("p{}:{}", p.id, addr));
                let label = p.label_of(addr);
                let init = if is_bits {
                    Variant::Boolean(false)
                } else {
                    Variant::Double(0.0)
                };
                let mut var = Variable::new(&id, label.as_str(), label.as_str(), init);
                // Holding-register (FC3) and coil (FC1) rows accept client
                // writes; input areas (FC2/FC4) are read-only by design.
                let wr = if p.func == 1 || p.func == 3 {
                    var.set_writable(true);
                    var.set_value_setter(Arc::new(Mutex::new(TagWriter {
                        engine: engine.clone(),
                        conn: p.conn_id.clone(),
                        slave: p.slave,
                        func: p.func,
                        addr,
                        fmt: p.fmt,
                        mult: p.mult,
                        div: p.div,
                        offset: p.offset,
                        is_bits,
                    })));
                    Some(WriteParams {
                        fmt: p.fmt,
                        mult: p.mult,
                        div: p.div,
                        offset: p.offset,
                    })
                } else {
                    None
                };
                sp.add_variables(vec![var], &folder);
                nodes.push(NodeRef {
                    id,
                    kind: NodeKind::Tag {
                        key: RowKey {
                            conn: p.conn_id.clone(),
                            slave: p.slave,
                            func: p.func,
                            addr,
                        },
                        is_bits,
                        wr,
                    },
                });
            }
        }

        // Computed tags.
        if !project.computed.is_empty() {
            let folder = NodeId::new(ns, "calc");
            sp.add_folder_with_id(&folder, "Computed", "Computed", &root);
            for (i, c) in project.computed.iter().enumerate() {
                let id = NodeId::new(ns, format!("calc:{i}"));
                let var =
                    Variable::new(&id, c.name.as_str(), c.name.as_str(), Variant::Double(0.0));
                sp.add_variables(vec![var], &folder);
                nodes.push(NodeRef {
                    id,
                    kind: NodeKind::Calc {
                        name: c.name.clone(),
                    },
                });
            }
        }

        // Connection status.
        let status = NodeId::new(ns, "status");
        sp.add_folder_with_id(&status, "Status", "Status", &root);
        for c in &project.connections {
            let cf = NodeId::new(ns, format!("conn:{}", c.id));
            sp.add_folder_with_id(&cf, c.name.as_str(), c.name.as_str(), &status);
            for (suffix, kind) in [("online", 0u8), ("tx", 1), ("errors", 2)] {
                let id = NodeId::new(ns, format!("conn:{}:{}", c.id, suffix));
                let init = if kind == 0 {
                    Variant::Boolean(false)
                } else {
                    Variant::UInt64(0)
                };
                let var = Variable::new(&id, suffix, suffix, init);
                sp.add_variables(vec![var], &cf);
                nodes.push(NodeRef {
                    id,
                    kind: match kind {
                        0 => NodeKind::Online { conn: c.id.clone() },
                        1 => NodeKind::Tx { conn: c.id.clone() },
                        _ => NodeKind::Errors { conn: c.id.clone() },
                    },
                });
            }
        }
    }
    (server, nodes, asp)
}

/// Push one engine snapshot into the address space: tag values with quality
/// and source timestamps, computed tag results, connection counters.
pub fn pump(engine: &Engine, asp: &Arc<RwLock<AddressSpace>>, nodes: &[NodeRef]) {
    let snap = engine.snapshot();
    let mut row_by_key: HashMap<RowKey, &mb_engine::RowView> = HashMap::new();
    for pv in &snap.polls {
        for r in &pv.rows {
            row_by_key.insert(
                RowKey {
                    conn: pv.conn_id.clone(),
                    slave: pv.slave,
                    func: pv.func,
                    addr: r.addr,
                },
                r,
            );
        }
    }
    let conn_by_id: HashMap<&str, &mb_engine::ConnView> = snap
        .connections
        .iter()
        .map(|c| (c.id.as_str(), c))
        .collect();
    let calc_by_name: HashMap<&str, &mb_engine::ComputedView> =
        snap.computed.iter().map(|c| (c.name.as_str(), c)).collect();

    let mut sp = asp.write();
    let now = DateTime::now();
    for n in nodes {
        match &n.kind {
            NodeKind::Tag { key, is_bits, .. } => {
                let Some(r) = row_by_key.get(key) else {
                    continue;
                };
                let (variant, status) = match r.quality.as_str() {
                    "good" => {
                        let v = if *is_bits {
                            Variant::Boolean(r.raw != 0)
                        } else {
                            Variant::Double(r.scaled)
                        };
                        (v, StatusCode::Good)
                    }
                    "off" => (Variant::Double(0.0), StatusCode::UncertainLastUsableValue),
                    _ => (Variant::Double(0.0), StatusCode::BadCommunicationError),
                };
                if let Some(var) = sp.find_variable_mut_by_ref(&n.id) {
                    let _ = var.set_value_direct(variant, status, &now, &now);
                }
            }
            NodeKind::Calc { name } => {
                let Some(c) = calc_by_name.get(name.as_str()) else {
                    continue;
                };
                let status = if c.ok {
                    StatusCode::Good
                } else {
                    StatusCode::BadDataUnavailable
                };
                if let Some(var) = sp.find_variable_mut_by_ref(&n.id) {
                    let _ = var.set_value_direct(Variant::Double(c.value), status, &now, &now);
                }
            }
            NodeKind::Online { conn } => {
                if let Some(c) = conn_by_id.get(conn.as_str()) {
                    if let Some(var) = sp.find_variable_mut_by_ref(&n.id) {
                        let _ = var.set_value_direct(
                            Variant::Boolean(c.stats.online),
                            StatusCode::Good,
                            &now,
                            &now,
                        );
                    }
                }
            }
            NodeKind::Tx { conn } | NodeKind::Errors { conn } => {
                if let Some(c) = conn_by_id.get(conn.as_str()) {
                    let v = match &n.kind {
                        NodeKind::Tx { .. } => c.stats.tx,
                        _ => c.stats.errors,
                    };
                    if let Some(var) = sp.find_variable_mut_by_ref(&n.id) {
                        let _ =
                            var.set_value_direct(Variant::UInt64(v), StatusCode::Good, &now, &now);
                    }
                }
            }
        }
    }
}
