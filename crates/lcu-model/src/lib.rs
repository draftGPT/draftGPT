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

/// Clash tournaments the local player is in: `[{state, tournamentId,
/// rosterId, bracketId}]`. `state` is `UPCOMING|LOCK_IN|SCOUTING|IN_GAME|
/// RESULTS|IDLE` (16.23 spec).
pub const CLASH_TOURNAMENT_SUMMARY: Resource = Resource {
    event: "OnJsonApiEvent_lol-clash_v1_tournament-summary",
    uri: "/lol-clash/v1/tournament-summary",
};

/// `[{tournamentId, state, currentPhaseId, nextPhaseId, nextStateChangeTime,
/// numRemainingPeriods}]`.
pub const CLASH_TOURNAMENT_STATE_INFO: Resource = Resource {
    event: "OnJsonApiEvent_lol-clash_v1_tournament-state-info",
    uri: "/lol-clash/v1/tournament-state-info",
};

/// The local player's Clash data (tier, positions, tickets).
pub const CLASH_PLAYER: Resource = Resource {
    event: "OnJsonApiEvent_lol-clash_v1_player",
    uri: "/lol-clash/v1/player",
};

/// Clash resources with their own event, snapshotted on connect like the core
/// ones. The ones that take IDs (bracket, rosters) have no event of their own;
/// capture fetches them while a Clash recording runs (`capture::clash`).
pub const CLASH_RESOURCES: &[Resource] = &[
    CLASH_TOURNAMENT_SUMMARY,
    CLASH_TOURNAMENT_STATE_INFO,
    CLASH_PLAYER,
];

/// Further Clash events subscribed to when there's no wire capture (with it,
/// every event arrives anyway).
pub const CLASH_EVENTS: &[&str] = &[
    "OnJsonApiEvent_lol-clash_v1_tournament",
    "OnJsonApiEvent_lol-clash_v1_ready",
    "OnJsonApiEvent_lol-clash_v1_checkin-allowed",
    "OnJsonApiEvent_lol-clash_v1_currentTournamentIds",
    "OnJsonApiEvent_lol-clash_v1_simple-state-flags",
];

/// URIs recorded besides the core resources: Clash state, brackets, rosters
/// and scouting. A trailing `/` means the whole prefix. Left out: chat rosters
/// (chat), tournament history and lists (large, not about the current match),
/// server time (ticks), icons and config.
const CLASH_RECORDED: &[&str] = &[
    "/lol-clash/v1/tournament-summary",
    "/lol-clash/v1/tournament-state-info",
    "/lol-clash/v1/tournament/",
    "/lol-clash/v1/player",
    "/lol-clash/v1/ready",
    "/lol-clash/v1/checkin-allowed",
    "/lol-clash/v1/currentTournamentIds",
    "/lol-clash/v1/simple-state-flags",
    "/lol-clash/v1/bracket/",
    "/lol-clash/v1/roster/",
    "/lol-clash/v1/scouting/",
    "/lol-clash/v1/thirdparty/team-data",
    "/lol-clash/v1/game-end",
];

pub fn is_clash_uri(uri: &str) -> bool {
    uri.starts_with("/lol-clash/")
}

/// True for the Clash URIs that main recordings contain.
pub fn is_recorded_clash_uri(uri: &str) -> bool {
    CLASH_RECORDED.iter().any(|p| match p.strip_suffix('/') {
        Some(prefix) => uri
            .strip_prefix(prefix)
            .is_some_and(|rest| rest.starts_with('/')),
        None => uri == *p,
    })
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recorded_clash_uris() {
        for uri in [
            "/lol-clash/v1/tournament-summary",
            "/lol-clash/v1/tournament/3456/player",
            "/lol-clash/v1/bracket/123",
            "/lol-clash/v1/roster/abc-def",
            "/lol-clash/v1/roster/abc-def/stats",
            "/lol-clash/v1/scouting/matchhistory",
            "/lol-clash/v1/thirdparty/team-data",
        ] {
            assert!(is_recorded_clash_uri(uri), "{uri}");
        }
        for uri in [
            "/lol-clash/v1/player/chat-rosters",
            "/lol-clash/v1/historyandwinners",
            "/lol-clash/v1/all-tournaments",
            "/lol-clash/v1/time",
            "/lol-clash/v1/tournament-summary-x",
            "/lol-clash/v1/bracket",
            SESSION_URI,
        ] {
            assert!(!is_recorded_clash_uri(uri), "{uri}");
        }
    }
}
