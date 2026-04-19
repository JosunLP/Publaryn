//! RubyGems adapter.
//!
//! Implements a RubyGems-compatible read and write surface:
//! metadata reads, gem file downloads, and `gem push` / yank.

pub mod gemspec;
pub mod metadata;
pub mod name;
pub mod publish;
pub mod routes;
