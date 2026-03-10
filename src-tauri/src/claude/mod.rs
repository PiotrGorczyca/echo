//! Claude Code Integration Module
//!
//! Echo as a thin sidecar - builds prompts and invokes Claude Code.
//! Claude Code does the actual agent work.
//!
//! ## Components
//!
//! - `prompt`: Builds rich prompts with task and workspace context
//! - `invoke`: Methods to invoke Claude Code (terminal, headless, clipboard)
//! - `api_server`: Local HTTP API for Claude Code to call back to Echo

pub mod prompt;
pub mod invoke;
pub mod api_server;







