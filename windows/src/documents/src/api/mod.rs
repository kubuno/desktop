//! Talking to the server.
//!
//! [`client`] knows the routes and nothing about when to call them, [`session`] knows when — which
//! is where every rule that protects the user's work lives —, [`remote`] plugs the two together
//! with the crash journal, and [`live`] runs a session on its own thread for an open window.

#![allow(dead_code)]

pub mod client;
pub mod live;
pub mod remote;
pub mod session;
