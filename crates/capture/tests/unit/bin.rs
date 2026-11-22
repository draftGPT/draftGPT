use super::*;
use clap::CommandFactory;

#[test]
fn cli_definition_is_valid() {
    Args::command().debug_assert();
}

#[test]
fn documented_invocations_parse() {
    for argv in [
        vec!["capture"],
        vec!["capture", "--out", "c", "auto"],
        vec!["capture", "record", "--label", "x", "--out", "c"],
        vec!["capture", "--wire", "false", "auto"],
        vec!["capture", "help-dump", "--file", "f.json"],
        vec!["capture", "convert-rcvolus", "in.json", "out.jsonl"],
    ] {
        Args::try_parse_from(&argv).unwrap_or_else(|e| panic!("{argv:?}: {e}"));
    }
}
