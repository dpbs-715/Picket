//! Controlled external IDE launcher.
//! Resolves real worktree paths, exports `HEAD:<path>` / `:<path>` / conflict stages to
//! read-only temporary snapshots (never passing Git object refs directly to IDEs),
//! and invokes `idea`, `rustrover`, `webstorm`, `code`, or `cursor` via argv without shell interpolation.

mod launcher;
mod open_flow;
mod snapshot;

pub use launcher::EditorLauncher;
