use super::*;
use clap::CommandFactory;

#[test]
fn cli_definition_is_valid() {
    Args::command().debug_assert();
}

#[test]
fn bundled_stats_parse() {
    StatsRecommender::from_json(BUNDLED_STATS).unwrap();
}
