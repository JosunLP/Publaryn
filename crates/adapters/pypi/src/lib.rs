//! # PyPI Simple API adapter
//!
//! Implements a subset of the PyPI Simple Repository API and legacy upload API using the
//! shared Publaryn package, release, and artifact domain data.
//!
//! The current slice supports:
//!
//! - project index responses at `/simple/`
//! - project detail responses at `/simple/<project>/`
//! - Twine-compatible uploads at `/legacy/`
//! - both HTML and JSON serializations via content negotiation
//! - distribution downloads for published, deprecated, and yanked releases
//! - direct reads of unlisted packages and authenticated reads of private or
//!   organization-internal packages

pub mod name;
pub mod routes;
pub mod simple;
pub mod upload;
