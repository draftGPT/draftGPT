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
fn fearless_champions_are_neither_picked_nor_banned() {
    let mut d = draft();
    d.fearless = vec![2, 4];
    let r = StatsRecommender::new(stats()).recommend(&d);
    assert!(
        r.picks
            .iter()
            .chain(&r.bans)
            .all(|x| ![2, 4].contains(&x.champion))
    );
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
