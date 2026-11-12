//! `vesperd stage` and `vesperd store list|rm` (ADR 0007 section 1, slice 5): one `VESPERD` exchange with the
//! System VM agent each, through the bridge the launch config names. The forms are checked here first
//! (`vesper::control::StageSpec` and friends); the guest checks every argument again and decides what a path names.
//!
//! Exit codes: 0 done; 1 refused or failed in the System VM, or the System VM is unreachable; 2 a bad argument,
//! refused before anything is sent.

use vesper::control::{self, ControlEndpoint, StageDest, StageSource};

use super::{Command, StoreAction};

/// How long a copy may take before the client gives up; the agent's reply comes only once the copy has finished.
const STAGE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(600);

pub fn run(args: &super::Args, command: &Command) -> i32 {
    let name = match command {
        Command::Stage { .. } => "vesperd stage",
        Command::Store { .. } => "vesperd store",
    };
    match run_inner(args, command) {
        Ok(lines) => {
            for line in lines {
                println!("{line}");
            }
            0
        }
        Err((code, err)) => {
            eprintln!("{name}: {err}");
            code
        }
    }
}

fn run_inner(args: &super::Args, command: &Command) -> Result<Vec<String>, (i32, String)> {
    if args.backend != super::Backend::Model || args.socket.is_some() || args.id.is_some() || args.memory_max.is_some() || !args.stage.is_empty() {
        return Err((2, "--backend, --socket, --id, --memory-max and --stage do not apply here; use vesperd stage --id ID".to_string()));
    }
    // Every form is checked before a connection is made.
    let request = match command {
        Command::Stage { id, src, dest } => {
            let src = StageSource::parse(src).map_err(|e| (2, e))?;
            let dest = StageDest::parse(dest).map_err(|e| (2, e))?;
            control::check_pair(&src, &dest).map_err(|e| (2, e))?;
            match id {
                Some(id) if !control::valid_microvm_id(id) => return Err((2, format!("--id {id:?}: use 1 to 64 letters, digits and hyphens"))),
                None if control::needs_id(&src, &dest) => return Err((2, "SRC or DEST is in the jail: pass --id".to_string())),
                _ => {}
            }
            Request::Stage(id.clone(), src, dest)
        }
        Command::Store { action: StoreAction::List } => Request::List,
        Command::Store { action: StoreAction::Rm { name } } => {
            if !control::valid_store_name(name) {
                return Err((2, format!("bad store name {name:?}")));
            }
            Request::Rm(name.clone())
        }
    };
    let (bridge, port) = super::systemvm::bridge_and_port(args).map_err(|e| (1, e))?;
    let mut endpoint = ControlEndpoint::new(bridge, port);
    endpoint.io_timeout = STAGE_TIMEOUT;
    let mut session = control::connect(&endpoint).map_err(|e| (1, e))?;
    let guest = |(rc, reason): (i32, String)| (if rc == 2 { 2 } else { 1 }, reason);
    match request {
        Request::Stage(id, src, dest) => session.stage(id.as_deref(), &src, &dest).map(|l| vec![l]).map_err(guest),
        Request::List => session.store_list().map_err(guest),
        Request::Rm(name) => session.store_rm(&name).map(|l| vec![l]).map_err(guest),
    }
}

enum Request {
    Stage(Option<String>, StageSource, StageDest),
    List,
    Rm(String),
}
