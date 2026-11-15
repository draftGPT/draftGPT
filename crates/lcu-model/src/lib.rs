//! What the League client (LCU) sends, with no I/O: [`raw`] events, typed
//! [`types`], the [`recording`] file format, and the resources we use. Builds
//! for wasm32. The client side (finding the client, HTTPS/WebSocket, event
//! sources) is in the `lcu` crate, which re-exports everything here.

pub mod raw;
pub mod recording;
pub mod types;

pub use raw::{EventKind, RawEvent};

/// An LCU resource: the WebSocket event that delivers it and its REST URI.
/// An event can cover a whole URI prefix (the client lists some only at
/// plugin level), so consumers filter frames by `uri`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Resource {
    pub event: &'static str,
    pub uri: &'static str,
}

pub const SESSION: Resource = Resource {
    event: "OnJsonApiEvent_lol-champ-select_v1_session",
    uri: "/lol-champ-select/v1/session",
};

/// The same session under the team-builder plugin. Same type; which modes use
/// it rather than `SESSION` isn't documented, so both are recorded.
pub const TEAM_BUILDER_SESSION: Resource = Resource {
    // 16.21 lists no per-resource event for this session, only the prefix.
    event: "OnJsonApiEvent_lol-lobby-team-builder_champ-select_v1",
    uri: "/lol-lobby-team-builder/champ-select/v1/session",
};

pub const GAMEFLOW_PHASE: Resource = Resource {
    event: "OnJsonApiEvent_lol-gameflow_v1_gameflow-phase",
    uri: "/lol-gameflow/v1/gameflow-phase",
};

/// Champions offered to the local player when the session has
/// `allowSubsetChampionPicks` (pick one of a few, as in queue 3270). The
/// session never lists them; a bare array of champion IDs, sent at the start
/// of champ select. Only the team-builder plugin serves it (16.22).
pub const SUBSET_CHAMPION_LIST: Resource = Resource {
    event: TEAM_BUILDER_SESSION.event,
    uri: "/lol-lobby-team-builder/champ-select/v1/subset-champion-list",
};

/// Queue, ready check and dodge state. When champ select ends early,
/// `dodgeData.state` says why (`PartyDodged` when the local party left); the
/// session only disappears.
pub const MATCHMAKING: Resource = Resource {
    event: "OnJsonApiEvent_lol-lobby-team-builder_v1_matchmaking",
    uri: "/lol-lobby-team-builder/v1/matchmaking",
};

/// Resources the typed pipeline uses and main recordings contain: the champ
/// select session (under both plugins), the offered champions, matchmaking
/// (dodges) and the gameflow phase.
pub const CORE_RESOURCES: &[Resource] = &[
    SESSION,
    TEAM_BUILDER_SESSION,
    SUBSET_CHAMPION_LIST,
    MATCHMAKING,
    GAMEFLOW_PHASE,
];

// Plain `&str` copies, because match patterns can't use a struct field.
pub const SESSION_URI: &str = SESSION.uri;
pub const TEAM_BUILDER_SESSION_URI: &str = TEAM_BUILDER_SESSION.uri;
pub const SUBSET_CHAMPION_LIST_URI: &str = SUBSET_CHAMPION_LIST.uri;
pub const MATCHMAKING_URI: &str = MATCHMAKING.uri;
pub const GAMEFLOW_PHASE_URI: &str = GAMEFLOW_PHASE.uri;

/// True for either URI the champ select session is served under.
pub fn is_session_uri(uri: &str) -> bool {
    uri == SESSION_URI || uri == TEAM_BUILDER_SESSION_URI
}

/// Event types subscribed to for the wire capture: every JSON API resource
/// event plus the client's other event channels. `OnLog` (the client's internal
/// log stream) is left out; it's large and unrelated to drafting.
pub const WIRE_EVENTS: &[&str] = &[
    "OnJsonApiEvent",
    "OnLcdsEvent",
    "OnServiceProxyAsyncEvent",
    "OnServiceProxyUuidEvent",
    "OnRegionLocaleChanged",
    "OnCallback",
];
