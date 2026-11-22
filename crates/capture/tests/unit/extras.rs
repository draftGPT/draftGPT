use super::*;

#[test]
fn one_full_sweep_per_client_launch() {
    let mut e = RecordingExtras::new();
    assert!(e.claim_sweep("https://127.0.0.1:49419"));
    assert!(!e.claim_sweep("https://127.0.0.1:49419"), "same launch");
    // A relaunched client gets a new port, so a new sweep.
    assert!(e.claim_sweep("https://127.0.0.1:51234"));
    assert!(!e.claim_sweep("https://127.0.0.1:51234"));
}
