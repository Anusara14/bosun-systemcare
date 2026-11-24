//! Shared system code for Bosun SystemCare: live metrics, process data, the junk scanner and journaled cleaning.

pub mod clean;
pub mod format;
pub mod junk;
pub mod metrics;
pub mod paths;
pub mod procs;
pub mod registry;
pub mod security;
pub mod shortcuts;
