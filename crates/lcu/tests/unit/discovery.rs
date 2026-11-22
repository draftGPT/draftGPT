use super::*;

#[test]
fn lockfile_parses() {
    let c = parse_lockfile("LeagueClient:12345:54321:s3cr3t-Token_x:https\n").unwrap();
    assert_eq!(c.port, 54321);
    assert_eq!(c.password, "s3cr3t-Token_x");
    assert_eq!(c.pid, Some(12345));
}

#[test]
fn lockfile_rejects_malformed() {
    assert!(matches!(
        parse_lockfile("LeagueClient:1:2:pw"),
        Err(DiscoveryError::MalformedLockfile(_))
    ));
    assert!(matches!(
        parse_lockfile("LeagueClient:1:notaport:pw:https"),
        Err(DiscoveryError::MalformedLockfile(_))
    ));
    assert!(matches!(
        parse_lockfile("LeagueClient:1:2::https"),
        Err(DiscoveryError::MalformedLockfile(_))
    ));
    assert!(parse_lockfile("").is_err());
}

#[test]
fn process_args_parse() {
    let args = [
        r"C:\Riot Games\League of Legends\LeagueClientUx.exe",
        "--riotclient-auth-token=other",
        "\"--remoting-auth-token=abcDEF123\"",
        "--app-port=61234",
        "--app-pid=999",
    ];
    let c = parse_process_args(args).unwrap();
    assert_eq!(c.port, 61234);
    assert_eq!(c.password, "abcDEF123");
    assert_eq!(c.pid, Some(999));
}

#[test]
fn process_args_missing_fields() {
    assert!(parse_process_args(["--app-port=1"]).is_none());
    assert!(parse_process_args(["--remoting-auth-token=x"]).is_none());
    assert!(parse_process_args(["--app-port=x", "--remoting-auth-token=y"]).is_none());
}

#[test]
fn debug_hides_password() {
    let c = parse_lockfile("LeagueClient:1:2:hunter2:https").unwrap();
    assert!(!format!("{c:?}").contains("hunter2"));
}

#[test]
fn auth_header_is_basic_riot() {
    let c = parse_lockfile("LeagueClient:1:2:pw:https").unwrap();
    // base64("riot:pw")
    assert_eq!(c.auth_header(), "Basic cmlvdDpwdw==");
}
