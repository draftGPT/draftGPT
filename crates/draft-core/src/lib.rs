//! The draft as every frontend sees it. Live and replayed sessions go through
//! [`reducer`] into a [`DraftState`]; the web drafter will build one from
//! manual picks. Everything downstream (recommendations, UIs) only sees
//! `DraftState`.
//!
//! No I/O, builds for wasm32, depends on `lcu-model` only.

pub mod reducer;
pub mod state;

pub use reducer::{Tracker, from_session};
pub use state::{Ban, Bans, ChampionId, DraftState, Phase, Position, Slot, Timer, Turn, TurnKind};
