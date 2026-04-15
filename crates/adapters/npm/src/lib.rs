//! # npm Registry Protocol Adapter
//!
//! Implements the npm registry HTTP protocol as understood by the npm CLI,
//! Yarn, pnpm, and Bun. These handlers translate native npm wire-format
//! requests into operations on the shared Publaryn domain model (PostgreSQL
//! metadata + S3 artifact storage) and respond with npm-compatible JSON.
//!
//! ## Mounted endpoints
//!
//! All routes are relative to the mount prefix (e.g. `/npm`):
//!
//! | Method | Path | Description |
//! |--------|------|-------------|
//! | `GET`  | `/:package` | Package metadata (packument) |
//! | `PUT`  | `/:package` | Publish a new version |
//! | `GET`  | `/:package/-/:filename` | Download tarball |
//! | `GET`  | `-/v1/search` | Search packages |
//! | `GET`  | `-/package/:package/dist-tags` | List dist-tags |
//! | `PUT`  | `-/package/:package/dist-tags/:tag` | Set dist-tag |
//! | `DELETE` | `-/package/:package/dist-tags/:tag` | Remove dist-tag |

pub mod metadata;
pub mod name;
pub mod publish;
pub mod routes;
