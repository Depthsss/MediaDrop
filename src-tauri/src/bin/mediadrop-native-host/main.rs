mod framing;
mod windows;

fn self_test() -> bool {
    let payload = br#"{"command":"hello"}"#;
    let mut bytes = Vec::new();
    framing::write_frame(&mut bytes, payload, 64).is_ok()
        && framing::read_frame(&mut std::io::Cursor::new(bytes), 64)
            .ok()
            .flatten()
            .as_deref()
            == Some(payload)
}

fn main() {
    if std::env::args_os().nth(1).as_deref() == Some(std::ffi::OsStr::new("--self-test")) {
        if !self_test() {
            std::process::exit(1);
        }
        return;
    }
    if windows::run().is_err() {
        eprintln!("mediadrop-native-host: bridge_failed");
        std::process::exit(1);
    }
}
