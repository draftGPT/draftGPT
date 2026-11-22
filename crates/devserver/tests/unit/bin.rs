use super::*;
use clap::CommandFactory;

#[test]
fn cli_definition_is_valid() {
    Args::command().debug_assert();
}
