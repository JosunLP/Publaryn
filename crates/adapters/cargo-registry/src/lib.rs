//! Cargo alternative registry adapter for Publaryn.
//!
//! Implements the Cargo sparse index protocol (RFC 2789) and the
//! Cargo Web API (publish, yank, unyank, owners, search, download).

pub mod metadata;
pub mod name;
pub mod publish;
pub mod routes;
