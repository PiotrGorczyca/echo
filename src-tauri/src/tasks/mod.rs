//! Task Management Module
//! 
//! Provides repo-scoped task management following the Pointer model.
//! Tasks are stored as MARKDOWN FILES in each repository, not in a database.
//! SQLite is only used to track which repos to monitor.
//!
//! ## Architecture
//!
//! ```
//! Repository (on disk)
//! ├── src/
//! └── .echo/tasks.md   ← Tasks stored here as markdown
//!     or TODO.md
//! ```
//!
//! Echo reads/writes these markdown files directly.

pub mod models;
pub mod storage;
pub mod service;
pub mod markdown;

pub use models::*;
pub use service::*;
pub use markdown::{parse_task_file, write_task_file, find_task_file, TaskUpdate};






