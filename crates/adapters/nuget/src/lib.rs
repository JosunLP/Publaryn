//! NuGet V3 protocol adapter for Publaryn.
//!
//! Implements the NuGet V3 Server API including service index, package
//! publish, flat container (download), registration (metadata), and search.

pub mod metadata;
pub mod name;
pub mod nuspec;
pub mod publish;
pub mod routes;
