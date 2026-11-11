import CoreServices
import Foundation
import Virtualization

// Host-edit notifications for virtio-fs shares (GH #150). virtio-fs tells the guest kernel nothing about
// host-side edits, so containers watching a bind mount get no inotify events. The watcher follows the
// watched shares with FSEvents and writes each changed path, one per line, to the guest's notification
// sink (`vesper-share notify` on a guest vsock port), which writes the path's own mtime back; that makes
// the guest kernel emit IN_MODIFY. The watcher never writes to the shares itself.
//
// Nothing here may block: the launcher's shutdown path runs while the watcher is busy, and a guest that
// stops reading must not hold up SIGTERM (council rework of GH #150). Writes to the sink are non-blocking
// and the counters are published under a lock, not read through the watcher's queue.

/// Decides which paths to send. Pure host-side logic: no VM, no sockets.
final class ShareReplayFilter {
    struct Key: Equatable {
        let mtimeNs: Int64
        let size: Int64
    }

    /// `real` is what FSEvents reports (symlinks resolved); `configured` is where the guest mounts the share.
    let shares: [(real: String, configured: String)]
    private var seen: [String: Key] = [:]
    private let seenLimit = 100_000
    private(set) var forwarded = 0
    private(set) var echoes = 0
    private(set) var ignored = 0

    init(paths: [String]) {
        shares = paths.map { path in
            let configured = path.count > 1 && path.hasSuffix("/") ? String(path.dropLast()) : path
            // realpath(3), not URL.resolvingSymlinksInPath: the latter strips "/private", FSEvents does not.
            guard let resolved = realpath(configured, nil) else { return (configured, configured) }
            defer { free(resolved) }
            return (String(cString: resolved), configured)
        }
    }

    private func share(containing path: String) -> (real: String, configured: String)? {
        shares.first { path == $0.real || path.hasPrefix($0.real + "/") }
    }

    private func key(_ path: String) -> Key? {
        var st = stat()
        guard lstat(path, &st) == 0 else { return nil }
        return Key(
            mtimeNs: Int64(st.st_mtimespec.tv_sec) * 1_000_000_000 + Int64(st.st_mtimespec.tv_nsec),
            size: Int64(st.st_size))
    }

    /// The guest path to replay for one FSEvents path, or nil when there is nothing to send.
    ///
    /// - A path that no longer exists (deleted or renamed away) is replayed as its parent directory, so
    ///   watchers that rescan a directory notice.
    /// - A share's root directory is never sent: the guest cannot touch it.
    /// - A path whose (mtime, size) was already sent is dropped. The guest's replay comes back as an FSEvents
    ///   event with the same mtime, and FSEvents flags cannot tell it from an edit; this check ends the loop.
    func line(forEvent path: String) -> String? {
        guard let share = share(containing: path), !path.contains("\n") else {
            ignored += 1
            return nil
        }
        var target = path
        var targetKey = key(target)
        if targetKey == nil {
            seen[target] = nil
            target = (target as NSString).deletingLastPathComponent
            targetKey = key(target)
        }
        guard let found = targetKey, target != share.real, target.hasPrefix(share.real + "/") else {
            ignored += 1
            return nil
        }
        if seen[target] == found {
            echoes += 1
            return nil
        }
        if seen.count >= seenLimit { seen.removeAll(keepingCapacity: true) }
        seen[target] = found
        forwarded += 1
        return share.configured + target.dropFirst(share.real.count)
    }

    /// Forgets a line that was returned but never delivered, so the next event for that path is sent again.
    func forget(line: String) {
        guard let share = shares.first(where: { line.hasPrefix($0.configured + "/") }) else { return }
        seen[share.real + line.dropFirst(share.configured.count)] = nil
    }

    /// FSEvents coalesced or dropped events under `path` (a burst). Replays what changed in the last
    /// `window` seconds below it, visiting at most `limit` entries.
    func lines(forRescanUnder path: String, window: TimeInterval = 60, limit: Int = 50_000) -> [String] {
        guard share(containing: path) != nil,
            let walker = FileManager.default.enumerator(
                at: URL(fileURLWithPath: path), includingPropertiesForKeys: [.contentModificationDateKey])
        else { return [] }
        let cutoff = Date().addingTimeInterval(-window)
        var out: [String] = []
        var visited = 0
        for case let url as URL in walker {
            visited += 1
            if visited > limit { break }
            guard let modified = try? url.resourceValues(forKeys: [.contentModificationDateKey]).contentModificationDate,
                modified >= cutoff
            else { continue }
            if let line = line(forEvent: url.path) { out.append(line) }
        }
        return out
    }
}

/// An open connection to the guest's sink: a descriptor to write to and a way to release it.
struct ShareSinkLink {
    let fd: Int32
    let close: () -> Void
}

/// Opens a link and calls back, on any queue, with it or with nil when the sink is not reachable yet.
typealias ShareSinkConnector = (@escaping (ShareSinkLink?) -> Void) -> Void

enum ShareSink {
    /// The guest's sink on a vsock port. Virtualization.framework objects belong to the VM's queue (main),
    /// so both the connect and the close run there.
    static func vsock(device: VZVirtioSocketDevice, port: UInt32) -> ShareSinkConnector {
        return { done in
            DispatchQueue.main.async {
                device.connect(toPort: port) { result in
                    guard case .success(let connection) = result else {
                        done(nil)
                        return
                    }
                    done(
                        ShareSinkLink(
                            fd: connection.fileDescriptor,
                            close: { DispatchQueue.main.async { connection.close() } }))
                }
            }
        }
    }

    /// A Unix stream socket standing in for the guest (self-test only; no VM).
    static func unixSocket(path: String) -> ShareSinkConnector {
        return { done in
            let fd = socket(AF_UNIX, SOCK_STREAM, 0)
            guard fd >= 0 else {
                done(nil)
                return
            }
            var address = sockaddr_un()
            address.sun_family = sa_family_t(AF_UNIX)
            let capacity = MemoryLayout.size(ofValue: address.sun_path)
            guard path.utf8.count < capacity else {
                close(fd)
                done(nil)
                return
            }
            withUnsafeMutableBytes(of: &address.sun_path) { raw in
                raw.copyBytes(from: path.utf8)
            }
            let ok = withUnsafePointer(to: &address) {
                $0.withMemoryRebound(to: sockaddr.self, capacity: 1) {
                    connect(fd, $0, socklen_t(MemoryLayout<sockaddr_un>.size)) == 0
                }
            }
            guard ok else {
                close(fd)
                done(nil)
                return
            }
            done(ShareSinkLink(fd: fd, close: { close(fd) }))
        }
    }
}

final class ShareWatcher {
    static let defaultPort: UInt32 = 2401

    private let filter: ShareReplayFilter
    private let paths: [String]
    private let connector: ShareSinkConnector?
    private let sinkName: String
    private let announce: Bool
    private let queue = DispatchQueue(label: "vesper.share-watch")
    private var stream: FSEventStreamRef?
    private var link: ShareSinkLink?
    private var connecting = false
    private var connectScheduled = false
    private var flushScheduled = false
    /// Lines waiting for the sink, oldest first. The first `wireLines` of them are in `wire`.
    private var pending: [String] = []
    /// One rescan returns at most 50,000 lines, so a burst that the sink keeps up with is never trimmed.
    /// VESPER_SHARE_WATCH_PENDING_LIMIT lowers it so the self-test can reach the limit.
    private let pendingLimit = max(
        2_000, Int(ProcessInfo.processInfo.environment["VESPER_SHARE_WATCH_PENDING_LIMIT"] ?? "") ?? 100_000)
    private let batchLines = 1_000
    /// The batch being written: its bytes, how many are written, and how many lines it holds.
    private var wire: [UInt8] = []
    private var wireOffset = 0
    private var wireLines = 0
    private var sent = 0
    private var dropped = 0
    private var connects = 0
    private var stalls = 0
    private var rescans = 0
    private let lock = NSLock()
    private var published = "forwarded=0 sent=0 echoes=0 ignored=0 dropped=0 connects=0 stalls=0 rescans=0 queued=0"

    /// With `connector == nil` the watcher is a dry run: it prints the lines instead of sending them.
    /// `announce` prints "WATCHING <n>" once the stream runs (the no-VM modes).
    init(
        paths: [String], connector: ShareSinkConnector?,
        sinkName: String = "guest port \(ShareWatcher.defaultPort)", announce: Bool = false
    ) {
        self.filter = ShareReplayFilter(paths: paths)
        self.paths = paths
        self.connector = connector
        self.sinkName = sinkName
        self.announce = announce
    }

    private func note(_ message: String) {
        FileHandle.standardError.write("vesper-vmlauncher: share-watch: \(message)\n".data(using: .utf8)!)
    }

    /// Starts the FSEvents stream. Returns false when the stream could not be created or started; host
    /// edits then do not reach the guest. The watcher must live as long as the process: the stream holds
    /// an unretained reference to it and is never stopped.
    @discardableResult
    func start() -> Bool {
        var context = FSEventStreamContext(
            version: 0, info: Unmanaged.passUnretained(self).toOpaque(), retain: nil, release: nil,
            copyDescription: nil)
        let callback: FSEventStreamCallback = { _, info, count, eventPaths, eventFlags, _ in
            guard let info = info else { return }
            let watcher = Unmanaged<ShareWatcher>.fromOpaque(info).takeUnretainedValue()
            let paths = unsafeBitCast(eventPaths, to: NSArray.self) as? [String] ?? []
            watcher.handle(paths: paths, flags: Array(UnsafeBufferPointer(start: eventFlags, count: count)))
        }
        // File-level events; no-defer delivers the first event of a quiet period at once, later ones are
        // coalesced for 50 ms.
        let flags = UInt32(
            kFSEventStreamCreateFlagFileEvents | kFSEventStreamCreateFlagUseCFTypes | kFSEventStreamCreateFlagNoDefer)
        guard
            let created = FSEventStreamCreate(
                nil, callback, &context, filter.shares.map { $0.real } as CFArray,
                FSEventStreamEventId(kFSEventStreamEventIdSinceNow), 0.05, flags)
        else {
            note("FSEventStreamCreate failed; host edits will not reach the guest")
            return false
        }
        stream = created
        FSEventStreamSetDispatchQueue(created, queue)
        guard FSEventStreamStart(created) else {
            note("FSEventStreamStart failed; host edits will not reach the guest")
            return false
        }
        if announce {
            print("WATCHING \(paths.count)")
            fflush(stdout)
        }
        if connector != nil { queue.async { self.connect() } }
        return true
    }

    /// A one-line summary of the counters, for the launcher's exit report. Never waits for the watcher's
    /// queue: it returns the last published values.
    func summary() -> String {
        lock.lock()
        defer { lock.unlock() }
        return published
    }

    // Everything below runs on `queue`.

    private func publish() {
        let line =
            "forwarded=\(filter.forwarded) sent=\(sent) echoes=\(filter.echoes) ignored=\(filter.ignored) "
            + "dropped=\(dropped) connects=\(connects) stalls=\(stalls) rescans=\(rescans) queued=\(pending.count)"
        lock.lock()
        published = line
        lock.unlock()
    }

    private func handle(paths: [String], flags: [FSEventStreamEventFlags]) {
        let rescan = UInt32(
            kFSEventStreamEventFlagMustScanSubDirs | kFSEventStreamEventFlagUserDropped
                | kFSEventStreamEventFlagKernelDropped)
        for (path, flag) in zip(paths, flags) {
            if flag & rescan != 0 {
                let lines = filter.lines(forRescanUnder: path)
                rescans += 1
                note("FSEvents dropped events under \(path); rescan found \(lines.count) recently changed path(s)")
                enqueue(lines)
            } else if let line = filter.line(forEvent: path) {
                enqueue([line])
            }
        }
        flush()
    }

    private func enqueue(_ lines: [String]) {
        pending.append(contentsOf: lines)
        if pending.count > pendingLimit {
            // The sink is unreachable or slower than a long burst. Files stay current on access; only the
            // notifications for the oldest paths not yet on the wire are lost. They are forgotten, so a
            // later edit of the same path is sent again.
            let excess = pending.count - pendingLimit
            let range = wireLines..<(wireLines + excess)
            for line in pending[range] { filter.forget(line: line) }
            pending.removeSubrange(range)
            dropped += excess
        }
    }

    private func flush() {
        defer { publish() }
        guard !pending.isEmpty else { return }
        guard connector != nil else {
            print(pending.joined(separator: "\n"))
            fflush(stdout)
            sent += pending.count
            pending.removeAll()
            return
        }
        guard let link = link else {
            // A failed attempt already has a retry scheduled; do not start one per event.
            if !connectScheduled { connect() }
            return
        }
        while true {
            if wire.isEmpty {
                guard !pending.isEmpty else { return }
                wireLines = min(batchLines, pending.count)
                wire = Array((pending[..<wireLines].joined(separator: "\n") + "\n").utf8)
                wireOffset = 0
            }
            let written = wire.withUnsafeBytes { raw in
                write(link.fd, raw.baseAddress! + wireOffset, wire.count - wireOffset)
            }
            if written > 0 {
                wireOffset += written
                if wireOffset == wire.count {
                    pending.removeFirst(wireLines)
                    sent += wireLines
                    wire = []
                    wireOffset = 0
                    wireLines = 0
                }
                continue
            }
            if written < 0 && errno == EINTR { continue }
            if written < 0 && (errno == EAGAIN || errno == EWOULDBLOCK) {
                // The sink is not reading fast enough. Never wait here: try again shortly.
                stalls += 1
                scheduleFlush()
                return
            }
            // The sink went away (service restart, guest shutdown). Keep the batch and reconnect; the sink
            // discards a line cut short by the close, and the whole batch is sent again.
            note("write to \(sinkName) failed (\(String(cString: strerror(errno)))); reconnecting")
            link.close()
            self.link = nil
            wire = []
            wireOffset = 0
            wireLines = 0
            scheduleConnect()
            return
        }
    }

    private func scheduleFlush() {
        guard !flushScheduled else { return }
        flushScheduled = true
        queue.asyncAfter(deadline: .now() + 0.05) {
            self.flushScheduled = false
            self.flush()
        }
    }

    private func scheduleConnect() {
        guard !connectScheduled else { return }
        connectScheduled = true
        queue.asyncAfter(deadline: .now() + 2) {
            self.connectScheduled = false
            self.connect()
        }
    }

    private func connect() {
        guard let connector = connector, link == nil, !connecting else { return }
        connecting = true
        connector { opened in
            self.queue.async {
                self.connecting = false
                guard let opened = opened else {
                    // Normal while the guest is still booting; the sink starts with the guest's services.
                    self.scheduleConnect()
                    return
                }
                let flags = fcntl(opened.fd, F_GETFL)
                guard flags >= 0, fcntl(opened.fd, F_SETFL, flags | O_NONBLOCK) == 0 else {
                    // A blocking descriptor could hold up shutdown; do not use it.
                    self.note("cannot make the connection to \(self.sinkName) non-blocking; retrying")
                    opened.close()
                    self.scheduleConnect()
                    return
                }
                self.link = opened
                self.connects += 1
                self.note("connected to \(self.sinkName) (connection \(self.connects))")
                self.flush()
            }
        }
    }
}
