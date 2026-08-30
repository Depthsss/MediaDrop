#![windows_subsystem = "windows"]

use mediadrop_installer_worker::{
    broker::{self, BrokerArgs},
    elevated::{self, ElevatedArgs},
    operation::WorkerOperation,
    security::SessionSecret,
};
use std::{collections::HashMap, ffi::OsString, path::PathBuf};

fn main() {
    std::process::exit(parse_and_run().unwrap_or(87) as i32);
}

fn parse_and_run() -> Result<u32, ()> {
    let mut arguments = std::env::args_os();
    let _executable = arguments.next();
    let mode = arguments.next().ok_or(())?;
    let values = parse_pairs(arguments.collect())?;
    #[cfg(feature = "installer-mode")]
    if mode == "--broker" {
        if values.len() != 1 {
            return Err(());
        }
        let session_dir = values.get("--session-dir").ok_or(())?;
        return Ok(broker::run(BrokerArgs {
            session_dir: PathBuf::from(session_dir),
            operation: WorkerOperation::Installer,
        }));
    }
    #[cfg(feature = "component-mode")]
    if mode == "--component-broker" {
        if values.len() != 1 {
            return Err(());
        }
        let session_dir = values.get("--session-dir").ok_or(())?;
        return Ok(broker::run(BrokerArgs {
            session_dir: PathBuf::from(session_dir),
            operation: WorkerOperation::Component,
        }));
    }
    #[cfg(feature = "installer-mode")]
    if mode == "--elevated-worker" {
        if values.len() != 5 {
            return Err(());
        }
        let text = |key: &str| values.get(key).and_then(|value| value.to_str()).ok_or(());
        let secret = SessionSecret::parse_hex(text("--secret")?).map_err(|_| ())?;
        let broker_pid = text("--broker-pid")?.parse().map_err(|_| ())?;
        return Ok(elevated::run(ElevatedArgs {
            pipe_name: text("--pipe")?.to_owned(),
            session_id: text("--session-id")?.to_owned(),
            secret,
            broker_pid,
            interactive_user_sid: text("--interactive-user-sid")?.to_owned(),
            operation: WorkerOperation::Installer,
        }));
    }
    #[cfg(feature = "component-mode")]
    if mode == "--component-elevated-worker" {
        if values.len() != 5 {
            return Err(());
        }
        let text = |key: &str| values.get(key).and_then(|value| value.to_str()).ok_or(());
        let secret = SessionSecret::parse_hex(text("--secret")?).map_err(|_| ())?;
        let broker_pid = text("--broker-pid")?.parse().map_err(|_| ())?;
        return Ok(elevated::run(ElevatedArgs {
            pipe_name: text("--pipe")?.to_owned(),
            session_id: text("--session-id")?.to_owned(),
            secret,
            broker_pid,
            interactive_user_sid: text("--interactive-user-sid")?.to_owned(),
            operation: WorkerOperation::Component,
        }));
    }
    Err(())
}

fn parse_pairs(arguments: Vec<OsString>) -> Result<HashMap<String, OsString>, ()> {
    if !arguments.len().is_multiple_of(2) {
        return Err(());
    }
    let mut values = HashMap::new();
    for pair in arguments.as_chunks::<2>().0 {
        let key = pair[0].to_str().ok_or(())?.to_owned();
        if !key.starts_with("--") || values.insert(key, pair[1].clone()).is_some() {
            return Err(());
        }
    }
    Ok(values)
}
