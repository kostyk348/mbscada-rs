//! `mb-opc` — OPC UA bridge for mbSCADA: library + `mb-opc` binary.
//!
//! The library part ([`bridge`]) opens an mbSCADA project (the same JSON the
//! desktop app saves), builds an OPC UA address space from it and pumps
//! engine snapshots into it. The binary wires that up with a CLI.

pub mod bridge;

/// URI of the bridge's OPC UA namespace and application.
pub const NAMESPACE_URI: &str = "urn:mbscada:opc";
