//! Read-only access to the running League client (LCU): finding it
//! ([`discovery`]), HTTPS/WebSocket ([`transport`]) and event sources
//! ([`source`]).
//!
//! The data model (raw events, typed resources, recording format, resource
//! constants) lives in `lcu-model` and is re-exported here, so `lcu::types`,
//! `lcu::RawEvent` etc. work for users of this crate.

pub mod discovery;
pub mod source;
pub mod transport;

pub use lcu_model::*;

pub use discovery::Credentials;
pub use source::{
    ChannelSource, ClientInfo, EventSource, Live, LiveHandle, SourceEvent, SourceStream, WireFrame,
};
pub use transport::http::LcuHttp;
