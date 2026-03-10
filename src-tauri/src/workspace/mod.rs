//! Workspace Context Module
//!
//! Direct context capture from file system and git CLI.
//! NO MCPs - Claude Code has those. Echo just gathers context to pass along.

pub mod context;
pub mod detection;
pub mod git;
pub mod watcher;

pub use context::WorkspaceContext;
pub use detection::WorkspaceDetector;
pub use watcher::CursorHandoffWatcher;
// Git module used internally by context



