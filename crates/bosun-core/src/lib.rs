//! Shared system code for Bosun SystemCare: live metrics, process data, the junk scanner and journaled cleaning.

pub mod apps;
pub mod clean;
pub mod drivers;
pub mod extensions;
pub mod finder;
pub mod format;
pub mod junk;
pub mod metrics;
pub mod paths;
pub mod procs;
pub mod registry;
pub mod security;
pub mod services;
pub mod shortcuts;
pub mod startup;
pub mod tasks;
pub mod turbo;
