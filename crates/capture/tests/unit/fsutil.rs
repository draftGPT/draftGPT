use super::*;

#[tokio::test]
async fn atomic_write_replaces_whole_file_and_cleans_up() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sub/x.help.json");
    write_atomic(&path, b"first").await.unwrap();
    write_atomic(&path, b"second").await.unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "second");
    let leftovers = std::fs::read_dir(dir.path().join("sub")).unwrap().count();
    assert_eq!(leftovers, 1, "no temp files left");
}

#[test]
fn temp_paths_are_unique_and_hidden() {
    let p = Path::new("/c/.help-cache/16.21.help.json");
    let (a, b) = (temp_path(p), temp_path(p));
    assert_ne!(a, b);
    assert!(a.file_name().unwrap().to_string_lossy().starts_with('.'));
    assert_eq!(a.parent(), p.parent());
}
