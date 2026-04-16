//! Background job processing for Publaryn.
//!
//! Uses a PostgreSQL-backed job queue with `SELECT ... FOR UPDATE SKIP LOCKED`
//! for safe, horizontally-scalable concurrent processing. No additional
//! infrastructure beyond PostgreSQL is required.
//!
//! # Architecture
//!
//! - **JobQueue**: Enqueues jobs into the `background_jobs` table.
//! - **Worker**: Polls for claimable jobs, dispatches to handlers, manages
//!   retries and dead-lettering.
//! - **JobHandler** trait: Implement this to define processing logic for each
//!   job kind.

pub mod handler;
pub mod queue;
pub mod scanners;
pub mod worker;
