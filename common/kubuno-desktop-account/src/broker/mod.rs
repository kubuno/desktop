//! The local token broker (vskubuno `docs/DESKTOP-OFFLINE-SYNC.md` §9): the shell serves access tokens of the
//! accounts it owns; apps borrow them and never see a refresh token.

#[cfg(feature = "client")]
mod client;
pub mod proto;
#[cfg(feature = "server")]
mod server;
mod transport;

#[cfg(feature = "client")]
pub use client::{BrokerClient, BrokerError, BrokerTokenSource, EventStream};
#[cfg(feature = "server")]
pub use server::{BoundBroker, BrokerServer};
pub use transport::{BrokerEndpoint, ClientPolicy, ServerPolicy};
