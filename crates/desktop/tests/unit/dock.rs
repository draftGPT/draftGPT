use super::*;

fn client(x: i32, width: i32) -> ClientWindow {
    ClientWindow {
        x,
        y: 100,
        width,
        height: 720,
        fullscreen: false,
        monitor_left: 0,
        monitor_right: 1920,
    }
}

#[test]
fn docks_right_of_the_client_when_there_is_room() {
    assert_eq!(docked_rect(&client(200, 1280), 380), (1480, 100, 380, 720));
}

#[test]
fn docks_left_when_the_right_side_is_full() {
    assert_eq!(docked_rect(&client(600, 1280), 380), (220, 100, 380, 720));
}

#[test]
fn stays_right_when_neither_side_fits() {
    assert_eq!(docked_rect(&client(100, 1700), 380), (1800, 100, 380, 720));
}
