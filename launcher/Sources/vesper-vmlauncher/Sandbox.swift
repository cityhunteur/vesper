// The seatbelt profile the launcher applies to itself (ADR 0012 slice 3, D-082), as `vesperd` and `vesper-proxy` do
// (src/sandbox.rs). It is built from the decoded launch config: the files the VM uses, the console log, the bridge
// socket and the shares. Three rules beyond files and sockets come from the spike (scripts/macos/sandbox-spike/
// launcher.sh), each the cause of a failed boot without it:
//   - generic-issue-extension: Virtualization.framework issues a "Fuse" extension to its VM process for virtio-fs;
//   - file-issue-extension, for the files and shares the profile names: it issues one for each to its VM process;
//   - mach-lookup of com.apple.FSEvents, only when a share is watched (ShareWatcher).
// The VM itself runs in Virtualization.framework's own XPC process, which launchd starts outside this profile.

import Darwin
import Foundation

enum SandboxRule {
    case readFile(String)
    case writeFile(String)
    case readTree(String)
    case writeTree(String)
    case listenUnix(String)
    case issueFileExtension(String, tree: Bool, writable: Bool)
    case issueGenericExtensions
    case lookup(String)
}

/// `path` as the kernel sees it: realpath(3) of the deepest existing ancestor, then the rest unchanged.
func sandboxCanonical(_ path: String) -> String? {
    guard path.hasPrefix("/") else { return nil }
    var base = path
    var rest: [String] = []
    while true {
        if let real = realpath(base, nil) {
            defer { free(real) }
            var out = String(cString: real)
            for part in rest.reversed() { out += (out.hasSuffix("/") ? "" : "/") + part }
            return out
        }
        let url = URL(fileURLWithPath: base)
        let name = url.lastPathComponent
        let parent = url.deletingLastPathComponent().path
        if name.isEmpty || parent == base { return nil }
        rest.append(name)
        base = parent
    }
}

/// A file: its directory resolved, its own name not, so a symlink at the file cannot choose what a rule grants.
func sandboxCanonicalFile(_ path: String) -> String? {
    let url = URL(fileURLWithPath: path)
    guard let dir = sandboxCanonical(url.deletingLastPathComponent().path) else { return nil }
    return (dir.hasSuffix("/") ? dir : dir + "/") + url.lastPathComponent
}

/// `path` as an SBPL string literal; a path with a control character (C0, DEL or C1, as Rust's `char::is_control`)
/// is refused, `\` and `"` are escaped. It walks Unicode scalars, not Swift `Character`s: a quote joined to a
/// combining mark is one `Character`, which would not compare equal to `"` and would end the string (review, D-082).
func sandboxQuote(_ path: String) -> String? {
    if path.unicodeScalars.contains(where: { $0.value < 0x20 || (0x7f...0x9f).contains($0.value) }) { return nil }
    var out = String.UnicodeScalarView()
    out.append("\"")
    for s in path.unicodeScalars {
        if s == "\\" || s == "\"" { out.append("\\") }
        out.append(s)
    }
    out.append("\"")
    return String(out)
}

func sandboxRender(_ rules: [SandboxRule]) -> Result<String, SandboxError> {
    var out = "(version 1)\n(deny default)\n(import \"system.sb\")\n(allow signal (target self))\n"
    func file(_ p: String) throws -> String {
        guard let c = sandboxCanonicalFile(p), let q = sandboxQuote(c) else { throw SandboxError.path(p) }
        return q
    }
    func tree(_ p: String) throws -> String {
        guard let c = sandboxCanonical(p), let q = sandboxQuote(c) else { throw SandboxError.path(p) }
        return q
    }
    do {
        for rule in rules {
            switch rule {
            case .readFile(let p): out += "(allow file-read* (literal \(try file(p))))\n"
            case .writeFile(let p): out += "(allow file-read* file-write* (literal \(try file(p))))\n"
            case .readTree(let p): out += "(allow file-read* (subpath \(try tree(p))))\n"
            case .writeTree(let p): out += "(allow file-read* file-write* (subpath \(try tree(p))))\n"
            case .listenUnix(let p):
                let q = try file(p)
                out += "(allow file-read* file-write* (literal \(q)))\n"
                out += "(allow network-bind network-inbound (local unix-socket (path-literal \(q))))\n"
            case .issueFileExtension(let p, let isTree, let writable):
                // Only the App Sandbox's file classes, and read-write only where the launcher itself may write.
                let classes = writable
                    ? "\"com.apple.app-sandbox.read\" \"com.apple.app-sandbox.read-write\"" : "\"com.apple.app-sandbox.read\""
                let filter = "(\(isTree ? "subpath" : "literal") \(isTree ? try tree(p) : try file(p)))"
                out += "(allow file-issue-extension (require-all (extension-class \(classes)) \(filter)))\n"
            case .issueGenericExtensions: out += "(allow generic-issue-extension)\n"
            case .lookup(let name):
                guard let q = sandboxQuote(name) else { throw SandboxError.path(name) }
                out += "(allow mach-lookup (global-name \(q)))\n"
            }
        }
    } catch let e as SandboxError {
        return .failure(e)
    } catch {
        return .failure(.path("?"))
    }
    return .success(out)
}

enum SandboxError: Error, CustomStringConvertible {
    case path(String)
    case apply(String)
    case check(String)
    var description: String {
        switch self {
        case .path(let p): return "sandbox: \(p) is not an absolute path, or has a control character"
        case .apply(let m): return m
        case .check(let m): return m
        }
    }
}

/// What the launcher may touch for `config`. Directories it creates are made first, by `sandboxPrepare`.
func sandboxRules(_ config: LaunchConfig) -> [SandboxRule] {
    // The config itself is decoded before the profile, so no rule names it.
    var rules: [SandboxRule] = [.issueGenericExtensions]
    for p in [config.kernel_path, config.initrd_path, config.boot_media_path].compactMap({ $0 }) where !p.isEmpty {
        rules += [.readFile(absolutePath(p)), .issueFileExtension(absolutePath(p), tree: false, writable: false)]
    }
    for p in [config.rootfs_path, config.data_disk_path].compactMap({ $0 }) where !p.isEmpty {
        rules += [.writeFile(absolutePath(p)), .issueFileExtension(absolutePath(p), tree: false, writable: true)]
    }
    rules += [.writeFile(absolutePath(config.console_log_path))]
    if let bridge = config.vsock_bridge_path, !bridge.isEmpty {
        rules.append(.listenUnix(absolutePath(bridge)))
    }
    for share in config.shared_directories ?? [] {
        let p = absolutePath(share.host_path)
        let readOnly = share.read_only ?? false
        rules.append(readOnly ? .readTree(p) : .writeTree(p))
        rules.append(.issueFileExtension(p, tree: true, writable: !readOnly))
    }
    if (config.shared_directories ?? []).contains(where: { $0.watch ?? false }) {
        rules.append(.lookup("com.apple.FSEvents"))
    }
    return rules
}

func absolutePath(_ p: String) -> String {
    p.hasPrefix("/") ? p : FileManager.default.currentDirectoryPath + "/" + p
}

/// Makes the directories of the console log and the bridge socket (0700) before the profile, which allows writing
/// in them but not making them; a directory that is a symbolic link is refused.
func sandboxPrepare(_ config: LaunchConfig) -> SandboxError? {
    var dirs = [URL(fileURLWithPath: absolutePath(config.console_log_path)).deletingLastPathComponent().path]
    if let bridge = config.vsock_bridge_path, !bridge.isEmpty {
        dirs.append(URL(fileURLWithPath: absolutePath(bridge)).deletingLastPathComponent().path)
    }
    for dir in dirs {
        if !FileManager.default.fileExists(atPath: dir) {
            do {
                try FileManager.default.createDirectory(
                    atPath: dir, withIntermediateDirectories: true, attributes: [.posixPermissions: 0o700])
            } catch {
                return .apply("create \(dir): \(error.localizedDescription)")
            }
        }
        var st = stat()
        if lstat(dir, &st) == 0, (st.st_mode & S_IFMT) == S_IFLNK {
            return .apply("\(dir) is a symbolic link; name the real directory")
        }
    }
    return nil
}

/// The user's home directory from the user database, which no environment value can change.
func sandboxUserHome() -> String? {
    guard let pw = getpwuid(getuid()), let dir = pw.pointee.pw_dir else { return nil }
    let home = String(cString: dir)
    return home.hasPrefix("/") && home != "/" ? home : nil
}

/// The per-user temporary directory macOS assigns (`confstr`), not `$TMPDIR`.
func sandboxUserTempDir() -> String? {
    var buf = [CChar](repeating: 0, count: 1024)
    let n = confstr(_CS_DARWIN_USER_TEMP_DIR, &buf, buf.count)
    guard n > 0, n <= buf.count else { return nil }
    return String(cString: buf)
}

/// A directory no rule covers, to prove the profile is in force: the home directory, else the user's temporary
/// directory (a share of the home directory is allowed).
func sandboxCanary(_ rules: [SandboxRule]) -> String? {
    let trees: [String] = rules.compactMap {
        switch $0 {
        case .readTree(let p), .writeTree(let p): return sandboxCanonical(p)
        default: return nil
        }
    }
    for candidate in [sandboxUserHome(), sandboxUserTempDir()].compactMap({ $0 }) {
        guard let real = sandboxCanonical(candidate) else { continue }
        let covered = trees.contains { real == $0 || real.hasPrefix($0.hasSuffix("/") ? $0 : $0 + "/") }
        if !covered { return real }
    }
    return nil
}

private typealias SandboxInit = @convention(c) (
    UnsafePointer<CChar>, UInt64, UnsafeMutablePointer<UnsafeMutablePointer<CChar>?>
) -> Int32
private typealias SandboxFreeError = @convention(c) (UnsafeMutablePointer<CChar>?) -> Void

/// Applies `profile` to this process with sandbox_init(3); it cannot be undone or widened.
func sandboxApply(_ profile: String) -> SandboxError? {
    let rtldDefault = UnsafeMutableRawPointer(bitPattern: -2)
    guard let initSym = dlsym(rtldDefault, "sandbox_init"), let freeSym = dlsym(rtldDefault, "sandbox_free_error")
    else {
        return .apply("cannot apply the sandbox profile: sandbox_init is missing (--no-sandbox runs without it, for diagnosis only)")
    }
    let initFn = unsafeBitCast(initSym, to: SandboxInit.self)
    let freeFn = unsafeBitCast(freeSym, to: SandboxFreeError.self)
    var err: UnsafeMutablePointer<CChar>? = nil
    let rc = profile.withCString { initFn($0, 0, &err) }
    if rc == 0 { return nil }
    var reason = String(cString: strerror(errno))
    if let e = err {
        reason = String(cString: e)
        freeFn(e)
    }
    return .apply(
        "cannot apply the sandbox profile: \(reason); is the parent process sandboxed? (--no-sandbox runs without it, for diagnosis only)")
}

/// Listing `canary` must fail with EPERM, the sandbox's refusal, or the profile is not in force.
func sandboxSelfCheck(_ canary: String) -> SandboxError? {
    if let dir = opendir(canary) {
        closedir(dir)
        return .check("sandbox self-check: \(canary) could be read, so the profile is not in force")
    }
    if errno != EPERM {
        return .check("sandbox self-check: \(canary) gave \(String(cString: strerror(errno))), not a refusal")
    }
    return nil
}

/// Builds, applies and proves the launcher's profile, or returns why not.
func sandboxConfine(_ config: LaunchConfig) -> SandboxError? {
    if let e = sandboxPrepare(config) { return e }
    let rules = sandboxRules(config)
    guard let canary = sandboxCanary(rules) else {
        return .check("sandbox: the shares cover both the home directory and the temporary directory, so the self-check has nothing to test; share a narrower directory")
    }
    let profile: String
    switch sandboxRender(rules) {
    case .success(let p): profile = p
    case .failure(let e): return e
    }
    if let e = sandboxApply(profile) { return e }
    return sandboxSelfCheck(canary)
}

/// The --sandbox-probe test hook: tries what the profile must refuse; one `PROBE <name> denied|ALLOWED` line each.
func sandboxProbe(tcpPort: UInt16?, unixPath: String?, file: String?) -> [String] {
    var out: [String] = []
    func check(_ name: String, _ ok: Bool) { out.append("PROBE \(name) \(ok ? "ALLOWED" : "denied")") }
    let home = sandboxUserHome() ?? "/nonexistent"
    if let d = opendir(home + "/.ssh") {
        closedir(d)
        check("read_ssh", true)
    } else {
        check("read_ssh", false)
    }
    let scratch = home + "/.vesper-sandbox-probe-\(getpid())"
    let fd = open(scratch, O_CREAT | O_EXCL | O_WRONLY, 0o600)
    check("write_home", fd >= 0)
    if fd >= 0 {
        close(fd)
        unlink(scratch)
    }
    var pid: pid_t = 0
    let argv: [UnsafeMutablePointer<CChar>?] = [strdup("/bin/sh"), strdup("-c"), strdup("true"), nil]
    let spawned = posix_spawn(&pid, "/bin/sh", nil, nil, argv, environ)
    argv.forEach { free($0) }
    if spawned == 0 {
        var status: Int32 = 0
        waitpid(pid, &status, 0)
    }
    check("exec_sh", spawned == 0)
    let udp = socket(AF_INET, SOCK_DGRAM, 0)
    var any = sockaddr_in()
    any.sin_family = sa_family_t(AF_INET)
    any.sin_len = UInt8(MemoryLayout<sockaddr_in>.size)
    let bound = withUnsafePointer(to: &any) {
        $0.withMemoryRebound(to: sockaddr.self, capacity: 1) { bind(udp, $0, socklen_t(MemoryLayout<sockaddr_in>.size)) }
    }
    check("bind_any", udp >= 0 && bound == 0)
    if udp >= 0 { close(udp) }
    if let port = tcpPort {
        let s = socket(AF_INET, SOCK_STREAM, 0)
        var to = sockaddr_in()
        to.sin_family = sa_family_t(AF_INET)
        to.sin_len = UInt8(MemoryLayout<sockaddr_in>.size)
        to.sin_port = port.bigEndian
        to.sin_addr.s_addr = inet_addr("127.0.0.1")
        let rc = withUnsafePointer(to: &to) {
            $0.withMemoryRebound(to: sockaddr.self, capacity: 1) { connect(s, $0, socklen_t(MemoryLayout<sockaddr_in>.size)) }
        }
        check("connect_loopback_tcp", s >= 0 && rc == 0)
        if s >= 0 { close(s) }
    }
    if let path = unixPath {
        let s = socket(AF_UNIX, SOCK_STREAM, 0)
        var addr = sockaddr_un()
        addr.sun_family = sa_family_t(AF_UNIX)
        let bytes = Array(path.utf8.prefix(MemoryLayout.size(ofValue: addr.sun_path) - 1))
        withUnsafeMutablePointer(to: &addr.sun_path) { ptr in
            ptr.withMemoryRebound(to: UInt8.self, capacity: bytes.count + 1) { dst in
                for (i, b) in bytes.enumerated() { dst[i] = b }
                dst[bytes.count] = 0
            }
        }
        let rc = withUnsafePointer(to: &addr) {
            $0.withMemoryRebound(to: sockaddr.self, capacity: 1) { connect(s, $0, socklen_t(MemoryLayout<sockaddr_un>.size)) }
        }
        check("connect_other_unix", s >= 0 && rc == 0)
        if s >= 0 { close(s) }
    }
    if let path = file {
        let f = open(path, O_RDONLY)
        check("read_outside_rules", f >= 0)
        if f >= 0 { close(f) }
    }
    return out
}
