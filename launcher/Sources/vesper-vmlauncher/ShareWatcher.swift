import CoreServices
import Foundation
import Virtualization

// Host-edit notifications for virtio-fs shares (GH #150). virtio-fs tells the guest kernel nothing about
// host-side edits, so containers watching a bind mount get no inotify events. The watcher follows the
// watched shares with FSEvents and writes each changed path, one per line, to the guest's notification
// sink (`vesper-share notify` on a guest vsock port), which sets the path's mtime to its own value; that
// makes the guest kernel emit IN_MODIFY. The watcher never writes to the shares itself.

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

final class ShareWatcher {
    static let defaultPort: UInt32 = 2401

    private let filter: ShareReplayFilter
    private let paths: [String]
    private let device: VZVirtioSocketDevice?
    private let port: UInt32
    private let queue = DispatchQueue(label: "vesper.share-watch")
    private var stream: FSEventStreamRef?
    private var connection: VZVirtioSocketConnection?
    private var connecting = false
    private var pending: [String] = []
    private let pendingLimit = 20_000
    private var sent = 0
    private var dropped = 0
    private var connects = 0

    /// With `device == nil` the watcher is a dry run: it prints the lines instead of sending them.
    init(paths: [String], device: VZVirtioSocketDevice?, port: UInt32 = ShareWatcher.defaultPort) {
        self.filter = ShareReplayFilter(paths: paths)
        self.paths = paths
        self.device = device
        self.port = port
    }

    private func note(_ message: String) {
        FileHandle.standardError.write("vesper-vmlauncher: share-watch: \(message)\n".data(using: .utf8)!)
    }

    func start() {
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
            return
        }
        stream = created
        FSEventStreamSetDispatchQueue(created, queue)
        guard FSEventStreamStart(created) else {
            note("FSEventStreamStart failed; host edits will not reach the guest")
            return
        }
        if device == nil {
            print("WATCHING \(paths.count)")
            fflush(stdout)
        } else {
            queue.async { self.connect() }
        }
    }

    /// A one-line summary of the counters, for the launcher's exit report.
    func summary() -> String {
        queue.sync {
            "forwarded=\(filter.forwarded) sent=\(sent) echoes=\(filter.echoes) ignored=\(filter.ignored) "
                + "dropped=\(dropped) connects=\(connects)"
        }
    }

    // Everything below runs on `queue`.

    private func handle(paths: [String], flags: [FSEventStreamEventFlags]) {
        let rescan = UInt32(
            kFSEventStreamEventFlagMustScanSubDirs | kFSEventStreamEventFlagUserDropped
                | kFSEventStreamEventFlagKernelDropped)
        for (path, flag) in zip(paths, flags) {
            if flag & rescan != 0 {
                enqueue(filter.lines(forRescanUnder: path))
            } else if let line = filter.line(forEvent: path) {
                enqueue([line])
            }
        }
        flush()
    }

    private func enqueue(_ lines: [String]) {
        pending.append(contentsOf: lines)
        if pending.count > pendingLimit {
            // The guest has been unreachable for a long burst. Files stay current on access; only the
            // notifications for the oldest paths are lost.
            let excess = pending.count - pendingLimit
            pending.removeFirst(excess)
            dropped += excess
        }
    }

    private func flush() {
        guard !pending.isEmpty else { return }
        if device == nil {
            print(pending.joined(separator: "\n"))
            fflush(stdout)
            sent += pending.count
            pending.removeAll()
            return
        }
        guard let connection = connection else {
            connect()
            return
        }
        let batch = pending
        let data = Array((batch.joined(separator: "\n") + "\n").utf8)
        var offset = 0
        while offset < data.count {
            let written = data.withUnsafeBytes { raw in
                write(connection.fileDescriptor, raw.baseAddress! + offset, data.count - offset)
            }
            if written <= 0 {
                // The sink went away (service restart, guest shutdown). Keep the batch and reconnect.
                note("write to guest port \(port) failed (\(String(cString: strerror(errno)))); reconnecting")
                connection.close()
                self.connection = nil
                scheduleConnect()
                return
            }
            offset += written
        }
        sent += batch.count
        pending.removeAll()
    }

    private func scheduleConnect() {
        queue.asyncAfter(deadline: .now() + 2) { self.connect() }
    }

    private func connect() {
        guard let device = device, connection == nil, !connecting else { return }
        connecting = true
        // Virtualization.framework objects belong to the VM's queue (main).
        DispatchQueue.main.async {
            device.connect(toPort: self.port) { result in
                self.queue.async {
                    self.connecting = false
                    switch result {
                    case .success(let connection):
                        self.connection = connection
                        self.connects += 1
                        self.note("connected to guest port \(self.port) (connection \(self.connects))")
                        self.flush()
                    case .failure:
                        // Normal while the guest is still booting; the sink starts with the guest's services.
                        self.scheduleConnect()
                    }
                }
            }
        }
    }
}
