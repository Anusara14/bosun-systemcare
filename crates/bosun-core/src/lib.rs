//! Shared system code for Bosun SystemCare: live metrics, process data, the junk scanner and journaled cleaning.

pub mod apps;
pub mod clean;
pub mod disks;
pub mod drivers;
pub mod elevate;
pub mod extensions;
pub mod finder;
pub mod format;
pub mod homepage;
pub mod junk;
pub mod metrics;
pub mod paths;
pub mod privacy;
pub mod procs;
pub mod registry;
pub mod rescue;
pub mod restore_point;
pub mod security;
pub mod services;
pub mod shortcuts;
pub mod startup;
pub mod tasks;
pub mod turbo;
pub mod tweaks;
pub mod updater;
