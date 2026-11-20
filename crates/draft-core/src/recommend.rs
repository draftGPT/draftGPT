//! Recommendations from a [`DraftState`] and per-patch statistics.
//!
//! [`Recommender`] is the interface every frontend calls; [`StatsRecommender`]
//! scores champions from a [`Stats`] file (`data/stats/<patch>.json`). Until
//! the real stats store exists those files are mock data
//! (`tools/mock_stats.py`), so the numbers mean nothing yet; the shape and the
//! flow are what matter.
//!
//! Scores are estimated win rates (0..1): the champion's win rate in the
//! position, plus its matchup deltas against locked enemies, plus synergy
//! deltas with locked allies. Reasons list the biggest contributions.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::state::{ChampionId, DraftState, Position};

/// How many picks and bans a recommendation lists.
pub const PICKS: usize = 10;
pub const BANS: usize = 6;
/// Reasons per recommendation, biggest first.
const REASONS: usize = 3;

pub trait Recommender {
    fn recommend(&self, draft: &DraftState) -> Recommendations;
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct Recommendations {
    /// Picks for the local player (or, for spectators and unknown positions,
    /// the best champions overall), best first.
    pub picks: Vec<Recommendation>,
    /// Champions worth banning, most dangerous first.
    pub bans: Vec<Recommendation>,
    /// The position the picks are for, if known.
    pub position: Option<Position>,
    pub patch: String,
    /// The stats are mock data.
    pub mock: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct Recommendation {
    pub champion: ChampionId,
    /// Picks: estimated win rate, 0..1. Bans: a ranking value (win rate plus
    /// threat and ban rate), only meaningful for ordering.
    pub score: f64,
    pub reasons: Vec<Reason>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Reason {
    /// Base win rate in the position.
    #[serde(rename_all = "camelCase")]
    WinRate { position: Position, win_rate: f64 },
    /// Good (delta > 0) or bad matchup against a locked enemy.
    #[serde(rename_all = "camelCase")]
    Matchup { enemy: ChampionId, delta: f64 },
    /// Plays well (delta > 0) or badly with a locked ally.
    #[serde(rename_all = "camelCase")]
    Synergy { ally: ChampionId, delta: f64 },
    /// Banned often.
    #[serde(rename_all = "camelCase")]
    BanRate { ban_rate: f64 },
    /// Beats one of your team's champions (bans).
    #[serde(rename_all = "camelCase")]
    Threat { ally: ChampionId, delta: f64 },
}

/// One patch's statistics: the file format of `data/stats/<patch>.json`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Stats {
    pub patch: String,
    #[serde(default)]
    pub mock: bool,
    pub champions: Vec<ChampionStats>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ChampionStats {
    pub id: ChampionId,
    pub ban_rate: f64,
    pub positions: Vec<PositionStats>,
    /// Win-rate change against these enemies (positive: this champion wins).
    pub matchups: Vec<Delta>,
    /// Win-rate change when on a team with these champions.
    pub synergies: Vec<Delta>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PositionStats {
    pub position: Position,
    pub win_rate: f64,
    pub pick_rate: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Delta {
    pub champion: ChampionId,
    pub delta: f64,
}

impl ChampionStats {
    fn in_position(&self, p: Position) -> Option<&PositionStats> {
        self.positions.iter().find(|s| s.position == p)
    }

    /// The position it's played in most.
    fn main_position(&self) -> Option<&PositionStats> {
        self.positions
            .iter()
            .max_by(|a, b| a.pick_rate.total_cmp(&b.pick_rate))
    }

    fn matchup(&self, enemy: ChampionId) -> f64 {
        find(&self.matchups, enemy)
    }

    fn synergy(&self, ally: ChampionId) -> f64 {
        find(&self.synergies, ally)
    }
}

fn find(deltas: &[Delta], champion: ChampionId) -> f64 {
    deltas
        .iter()
        .find(|d| d.champion == champion)
        .map_or(0.0, |d| d.delta)
}

/// Scores champions from one patch's [`Stats`].
#[derive(Debug, Clone)]
pub struct StatsRecommender {
    patch: String,
    mock: bool,
    champions: HashMap<ChampionId, ChampionStats>,
}

impl StatsRecommender {
    pub fn new(stats: Stats) -> Self {
        Self {
            patch: stats.patch,
            mock: stats.mock,
            champions: stats.champions.into_iter().map(|c| (c.id, c)).collect(),
        }
    }

    /// Parses a stats file.
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json).map(Self::new)
    }

    fn picks(
        &self,
        d: &DraftState,
        position: Option<Position>,
        taken: &[ChampionId],
    ) -> Vec<Recommendation> {
        let enemies: Vec<ChampionId> = d.enemy.iter().filter_map(|s| s.champion).collect();
        let allies: Vec<ChampionId> = d
            .ally
            .iter()
            .filter(|s| !s.is_local)
            .filter_map(|s| s.champion)
            .collect();
        // ARAM and pick-one-of-few modes: only what you can actually take.
        let choices: &[ChampionId] = if d.offered.is_empty() {
            &d.bench
        } else {
            &d.offered
        };
        let mut out: Vec<Recommendation> = self
            .champions
            .values()
            .filter(|c| !taken.contains(&c.id))
            .filter(|c| choices.is_empty() || choices.contains(&c.id))
            .filter_map(|c| {
                let base = match position {
                    Some(p) => c.in_position(p)?,
                    None => c.main_position()?,
                };
                let mut reasons = vec![Reason::WinRate {
                    position: base.position,
                    win_rate: base.win_rate,
                }];
                let mut extra: Vec<Reason> = enemies
                    .iter()
                    .map(|&e| Reason::Matchup {
                        enemy: e,
                        delta: c.matchup(e),
                    })
                    .chain(allies.iter().map(|&a| Reason::Synergy {
                        ally: a,
                        delta: c.synergy(a),
                    }))
                    .filter(|r| delta(r) != 0.0)
                    .collect();
                let score = base.win_rate + extra.iter().map(delta).sum::<f64>();
                extra.sort_by(|a, b| delta(b).abs().total_cmp(&delta(a).abs()));
                reasons.extend(extra);
                reasons.truncate(REASONS);
                Some(Recommendation {
                    champion: c.id,
                    score,
                    reasons,
                })
            })
            .collect();
        sort_and_cut(&mut out, PICKS);
        out
    }

    fn bans(&self, d: &DraftState, taken: &[ChampionId]) -> Vec<Recommendation> {
        // Your team's champions, hovered ones included: bans protect them.
        // They're also never suggested: you don't ban your teammate's hover.
        let allies: Vec<ChampionId> = d.ally.iter().filter_map(|s| s.shown()).collect();
        // A champion a teammate is already hovering to ban needs no second ban.
        let hovered: Vec<ChampionId> = d.bans.ally.iter().filter_map(|b| b.hover).collect();
        let mut out: Vec<Recommendation> = self
            .champions
            .values()
            .filter(|c| {
                !taken.contains(&c.id) && !allies.contains(&c.id) && !hovered.contains(&c.id)
            })
            .filter_map(|c| {
                let best = c.main_position()?;
                let mut threats: Vec<Reason> = allies
                    .iter()
                    .map(|&a| Reason::Threat {
                        ally: a,
                        delta: c.matchup(a),
                    })
                    .filter(|r| delta(r) > 0.0)
                    .collect();
                let threat: f64 = threats.iter().map(delta).sum();
                let score = best.win_rate + threat + c.ban_rate * 0.2;
                threats.sort_by(|a, b| delta(b).total_cmp(&delta(a)));
                let mut reasons = vec![
                    Reason::WinRate {
                        position: best.position,
                        win_rate: best.win_rate,
                    },
                    Reason::BanRate {
                        ban_rate: c.ban_rate,
                    },
                ];
                reasons.splice(0..0, threats);
                reasons.truncate(REASONS);
                Some(Recommendation {
                    champion: c.id,
                    score,
                    reasons,
                })
            })
            .collect();
        sort_and_cut(&mut out, BANS);
        out
    }
}

impl Recommender for StatsRecommender {
    fn recommend(&self, d: &DraftState) -> Recommendations {
        let taken: Vec<ChampionId> = d.unavailable().collect();
        let position = d.local().and_then(|s| s.position);
        Recommendations {
            picks: self.picks(d, position, &taken),
            bans: self.bans(d, &taken),
            position,
            patch: self.patch.clone(),
            mock: self.mock,
        }
    }
}

fn delta(r: &Reason) -> f64 {
    match r {
        Reason::Matchup { delta, .. }
        | Reason::Synergy { delta, .. }
        | Reason::Threat { delta, .. } => *delta,
        Reason::WinRate { .. } | Reason::BanRate { .. } => 0.0,
    }
}

/// Best first; ties by champion ID so the order is stable.
fn sort_and_cut(recs: &mut Vec<Recommendation>, n: usize) {
    recs.sort_by(|a, b| {
        b.score
            .total_cmp(&a.score)
            .then(a.champion.cmp(&b.champion))
    });
    recs.truncate(n);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{Ban, Slot};

    fn stats() -> Stats {
        let champ =
            |id, pos, wr, matchups: Vec<(i64, f64)>, synergies: Vec<(i64, f64)>| ChampionStats {
                id,
                ban_rate: 0.1,
                positions: vec![PositionStats {
                    position: pos,
                    win_rate: wr,
                    pick_rate: 0.05,
                }],
                matchups: matchups
                    .into_iter()
                    .map(|(champion, delta)| Delta { champion, delta })
                    .collect(),
                synergies: synergies
                    .into_iter()
                    .map(|(champion, delta)| Delta { champion, delta })
                    .collect(),
            };
        Stats {
            patch: "16.22".into(),
            mock: true,
            champions: vec![
                champ(1, Position::Middle, 0.50, vec![(9, 0.04)], vec![]),
                champ(2, Position::Middle, 0.52, vec![(9, -0.04)], vec![]),
                champ(3, Position::Middle, 0.49, vec![], vec![(7, 0.05)]),
                champ(4, Position::Top, 0.60, vec![], vec![]),
                champ(5, Position::Middle, 0.55, vec![], vec![]),
                champ(9, Position::Middle, 0.51, vec![(1, 0.03)], vec![]),
            ],
        }
    }

    fn slot(cell: i64, local: bool, pos: Option<Position>, champ: Option<i64>) -> Slot {
        Slot {
            cell_id: cell,
            is_local: local,
            position: pos,
            champion: champ,
            ..Slot::default()
        }
    }

    fn draft() -> DraftState {
        DraftState {
            ally: vec![
                slot(0, true, Some(Position::Middle), None),
                slot(1, false, None, Some(7)),
            ],
            enemy: vec![slot(5, false, None, Some(9))],
            bans: crate::state::Bans {
                ally: vec![Ban {
                    champion: Some(5),
                    cell_id: Some(0),
                    hover: None,
                }],
                enemy: vec![],
            },
            ..DraftState::default()
        }
    }

    #[test]
    fn picks_fit_the_position_skip_taken_and_add_matchups_and_synergy() {
        let r = StatsRecommender::new(stats()).recommend(&draft());
        let ids: Vec<i64> = r.picks.iter().map(|p| p.champion).collect();
        // 4 is top, 5 is banned, 9 is picked by the enemy.
        assert_eq!(ids, vec![1, 3, 2]);
        let first = &r.picks[0];
        assert!((first.score - 0.54).abs() < 1e-9, "{}", first.score);
        assert!(first.reasons.contains(&Reason::Matchup {
            enemy: 9,
            delta: 0.04
        }));
        assert!(r.picks[1].reasons.contains(&Reason::Synergy {
            ally: 7,
            delta: 0.05
        }));
        assert_eq!(r.position, Some(Position::Middle));
        assert!(r.mock);
    }

    #[test]
    fn bans_prefer_threats_to_your_champions_and_skip_taken() {
        let mut d = draft();
        d.ally[0].hover = Some(1);
        let r = StatsRecommender::new(stats()).recommend(&d);
        assert!(r.bans.iter().all(|b| ![5, 9].contains(&b.champion)));
        assert_eq!(r.bans[0].champion, 4, "highest win rate");
        let threat = r.bans.iter().find(|b| {
            b.reasons
                .iter()
                .any(|x| matches!(x, Reason::Threat { ally: 1, .. }))
        });
        assert!(threat.is_none(), "9 threatens 1 but is already picked");
    }

    #[test]
    fn a_ban_a_teammate_hovers_isnt_suggested_again() {
        let mut d = draft();
        d.bans.ally.push(Ban {
            champion: None,
            cell_id: Some(1),
            hover: Some(4),
        });
        let r = StatsRecommender::new(stats()).recommend(&d);
        assert!(r.bans.iter().all(|b| b.champion != 4));
        assert!(!d.unavailable().any(|c| c == 4), "not banned yet");
    }

    #[test]
    fn without_a_position_picks_use_each_champions_main_position() {
        let mut d = draft();
        d.ally[0].position = None;
        let r = StatsRecommender::new(stats()).recommend(&d);
        assert_eq!(r.picks[0].champion, 4);
        assert_eq!(r.position, None);
    }

    #[test]
    fn the_bundled_mock_stats_load_and_recommend() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../data/stats/16.22.json");
        let rec = StatsRecommender::from_json(&std::fs::read_to_string(path).unwrap()).unwrap();
        let r = rec.recommend(&draft());
        assert_eq!(r.picks.len(), PICKS);
        assert_eq!(r.bans.len(), BANS);
        assert!(r.mock && r.patch == "16.22");
        assert!(r.picks.windows(2).all(|w| w[0].score >= w[1].score));
    }

    #[test]
    fn teammates_hovers_are_never_suggested_as_bans() {
        let rec = StatsRecommender::new(stats());
        let mut d = draft();
        let top_ban = rec.recommend(&d).bans[0].champion;
        d.ally[1].champion = None;
        d.ally[1].hover = Some(top_ban);
        assert!(rec.recommend(&d).bans.iter().all(|b| b.champion != top_ban));
    }

    #[test]
    fn aram_bench_and_offers_limit_the_picks() {
        let rec = StatsRecommender::new(stats());
        let mut d = draft();
        d.ally[0].position = None;
        d.bench = vec![2, 3];
        let ids = |d: &DraftState| -> Vec<i64> {
            rec.recommend(d).picks.iter().map(|p| p.champion).collect()
        };
        assert_eq!(ids(&d), vec![3, 2]);
        d.offered = vec![1];
        assert_eq!(ids(&d), vec![1], "the offer wins over the bench");
    }
}
