//! Measurement.
//!
//! `bit-cli` is pointed at the operator's own infrastructure to answer: is my
//! server serving, how fast, to how many peers, with what latency, and where
//! does it fall over. That makes `bench` a deliverable held to the same
//! standard as the download path, not a side feature.
//!
//! Three parts:
//!
//! - [`report`] is the shape of a result. One envelope for every subcommand,
//!   carrying the environment it was taken in, the arguments, the time series,
//!   and the summary.
//! - [`recorder`] collects a measurement while it runs. Latency goes into a
//!   histogram, so percentiles cost a fixed amount of memory.
//! - [`render`] writes a report out as JSON, NDJSON, CSV, or text.
//!
//! The measurement drivers live beside them: [`webseed`] measures HTTP
//! sources, and [`disk`] measures what the payload file costs when several
//! receive paths write into it at once.

pub mod disk;
pub mod probe;
pub mod recorder;
pub mod render;
pub mod report;
pub mod swarm;
pub mod webseed;

pub use recorder::{Observation, Recorder};
pub use render::{Format, render};
pub use report::{Build, Environment, Kind, Parameters, Report, Summary, Target, compare};
