//! Conjure's MCP surface: parameter and result payloads plus the rmcp server.

pub mod params;
pub mod results;
pub mod server;

pub use server::{ConjureMcp, check_loopback, serve_http, serve_stdio};
