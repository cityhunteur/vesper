//! `vesperd --backend systemvm` against a fake bridge and agent (ADR 0007 slice 4). These tests start the real binary:
//! the `LAUNCHAPI` lease, the version check against the pinned swagger, the `FCAPI` relay, the lease closing from
//! the System VM side, and SIGTERM closing the lease. Slice 5 adds the `DATA status` check before the launch,
//! `--stage` tokens on `LAUNCHAPI`, and `vesperd stage` and `vesperd store`, each refused locally for a bad form.

use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

/// A Firecracker response with the shape and bytes of the real one.
const FC_REPLY: &[u8] = b"HTTP/1.1 400 \r\nServer: Firecracker API\r\nConnection: keep-alive\r\nContent-Type: application/json\r\nContent-Length: 30\r\n\r\n{\"fault_message\":\"fake error\"}";

#[derive(Default)]
struct Agent {
    /// `firecracker_version` in `READY`.
    version: String,
    /// `ERR busy` replies to send before `READY`.
    busy: u32,
    /// `FCAPI` replies `ERR` instead of `OK`.
    refuse_fcapi: bool,
    /// Set by the test: the agent closes the lease, as when Firecracker exits.
    end_lease: bool,
    /// Set by the agent when the client (vesperd) closed the lease.
    client_closed_lease: bool,
    launches: u32,
    /// The state in `DATA status`; empty means `mounted`.
    data: String,
    /// Every line after the greeting, in order.
    requests: Vec<String>,
}

fn fake_agent(dir: &Path, version: &str) -> (PathBuf, Arc<Mutex<Agent>>) {
    let vsock = dir.join("vsock");
    std::fs::create_dir_all(&vsock).unwrap();
    let bridge = vsock.join("3.sock");
    let listener = UnixListener::bind(&bridge).unwrap();
    let agent = Arc::new(Mutex::new(Agent { version: version.to_string(), ..Default::default() }));
    let shared = agent.clone();
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { break };
            let agent = shared.clone();
            thread::spawn(move || serve_one(stream, agent));
        }
    });
    (bridge, agent)
}

fn serve_one(stream: UnixStream, agent: Arc<Mutex<Agent>>) {
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut writer = stream;
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    assert_eq!(line, "CONNECT 1024\n");
    writer.write_all(b"VESPER-AGENT 1 vesper\n").unwrap();
    line.clear();
    if reader.read_line(&mut line).unwrap_or(0) == 0 {
        return;
    }
    agent.lock().unwrap().requests.push(line.trim_end().to_string());
    if line == "DATA status\n" {
        let state = agent.lock().unwrap().data.clone();
        let state = if state.is_empty() { "mounted /dev/vdb used=1 avail=2".to_string() } else { state };
        let _ = writer.write_all(format!("DATA 0 1\nDATA {state}\n").as_bytes());
        return;
    }
    if let Some(args) = line.trim_end().strip_prefix("VESPERD ") {
        let reply = match args.split(' ').collect::<Vec<_>>().as_slice() {
            ["stage", "vm-1", src, dest] => format!("VESPERD 0 1\nSTAGED {src} {dest} copy bytes=7\n"),
            ["stage", _, _, _] => "VESPERD 1 1\nERR no vesperd microVM\n".to_string(),
            ["store-list"] => "VESPERD 0 2\nSTORE a.mem bytes=7 links=1\nFREE avail=100 reserve=50\n".to_string(),
            ["store-rm", "a.mem"] => "VESPERD 0 1\nREMOVED a.mem\n".to_string(),
            _ => "VESPERD 1 1\nERR no store file\n".to_string(),
        };
        let _ = writer.write_all(reply.as_bytes());
        return;
    }
    if line.starts_with("LAUNCHAPI vm-1") {
        let version = {
            let mut a = agent.lock().unwrap();
            a.launches += 1;
            if a.busy > 0 {
                a.busy -= 1;
                drop(a);
                let _ = writer.write_all(b"ERR busy vm-1\n");
                return;
            }
            a.version.clone()
        };
        writer.write_all(format!("READY vm-1 pid=4242 firecracker_version={version}\n").as_bytes()).unwrap();
        writer.set_read_timeout(Some(Duration::from_millis(50))).unwrap();
        let mut buf = [0u8; 64];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => {
                    agent.lock().unwrap().client_closed_lease = true;
                    return;
                }
                Ok(_) => {}
                Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {}
                Err(_) => return,
            }
            if agent.lock().unwrap().end_lease {
                let _ = writer.shutdown(std::net::Shutdown::Both);
                return;
            }
        }
    } else if line == "FCAPI vm-1\n" {
        if agent.lock().unwrap().refuse_fcapi {
            let _ = writer.write_all(b"ERR vm-1 not ready\n");
            return;
        }
        writer.write_all(b"OK\n").unwrap();
        loop {
            let mut head = String::new();
            if reader.read_line(&mut head).unwrap_or(0) == 0 {
                return;
            }
            loop {
                let mut h = String::new();
                if reader.read_line(&mut h).unwrap_or(0) == 0 || h == "\r\n" {
                    break;
                }
            }
            if writer.write_all(FC_REPLY).is_err() {
                return;
            }
        }
    } else {
        let _ = writer.write_all(b"ERR unknown command\n");
    }
}

struct Vesperd {
    child: Child,
    socket: PathBuf,
}

impl Drop for Vesperd {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn spawn(dir: &Path, bridge: &Path, extra: &[&str]) -> Vesperd {
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700)).unwrap();
    let socket = dir.join("api.sock");
    let child = Command::new(env!("CARGO_BIN_EXE_vesperd"))
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .args(["--backend", "systemvm", "--id", "vm-1", "--bridge"])
        .arg(bridge)
        .arg("--socket")
        .arg(&socket)
        .args(extra)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn vesperd");
    Vesperd { child, socket }
}

fn wait_listening(v: &Vesperd) {
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(20) {
        if UnixStream::connect(&v.socket).is_ok() {
            return;
        }
        thread::sleep(Duration::from_millis(50));
    }
    panic!("vesperd did not listen on {}", v.socket.display());
}

/// Waits for the process to exit; returns its status and stderr.
fn wait_exit(v: &mut Vesperd, within: Duration) -> (ExitStatus, String) {
    let started = Instant::now();
    loop {
        if let Some(status) = v.child.try_wait().unwrap() {
            let mut err = String::new();
            v.child.stderr.take().unwrap().read_to_string(&mut err).unwrap();
            return (status, err);
        }
        assert!(started.elapsed() < within, "vesperd still running after {within:?}");
        thread::sleep(Duration::from_millis(50));
    }
}

fn wait_for(what: &str, within: Duration, check: impl Fn() -> bool) {
    let started = Instant::now();
    while !check() {
        assert!(started.elapsed() < within, "{what} not seen within {within:?}");
        thread::sleep(Duration::from_millis(20));
    }
}

fn request(socket: &Path) -> Vec<u8> {
    let mut client = UnixStream::connect(socket).unwrap();
    client.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    client.write_all(b"PUT /machine-config HTTP/1.1\r\nHost: localhost\r\n\r\n").unwrap();
    let mut reply = vec![0u8; FC_REPLY.len()];
    client.read_exact(&mut reply).unwrap();
    reply
}

#[test]
fn relays_firecracker_bytes_and_sigterm_closes_the_lease() {
    let tmp = tempfile::tempdir().unwrap();
    let (bridge, agent) = fake_agent(tmp.path(), "1.16.0");
    let mut v = spawn(tmp.path(), &bridge, &[]);
    wait_listening(&v);
    assert_eq!(std::fs::metadata(&v.socket).unwrap().permissions().mode() & 0o777, 0o600);
    assert_eq!(request(&v.socket), FC_REPLY, "the upstream's bytes must pass through unchanged");
    assert!(!agent.lock().unwrap().client_closed_lease);

    // A closed stdout must not turn the stop into a panic.
    drop(v.child.stdout.take());
    let pid = v.child.id().to_string();
    assert!(Command::new("kill").args(["-TERM", &pid]).status().unwrap().success());
    let (status, err) = wait_exit(&mut v, Duration::from_secs(10));
    assert!(status.success(), "SIGTERM should exit 0: {status:?} {err}");
    assert!(agent.lock().unwrap().client_closed_lease, "SIGTERM must close the lease");
    assert!(!v.socket.exists(), "the socket is removed on exit");
}

#[test]
fn a_version_other_than_the_pinned_swagger_is_refused() {
    let tmp = tempfile::tempdir().unwrap();
    let (bridge, agent) = fake_agent(tmp.path(), "9.9.9");
    let mut v = spawn(tmp.path(), &bridge, &[]);
    let (status, err) = wait_exit(&mut v, Duration::from_secs(10));
    assert!(!status.success());
    assert!(err.contains("Firecracker 9.9.9") && err.contains("pinned swagger is 1.16.0"), "stderr: {err}");
    assert!(!v.socket.exists(), "no socket for a refused microVM");
    wait_for("the lease closed by vesperd", Duration::from_secs(5), || agent.lock().unwrap().client_closed_lease);
}

#[test]
fn the_lease_closing_makes_vesperd_exit_non_zero() {
    let tmp = tempfile::tempdir().unwrap();
    let (bridge, agent) = fake_agent(tmp.path(), "1.16.0");
    let mut v = spawn(tmp.path(), &bridge, &[]);
    wait_listening(&v);
    agent.lock().unwrap().end_lease = true;
    let (status, err) = wait_exit(&mut v, Duration::from_secs(10));
    assert!(!status.success());
    assert!(err.contains("the lease of microVM vm-1 closed"), "stderr: {err}");
    assert!(!v.socket.exists(), "the socket is removed when the lease closes");
}

#[test]
fn busy_is_retried_and_a_refused_fcapi_closes_the_client_with_no_body() {
    let tmp = tempfile::tempdir().unwrap();
    let (bridge, agent) = fake_agent(tmp.path(), "1.16.0");
    {
        let mut a = agent.lock().unwrap();
        a.busy = 2;
        a.refuse_fcapi = true;
    }
    let v = spawn(tmp.path(), &bridge, &[]);
    wait_listening(&v);
    assert_eq!(agent.lock().unwrap().launches, 3, "two ERR busy, then READY");
    let mut client = UnixStream::connect(&v.socket).unwrap();
    client.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    client.write_all(b"GET / HTTP/1.1\r\n\r\n").unwrap();
    // "Closed with no bytes": macOS reports end of file; Linux resets a socket closed with the request unread.
    let mut rest = Vec::new();
    match client.read_to_end(&mut rest) {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::ConnectionReset => {}
        Err(e) => panic!("read: {e}"),
    }
    assert!(rest.is_empty(), "vesperd sent {:?}", String::from_utf8_lossy(&rest));
}

#[test]
fn a_second_vesperd_on_the_socket_launches_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let (bridge, agent) = fake_agent(tmp.path(), "1.16.0");
    let first = spawn(tmp.path(), &bridge, &[]);
    wait_listening(&first);
    let mut second = spawn(tmp.path(), &bridge, &[]);
    let (status, err) = wait_exit(&mut second, Duration::from_secs(10));
    assert!(!status.success());
    assert!(err.contains("another vesperd holds"), "stderr: {err}");
    assert_eq!(agent.lock().unwrap().launches, 1, "the refused vesperd must not launch a microVM");
    assert_eq!(request(&first.socket), FC_REPLY);
}

#[test]
fn bad_arguments_are_refused() {
    let tmp = tempfile::tempdir().unwrap();
    let run = |args: &[&str]| {
        let out = Command::new(env!("CARGO_BIN_EXE_vesperd"))
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .env("XDG_RUNTIME_DIR", tmp.path())
            .args(args)
            .output()
            .unwrap();
        assert!(!out.status.success(), "{args:?} accepted");
        String::from_utf8_lossy(&out.stderr).into_owned()
    };
    assert!(run(&["--backend", "systemvm", "--bridge", "x/3.sock"]).contains("needs --id"));
    assert!(run(&["--backend", "systemvm", "--id", "../x", "--bridge", "x/3.sock"]).contains("letters, digits and hyphens"));
    assert!(run(&["--id", "vm-1"]).contains("apply to --backend systemvm"));
    assert!(run(&["--tap", "tap0"]).contains("apply to --backend systemvm"));
    assert!(run(&["--network", "nat"]).contains("apply to --backend systemvm"));
}

fn cli(dir: &Path, bridge: &Path, args: &[&str]) -> (i32, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_vesperd"))
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .args(args)
        .arg("--bridge")
        .arg(bridge)
        .env("XDG_RUNTIME_DIR", dir)
        .output()
        .unwrap();
    (out.status.code().unwrap_or(-1), String::from_utf8_lossy(&out.stdout).into_owned(), String::from_utf8_lossy(&out.stderr).into_owned())
}

#[test]
fn a_data_disk_not_in_use_is_refused_before_any_launch() {
    let tmp = tempfile::tempdir().unwrap();
    let (bridge, agent) = fake_agent(tmp.path(), "1.16.0");
    agent.lock().unwrap().data = "absent".to_string();
    let mut v = spawn(tmp.path(), &bridge, &[]);
    let (status, err) = wait_exit(&mut v, Duration::from_secs(10));
    assert!(!status.success());
    assert!(err.contains("data disk is not in use (DATA absent)"), "stderr: {err}");
    let a = agent.lock().unwrap();
    assert_eq!((a.launches, a.requests.as_slice()), (0, &["DATA status".to_string()][..]), "no LAUNCHAPI without the data disk");
    assert!(!v.socket.exists());
}

#[test]
fn stage_flags_ride_on_launchapi_in_order() {
    let tmp = tempfile::tempdir().unwrap();
    let (bridge, agent) = fake_agent(tmp.path(), "1.16.0");
    let v = spawn(
        tmp.path(),
        &bridge,
        &["--memory-max", "268435456", "--stage", "store:a.mem:/snap.mem", "--stage", "share:/Users/me/vm/d.img:/d.img"],
    );
    wait_listening(&v);
    let a = agent.lock().unwrap();
    assert_eq!(a.requests[0], "DATA status");
    assert_eq!(a.requests[1], "LAUNCHAPI vm-1 268435456 stage=store:a.mem:/snap.mem stage=share:/Users/me/vm/d.img:/d.img");
}

#[test]
fn tap_flags_ride_on_launchapi_after_the_stages() {
    let tmp = tempfile::tempdir().unwrap();
    let (bridge, agent) = fake_agent(tmp.path(), "1.16.0");
    let v = spawn(tmp.path(), &bridge, &["--tap", "tap0", "--stage", "store:a.mem:/snap.mem", "--tap", "tap1"]);
    wait_listening(&v);
    let a = agent.lock().unwrap();
    assert_eq!(a.requests[1], "LAUNCHAPI vm-1 stage=store:a.mem:/snap.mem tap=tap0 tap=tap1");
}

#[test]
fn network_nat_rides_on_launchapi_with_tap_gateways() {
    let tmp = tempfile::tempdir().unwrap();
    let (bridge, agent) = fake_agent(tmp.path(), "1.16.0");
    let v = spawn(tmp.path(), &bridge, &["--network", "nat", "--tap", "tap0:172.16.0.1/30", "--tap", "tap1"]);
    wait_listening(&v);
    let a = agent.lock().unwrap();
    assert_eq!(a.requests[1], "LAUNCHAPI vm-1 tap=tap0 tapaddr=tap0:172.16.0.1/30 tap=tap1 net=nat");
}

#[test]
fn a_bad_network_flag_launches_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let (bridge, agent) = fake_agent(tmp.path(), "1.16.0");
    for (flags, why) in [
        (vec!["--tap", "tap0:172.16.0.1/30"], "needs --network nat"),
        (vec!["--network", "nat", "--tap", "tap0"], "needs a --tap NAME:ADDR/PREFIX"),
        (vec!["--network", "nat", "--tap", "tap0:8.8.8.1/24"], "RFC 1918"),
        (vec!["--network", "nat", "--tap", "tap0:198.18.0.1/24"], "RFC 1918"),
        (vec!["--network", "nat", "--tap", "tap0:172.17.5.1/24"], "overlaps 172.17.0.0/16"),
        (vec!["--network", "nat", "--tap", "a:10.1.0.1/16", "--tap", "b:10.1.9.1/24"], "overlaps"),
        (vec!["--network", "open"], "invalid value"),
    ] {
        let mut v = spawn(tmp.path(), &bridge, &flags);
        let (status, err) = wait_exit(&mut v, Duration::from_secs(10));
        assert!(!status.success() && err.contains(why), "{flags:?}: {err}");
    }
    assert!(agent.lock().unwrap().requests.is_empty(), "nothing reaches the agent");
}

#[test]
fn a_bad_tap_flag_launches_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let (bridge, agent) = fake_agent(tmp.path(), "1.16.0");
    for (flags, why) in [
        (vec!["--tap", "lo"], "not lo"),
        (vec!["--tap=-tap0"], "not starting with -"),
        (vec!["--tap", "tap 0"], "letters, digits"),
        (vec!["--tap", "a-name-of-16-chr"], "1 to 15"),
        (vec!["--tap", "tap0", "--tap", "tap0"], "given twice"),
    ] {
        let mut v = spawn(tmp.path(), &bridge, &flags);
        let (status, err) = wait_exit(&mut v, Duration::from_secs(10));
        assert!(!status.success() && err.contains(why), "{flags:?}: {err}");
    }
    let names: Vec<String> = (0..9).map(|i| format!("tap{i}")).collect();
    let many: Vec<&str> = names.iter().flat_map(|n| ["--tap", n.as_str()]).collect();
    let mut v = spawn(tmp.path(), &bridge, &many);
    let (status, err) = wait_exit(&mut v, Duration::from_secs(10));
    assert!(!status.success() && err.contains("at most 8"), "{err}");
    assert!(agent.lock().unwrap().requests.is_empty(), "nothing reaches the agent");
}

#[test]
fn a_bad_stage_flag_launches_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let (bridge, agent) = fake_agent(tmp.path(), "1.16.0");
    for (flag, why) in [
        ("share:/Users/me/../etc/shadow:/x", "no . or .. component"),
        ("jail:/a:/b", "need a running microVM"),
        ("store:a.mem:store:b", "--stage writes into the jail"),
        ("store:a.mem", "use SRC:DEST"),
    ] {
        let mut v = spawn(tmp.path(), &bridge, &["--stage", flag]);
        let (status, err) = wait_exit(&mut v, Duration::from_secs(10));
        assert!(!status.success() && err.contains(why), "{flag}: {err}");
    }
    let many: Vec<&str> = std::iter::repeat(["--stage", "store:a.mem:/a.mem"]).take(17).flatten().collect();
    let mut v = spawn(tmp.path(), &bridge, &many);
    let (status, err) = wait_exit(&mut v, Duration::from_secs(10));
    assert!(!status.success() && err.contains("at most 16"), "{err}");
    assert!(agent.lock().unwrap().requests.is_empty(), "nothing reaches the agent");
}

#[test]
fn stage_and_store_subcommands_send_one_vesperd_line() {
    let tmp = tempfile::tempdir().unwrap();
    let (bridge, agent) = fake_agent(tmp.path(), "1.16.0");
    let (code, out, err) = cli(tmp.path(), &bridge, &["stage", "--id", "vm-1", "jail:/snap.mem", "store:a.mem"]);
    assert_eq!((code, out.as_str()), (0, "STAGED jail:/snap.mem store:a.mem copy bytes=7\n"), "{err}");
    let (code, _, err) = cli(tmp.path(), &bridge, &["stage", "--id", "vm-2", "store:a.mem", "/a.mem"]);
    assert_eq!(code, 1, "a refusal in the System VM exits 1");
    assert!(err.contains("vesperd stage: no vesperd microVM"), "{err}");
    let (code, out, _) = cli(tmp.path(), &bridge, &["store", "list"]);
    assert_eq!((code, out.as_str()), (0, "STORE a.mem bytes=7 links=1\nFREE avail=100 reserve=50\n"));
    let (code, out, _) = cli(tmp.path(), &bridge, &["store", "rm", "a.mem"]);
    assert_eq!((code, out.as_str()), (0, "REMOVED a.mem\n"));
    let (code, _, err) = cli(tmp.path(), &bridge, &["store", "rm", "b"]);
    assert!(code == 1 && err.contains("no store file"), "{err}");
    assert_eq!(
        agent.lock().unwrap().requests,
        ["VESPERD stage vm-1 jail:/snap.mem store:a.mem", "VESPERD stage vm-2 store:a.mem /a.mem", "VESPERD store-list", "VESPERD store-rm a.mem", "VESPERD store-rm b"]
    );
}

#[test]
fn bad_stage_and_store_arguments_never_reach_the_agent() {
    let tmp = tempfile::tempdir().unwrap();
    let (bridge, agent) = fake_agent(tmp.path(), "1.16.0");
    for (args, why) in [
        (&["stage", "store:a.mem", "/a.mem"][..], "pass --id"),
        (&["stage", "--id", "../x", "store:a", "/a"][..], "letters, digits and hyphens"),
        (&["stage", "--id", "vm-1", "store:a", "store:b"][..], "store to store"),
        (&["stage", "--id", "vm-1", "jail:/a", "/b"][..], "jail to jail"),
        (&["stage", "--id", "vm-1", "share:/a b", "/b"][..], "bad SRC"),
        (&["stage", "--id", "vm-1", "share:/a", "relative"][..], "bad DEST"),
        (&["store", "rm", ".partial.1"][..], "bad store name"),
        (&["--backend", "systemvm", "store", "list"][..], "do not apply here"),
        (&["--tap", "tap0", "store", "list"][..], "do not apply here"),
    ] {
        let (code, _, err) = cli(tmp.path(), &bridge, args);
        assert!(code == 2 && err.contains(why), "{args:?}: exit {code}: {err}");
    }
    assert!(agent.lock().unwrap().requests.is_empty());
}

/// The guest helper's own tap check (`vesper-vesperd launch`, `valid_tap`), run hermetically: it refuses a bad
/// `tap=` token before it touches anything, and agrees with `vesper::control::valid_tap_name`.
#[test]
fn the_guest_helper_refuses_the_tap_names_vesperd_refuses() {
    let script = concat!(env!("CARGO_MANIFEST_DIR"), "/systemvm/overlay/usr/local/bin/vesper-vesperd");
    let launch = |tokens: &[&str]| {
        let out = Command::new("sh").arg(script).args(["launch", "vm-1"]).args(tokens).output().unwrap();
        String::from_utf8_lossy(&out.stdout).into_owned()
    };
    for name in ["", "lo", "-", "-tap0", "tap 0", "tap.0", "tap/0", "a-name-of-16-chr"] {
        let out = launch(&[&format!("tap={name}")]);
        assert!(out.starts_with("ERR bad tap name"), "{name:?}: {out}");
        assert!(!vesper::control::valid_tap_name(name), "{name:?} accepted by vesperd");
    }
    for name in ["tap0", "up", "dev", "t-", &"t".repeat(15)] {
        let out = launch(&[&format!("tap={name}")]);
        // Past the token checks the helper stops at the first System VM requirement (memory, Firecracker).
        assert!(out.starts_with("ERR") && !out.contains("tap"), "{name:?}: {out}");
        assert!(vesper::control::valid_tap_name(name), "{name:?} refused by vesperd");
    }
    assert!(launch(&["tap=tap0", "tap=tap0"]).starts_with("ERR tap tap0 given twice"));
    let nine: Vec<String> = (0..9).map(|i| format!("tap=tap{i}")).collect();
    let nine: Vec<&str> = nine.iter().map(String::as_str).collect();
    assert!(launch(&nine).starts_with("ERR at most 8 tap= tokens"), "{}", launch(&nine));
    // ADR 0008: the guest checks every network token again, before it touches anything.
    for (tokens, why) in [
        (&["tap=tap0", "tapaddr=tap0:172.16.0.1/30"][..], "ERR tapaddr= needs net=nat"),
        (&["tap=tap0", "net=nat"][..], "ERR net=nat needs a tapaddr="),
        (&["tap=tap0", "tapaddr=tap1:172.16.0.1/30", "net=nat"][..], "ERR tapaddr= names tap1, which is not a tap= token"),
        (&["tap=tap0", "tapaddr=tap0:8.8.8.1/24", "net=nat"][..], "not inside RFC 1918"),
        (&["tap=tap0", "tapaddr=tap0:172.16.0.0/30", "net=nat"][..], "host address"),
        (&["tap=tap0", "tapaddr=tap0:172.16.0.1/31", "net=nat"][..], "prefix length of 16 to 30"),
        (&["tap=tap0", "tapaddr=tap0:172.17.0.9/24", "net=nat"][..], "overlaps 172.17.0.0/16"),
        (&["tap=a", "tap=b", "tapaddr=a:10.1.0.1/16", "tapaddr=b:10.1.7.1/24", "net=nat"][..], "overlaps 10.1.0.1/16"),
        (&["tap=tap0", "tapaddr=tap0:172.16.0.1/30", "tapaddr=tap0:172.16.1.1/30", "net=nat"][..], "gives tap0 twice"),
        (&["net=open"][..], "ERR bad network policy open"),
        (&["net=nat", "net=nat"][..], "ERR net= given twice"),
    ] {
        let out = launch(tokens);
        assert!(out.contains(why), "{tokens:?}: {out}");
    }
    let ok = launch(&["tap=tap0", "tapaddr=tap0:172.16.0.1/30", "net=nat"]);
    assert!(ok.starts_with("ERR") && !ok.contains("tap") && !ok.contains("net"), "{ok}");
}
