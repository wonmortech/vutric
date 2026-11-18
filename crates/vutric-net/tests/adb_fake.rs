//! ADR-0070 without a phone: a fake `adb` (a shell or batch script) prints a recorded `adb devices -l` and logs
//! the arguments it was given, so the exact `reverse` and `am start` commands are checked.

use std::path::{Path, PathBuf};

use vutric_net::usb::adb::{Adb, State};

const LISTING: &str = "List of devices attached
R5CT1234ABC            device usb:1-1 product:o1sxxx model:SM_G991B device:o1s transport_id:1
0A1B2C3D               unauthorized usb:2-3 transport_id:3
";

fn fake_adb(dir: &Path) -> PathBuf {
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(dir.join("listing.txt"), LISTING).unwrap();
    let log = dir.join("log.txt");
    if cfg!(windows) {
        let path = dir.join("adb.cmd");
        let script = format!(
            "@echo off\r\necho %*>>\"{}\"\r\nif \"%1\"==\"devices\" type \"{}\"\r\n",
            log.display(),
            dir.join("listing.txt").display()
        );
        std::fs::write(&path, script).unwrap();
        path
    } else {
        let path = dir.join("adb");
        let script = format!(
            "#!/bin/sh\necho \"$@\" >> '{}'\n[ \"$1\" = devices ] && cat '{}'\nexit 0\n",
            log.display(),
            dir.join("listing.txt").display()
        );
        std::fs::write(&path, script).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        path
    }
}

#[test]
fn reverse_and_open_link_run_the_right_commands() {
    let dir = std::env::temp_dir().join(format!("vutric-adb-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let adb = Adb::at(fake_adb(&dir));

    let devices = adb.devices().expect("devices");
    assert_eq!(devices.len(), 2);
    let ready: Vec<_> = devices
        .iter()
        .filter(|d| d.state == State::Device && d.usb)
        .collect();
    assert_eq!(ready.len(), 1);
    assert_eq!(ready[0].model.as_deref(), Some("SM G991B"));

    adb.reverse(&ready[0].serial, 47123).expect("reverse");
    adb.open_link(&ready[0].serial, "vutric://pair?v=2&id=ab")
        .expect("open link");
    adb.remove_reverse(&ready[0].serial, 47123)
        .expect("remove reverse");

    let log = std::fs::read_to_string(dir.join("log.txt")).unwrap();
    let lines: Vec<&str> = log.lines().map(str::trim).collect();
    assert_eq!(lines[0], "devices -l");
    assert_eq!(lines[1], "-s R5CT1234ABC reverse tcp:47123 tcp:47123");
    assert!(
        lines[2].starts_with("-s R5CT1234ABC shell ")
            && lines[2]
                .contains("am start -a android.intent.action.VIEW -d 'vutric://pair?v=2&id=ab'"),
        "{}",
        lines[2]
    );
    assert_eq!(lines[3], "-s R5CT1234ABC reverse --remove tcp:47123");
    let _ = std::fs::remove_dir_all(&dir);
}
