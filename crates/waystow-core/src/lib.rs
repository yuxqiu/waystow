//! Pure logic for WayStow: config, the shelf model, action specs and
//! templating, collision-safe naming, state machines and IPC types. No I/O
//! beyond reading config and action files.

pub mod action;
pub mod catcher;
pub mod config;
pub mod ipc;
pub mod mime;
pub mod model;
pub mod naming;
pub mod paths;
pub mod progress;
pub mod recent;
pub mod spring;
pub mod uri;
