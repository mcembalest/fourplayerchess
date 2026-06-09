//! fpc-core: rules engine for chess.com-style 4-player free-for-all chess.
//! Ported from game.js (the human-facing browser UI) and kept in lockstep with
//! it via the differential test in tests/diff.rs.

mod features;
mod movegen;
mod state;
mod types;

pub use features::*;
pub use movegen::*;
pub use state::*;
pub use types::*;
