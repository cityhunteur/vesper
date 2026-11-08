// vesper-vmlauncher: boot one Vesper System VM launch config on Virtualization.framework.
//
// Reads build/systemvm/launch-config.json (written by `systemvmctl launch-config`), boots the
// kernel with the initramfs and/or block rootfs it names, attaches a NAT network device and a
// virtio console, streams the console to `console_log_path`, and exits as soon as the console
// shows `--ready-pattern` (default "login:").
//
// When the config carries a `probe`, the launcher then logs in on the console, types the probe
// commands, waits for the success pattern and types the shutdown command, so the guest stops itself.
// With --keep-running the shutdown command is skipped and the VM stays up until the guest powers
// off (for example through the agent's SHUTDOWN) or the launcher receives SIGTERM.
// When the config carries `vsock_bridge_path`, the launcher serves that Unix socket as the vsock
// bridge: a client writes "CONNECT <port>\n" and is then relayed to that guest vsock port.
//
// Exit codes: 0 ready pattern seen and VM stopped (probe, if any, succeeded); 1 VM failed to start,
// stopped before ready, or stopped before the probe succeeded; 2 timeout waiting for the ready
// pattern or for a probe step; 3 bad arguments or configuration; 4 probe step produced no match
// before the guest stopped.

import Foundation
import Virtualization

struct ProbeSettings: Decodable {
    let login_user: String
    let shell_prompt: String
    let commands: [String]
    let success_pattern: String
    let shutdown_command: String
}

/// A host directory shared into the guest over virtio-fs (GH #29); the guest mounts it by `tag`.
struct SharedDirectory: Decodable {
    let tag: String
    let host_path: String
    let read_only: Bool?
}

struct LaunchConfig: Decodable {
    let kernel_path: String
    let rootfs_path: String?
    let initrd_path: String?
    let boot_media_path: String?
    let kernel_cmdline: String
    let ready_pattern: String?
    let max_boot_ms: UInt64?
    let probe: ProbeSettings?
    let vsock_bridge_path: String?
    let agent_port: UInt32?
    /// "auto" (enable when the host supports it), "require" (fail otherwise) or "off".
    let nested_virtualization: String?
    let network_mode: String
    let cpu_count: Int
    let memory_mib: UInt64
    let console_log_path: String
    let shared_directories: [SharedDirectory]?
}

struct Options {
    var configPath = "build/systemvm/launch-config.json"
    var readyPattern: String? = nil
    var timeoutSeconds: Double = 60
    var keepRunning = false
    var skipProbe = false
    var hostCapabilities = false
}

/// Host facts for backend selection (GH #17): nested virtualisation needs macOS 15 and an M3 or later.
func hostCapabilitiesJSON() -> String {
    func sysctl(_ name: String) -> String {
        var size = 0
        sysctlbyname(name, nil, &size, nil, 0)
        var buffer = [CChar](repeating: 0, count: max(size, 1))
        sysctlbyname(name, &buffer, &size, nil, 0)
        return String(cString: buffer)
    }
    let version = ProcessInfo.processInfo.operatingSystemVersion
    let macos = "\(version.majorVersion).\(version.minorVersion).\(version.patchVersion)"
    var apiAvailable = false
    var supported = false
    if #available(macOS 15.0, *) {
        apiAvailable = true
        supported = VZGenericPlatformConfiguration.isNestedVirtualizationSupported
    }
    return """
    {"api_available":\(apiAvailable),"nested_virtualization_supported":\(supported),"virtualization_supported":\(VZVirtualMachine.isSupported),"macos":"\(macos)","chip":"\(sysctl("machdep.cpu.brand_string"))","model":"\(sysctl("hw.model"))"}
    """
}

func usage() -> String {
    """
    usage: vesper-vmlauncher [--config PATH] [--ready-pattern TEXT] [--timeout-seconds N] [--keep-running] [--skip-probe]
           vesper-vmlauncher --host-capabilities
      --config PATH          launch config JSON (default build/systemvm/launch-config.json)
      --ready-pattern TEXT   console text that marks a healthy boot (default: ready_pattern from the config, else "login:")
      --timeout-seconds N    give up after N seconds (default 60)
      --keep-running         do not stop the VM after the ready pattern; wait for guest shutdown
      --skip-probe           ignore the config's probe: stop right after the ready pattern
      --host-capabilities    print host facts (nested virtualization support, macOS, chip) as JSON and exit
    """
}

func fail(_ code: Int32, _ message: String) -> Never {
    FileHandle.standardError.write(("vesper-vmlauncher: " + message + "\n").data(using: .utf8)!)
    exit(code)
}

var nestedVirtualizationState = "off"

func parseOptions() -> Options {
    var options = Options()
    var args = Array(CommandLine.arguments.dropFirst())
    while !args.isEmpty {
        let arg = args.removeFirst()
        switch arg {
        case "--config":
            guard !args.isEmpty else { fail(3, "--config needs a value") }
            options.configPath = args.removeFirst()
        case "--ready-pattern":
            guard !args.isEmpty else { fail(3, "--ready-pattern needs a value") }
            options.readyPattern = args.removeFirst()
        case "--timeout-seconds":
            guard !args.isEmpty, let value = Double(args.removeFirst()), value > 0 else {
                fail(3, "--timeout-seconds needs a positive number")
            }
            options.timeoutSeconds = value
        case "--keep-running":
            options.keepRunning = true
        case "--skip-probe":
            options.skipProbe = true
        case "--host-capabilities":
            options.hostCapabilities = true
        case "--help", "-h":
            print(usage())
            exit(0)
        default:
            fail(3, "unknown argument \(arg)\n" + usage())
        }
    }
    if let pattern = options.readyPattern, pattern.isEmpty { fail(3, "--ready-pattern must not be empty") }
    return options
}

func loadConfig(_ path: String) -> LaunchConfig {
    guard let data = FileManager.default.contents(atPath: path) else {
        fail(3, "cannot read launch config \(path)")
    }
    do {
        return try JSONDecoder().decode(LaunchConfig.self, from: data)
    } catch {
        fail(3, "cannot parse launch config \(path): \(error)")
    }
}

func requireFile(_ path: String, _ what: String) -> URL {
    guard FileManager.default.isReadableFile(atPath: path) else {
        fail(3, "\(what) \(path) is missing or unreadable")
    }
    return URL(fileURLWithPath: path)
}

func buildConfiguration(_ config: LaunchConfig, consoleOutput: FileHandle, consoleInput: FileHandle)
    -> VZVirtualMachineConfiguration
{
    let bootLoader = VZLinuxBootLoader(kernelURL: requireFile(config.kernel_path, "kernel"))
    if let initrd = config.initrd_path, !initrd.isEmpty {
        bootLoader.initialRamdiskURL = requireFile(initrd, "initrd")
    }
    bootLoader.commandLine = config.kernel_cmdline

    let vm = VZVirtualMachineConfiguration()
    vm.bootLoader = bootLoader
    // Nested virtualization (GH #17): "auto" enables it when supported, "require" fails otherwise, "off" never.
    let platform = VZGenericPlatformConfiguration()
    let nestedMode = (config.nested_virtualization ?? "auto").lowercased()
    var nestedState = "off"
    if nestedMode != "off" {
        var supported = false
        if #available(macOS 15.0, *) {
            supported = VZGenericPlatformConfiguration.isNestedVirtualizationSupported
            if supported { platform.isNestedVirtualizationEnabled = true }
        }
        if supported {
            nestedState = "enabled"
        } else if nestedMode == "require" {
            fail(3, "nested virtualization is required but this host does not support it (needs macOS 15 or later on an Apple M3 or later chip); use nested_virtualization \"auto\" or \"off\" for a Backend B host")
        } else {
            nestedState = "unsupported"
        }
    } else if nestedMode != "off" && nestedMode != "auto" && nestedMode != "require" {
        fail(3, "nested_virtualization must be auto, require or off, got \(nestedMode)")
    }
    vm.platform = platform
    nestedVirtualizationState = nestedState
    vm.cpuCount = min(
        max(config.cpu_count, VZVirtualMachineConfiguration.minimumAllowedCPUCount),
        VZVirtualMachineConfiguration.maximumAllowedCPUCount)
    vm.memorySize = min(
        max(config.memory_mib * 1024 * 1024, VZVirtualMachineConfiguration.minimumAllowedMemorySize),
        VZVirtualMachineConfiguration.maximumAllowedMemorySize)

    let console = VZVirtioConsoleDeviceSerialPortConfiguration()
    console.attachment = VZFileHandleSerialPortAttachment(
        fileHandleForReading: consoleInput, fileHandleForWriting: consoleOutput)
    vm.serialPorts = [console]

    switch config.network_mode {
    case "nat":
        let net = VZVirtioNetworkDeviceConfiguration()
        net.attachment = VZNATNetworkDeviceAttachment()
        vm.networkDevices = [net]
    case "none":
        vm.networkDevices = []
    default:
        fail(3, "unsupported network_mode \(config.network_mode) (expected nat or none)")
    }

    vm.entropyDevices = [VZVirtioEntropyDeviceConfiguration()]
    vm.socketDevices = [VZVirtioSocketDeviceConfiguration()]

    // virtio-fs shares (GH #29): one device per configured host directory, mounted in the guest by tag.
    var shares: [VZDirectorySharingDeviceConfiguration] = []
    for share in config.shared_directories ?? [] {
        do {
            try VZVirtioFileSystemDeviceConfiguration.validateTag(share.tag)
        } catch {
            fail(3, "shared directory tag \(share.tag) is invalid: \(error)")
        }
        var isDir: ObjCBool = false
        guard FileManager.default.fileExists(atPath: share.host_path, isDirectory: &isDir), isDir.boolValue else {
            fail(3, "shared directory \(share.host_path) is not a directory")
        }
        let device = VZVirtioFileSystemDeviceConfiguration(tag: share.tag)
        device.share = VZSingleDirectoryShare(
            directory: VZSharedDirectory(url: URL(fileURLWithPath: share.host_path), readOnly: share.read_only ?? false))
        shares.append(device)
    }
    vm.directorySharingDevices = shares

    // Block devices in order: boot media (read-only) first, then the writable rootfs, so the
    // kernel command line can name them as /dev/vda and /dev/vdb.
    var disks: [VZStorageDeviceConfiguration] = []
    if let media = config.boot_media_path, !media.isEmpty {
        do {
            let attachment = try VZDiskImageStorageDeviceAttachment(
                url: requireFile(media, "boot media"), readOnly: true)
            disks.append(VZVirtioBlockDeviceConfiguration(attachment: attachment))
        } catch {
            fail(3, "cannot attach boot media \(media): \(error)")
        }
    }
    if let rootfs = config.rootfs_path, !rootfs.isEmpty {
        do {
            let attachment = try VZDiskImageStorageDeviceAttachment(
                url: requireFile(rootfs, "rootfs"), readOnly: false)
            disks.append(VZVirtioBlockDeviceConfiguration(attachment: attachment))
        } catch {
            fail(3, "cannot attach rootfs \(rootfs): \(error)")
        }
    }
    vm.storageDevices = disks

    do {
        try vm.validate()
    } catch {
        fail(3, "launch configuration is invalid: \(error)")
    }
    return vm
}

final class Launcher: NSObject, VZVirtualMachineDelegate {
    let options: Options
    let config: LaunchConfig
    let startedAt = Date()
    var machine: VZVirtualMachine!
    var ready = false
    var tail = Data()
    let log: FileHandle
    let readyPattern: String
    let patternData: Data
    var consoleInput: FileHandle?
    // Probe state machine: the pattern currently awaited and what to type once it appears.
    var probe: ProbeSettings?
    var probeStep = 0  // 0 = waiting for login prompt handled by `ready`; then index into steps
    var awaiting: Data?
    var probeDone = false
    var bridgeSource: DispatchSourceRead?
    var bridgeFd: Int32 = -1
    var signalSource: DispatchSourceSignal?

    init(options: Options, config: LaunchConfig) {
        self.options = options
        self.config = config
        let pattern = options.readyPattern ?? config.ready_pattern ?? "login:"
        if pattern.isEmpty { fail(3, "ready pattern must not be empty") }
        self.readyPattern = pattern
        self.patternData = pattern.data(using: .utf8)!
        self.probe = options.skipProbe ? nil : config.probe
        let logURL = URL(fileURLWithPath: config.console_log_path)
        try? FileManager.default.createDirectory(
            at: logURL.deletingLastPathComponent(), withIntermediateDirectories: true)
        FileManager.default.createFile(atPath: config.console_log_path, contents: nil)
        guard let handle = FileHandle(forWritingAtPath: config.console_log_path) else {
            fail(3, "cannot open console log \(config.console_log_path) for writing")
        }
        self.log = handle
        super.init()
    }

    func elapsedMs() -> Int { Int(Date().timeIntervalSince(startedAt) * 1000) }

    func consume(_ chunk: Data) {
        log.write(chunk)
        tail.append(chunk)
        if tail.count > 65536 { tail.removeFirst(tail.count - 65536) }
        if !ready {
            guard tail.range(of: patternData) != nil else { return }
            ready = true
            report(status: "ready")
            if let probe = probe {
                tail.removeAll()
                startProbe(probe)
                return
            }
            if options.keepRunning {
                report(status: "running")
                return
            }
            stopAndExit(0)
            return
        }
        advanceProbe()
    }

    // Probe steps after login: wait for the shell prompt, type each command and wait for the prompt
    // again, then wait for the success pattern, then type the shutdown command and wait for the
    // guest to stop. Input is only typed after the awaited pattern appeared, so nothing is lost.
    func typeLine(_ line: String) {
        consoleInput?.write((line + "\n").data(using: .utf8)!)
    }

    func expect(_ pattern: String) {
        awaiting = pattern.data(using: .utf8)!
        tail.removeAll()
    }

    func startProbe(_ probe: ProbeSettings) {
        typeLine(probe.login_user)
        probeStep = 0
        expect(probe.shell_prompt)
    }

    func advanceProbe() {
        guard let probe = probe, !probeDone, let pattern = awaiting else { return }
        guard tail.range(of: pattern) != nil else { return }
        // Steps 0..<commands.count: prompt seen, type command i. Step commands.count: all commands
        // typed and prompt seen again, now require the success pattern. Last: shut down.
        if probeStep < probe.commands.count {
            let command = probe.commands[probeStep]
            probeStep += 1
            typeLine(command)
            expect(probe.shell_prompt)
            return
        }
        // The prompt came back after the last command; `tail` holds everything since that command
        // was typed (its echo included), so the success pattern must be in there.
        probeStep += 1
        awaiting = nil
        if tail.range(of: probe.success_pattern.data(using: .utf8)!) == nil {
            report(status: "probe-failed")
            stopAndExit(4)
            return
        }
        probeDone = true
        report(status: "probe-ok")
        if options.keepRunning {
            report(status: "running")
            return
        }
        typeLine(probe.shutdown_command)
    }

    // MARK: vsock bridge

    func startBridge(_ path: String) {
        guard let device = machine.socketDevices.first as? VZVirtioSocketDevice else {
            fail(3, "VM has no virtio socket device")
        }
        let url = URL(fileURLWithPath: path)
        try? FileManager.default.createDirectory(
            at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
        unlink(path)
        let fd = socket(AF_UNIX, SOCK_STREAM, 0)
        guard fd >= 0 else { fail(1, "bridge socket(): \(String(cString: strerror(errno)))") }
        var addr = sockaddr_un()
        addr.sun_family = sa_family_t(AF_UNIX)
        let pathBytes = Array(path.utf8)
        guard pathBytes.count < MemoryLayout.size(ofValue: addr.sun_path) else {
            fail(3, "bridge socket path is too long: \(path)")
        }
        withUnsafeMutablePointer(to: &addr.sun_path) { ptr in
            ptr.withMemoryRebound(to: UInt8.self, capacity: pathBytes.count + 1) { dst in
                for (index, byte) in pathBytes.enumerated() { dst[index] = byte }
                dst[pathBytes.count] = 0
            }
        }
        let bound = withUnsafePointer(to: &addr) { ptr in
            ptr.withMemoryRebound(to: sockaddr.self, capacity: 1) { sa in
                Darwin.bind(fd, sa, socklen_t(MemoryLayout<sockaddr_un>.size))
            }
        }
        guard bound == 0, listen(fd, 16) == 0 else {
            fail(1, "bridge bind/listen \(path): \(String(cString: strerror(errno)))")
        }
        // The bridge reaches any guest vsock port, so only the owner may use it.
        chmod(path, 0o600)
        bridgeFd = fd
        let source = DispatchSource.makeReadSource(fileDescriptor: fd, queue: .main)
        source.setEventHandler { [weak self] in
            guard let self = self else { return }
            let client = accept(fd, nil, nil)
            if client >= 0 {
                var one: Int32 = 1
                setsockopt(client, SOL_SOCKET, SO_NOSIGPIPE, &one, socklen_t(MemoryLayout<Int32>.size))
                self.handleBridgeClient(client, device)
            }
        }
        source.resume()
        bridgeSource = source
        report(status: "bridge-listening", extra: "\"bridge\":\"\(path)\"")
    }

    func handleBridgeClient(_ client: Int32, _ device: VZVirtioSocketDevice) {
        DispatchQueue.global().async {
            var line = [UInt8]()
            var byte: UInt8 = 0
            while line.count < 64 {
                let n = read(client, &byte, 1)
                if n <= 0 { close(client); return }
                if byte == 10 { break }
                line.append(byte)
            }
            guard let text = String(bytes: line, encoding: .utf8),
                text.hasPrefix("CONNECT "),
                let port = UInt32(text.dropFirst(8).trimmingCharacters(in: .whitespaces))
            else {
                FileHandle.standardError.write("vesper-vmlauncher: bridge: bad preface\n".data(using: .utf8)!)
                close(client)
                return
            }
            DispatchQueue.main.async {
                device.connect(toPort: port) { result in
                    switch result {
                    case .success(let connection):
                        self.relay(client: client, connection: connection)
                    case .failure(let error):
                        FileHandle.standardError.write(
                            "vesper-vmlauncher: bridge: connect to guest port \(port) failed: \(error)\n".data(using: .utf8)!)
                        close(client)
                    }
                }
            }
        }
    }

    func relay(client: Int32, connection: VZVirtioSocketConnection) {
        let guest = connection.fileDescriptor
        let group = DispatchGroup()
        for (source, sink) in [(client, guest), (guest, client)] {
            group.enter()
            DispatchQueue.global().async {
                var buffer = [UInt8](repeating: 0, count: 65536)
                outer: while true {
                    let n = read(source, &buffer, buffer.count)
                    if n <= 0 { break }
                    var offset = 0
                    while offset < n {
                        let written = buffer.withUnsafeBytes { raw in
                            write(sink, raw.baseAddress! + offset, n - offset)
                        }
                        if written <= 0 { break outer }
                        offset += written
                    }
                }
                shutdown(sink, SHUT_WR)
                group.leave()
            }
        }
        group.notify(queue: .main) {
            close(client)
            connection.close()
        }
    }

    func installSignalHandler() {
        // A bridge client (vesper-proxy) that dies while the guest is still sending makes the relay's
        // write() fail with EPIPE; without this the default SIGPIPE action would kill the VM with it.
        signal(SIGPIPE, SIG_IGN)
        signal(SIGTERM, SIG_IGN)
        let source = DispatchSource.makeSignalSource(signal: SIGTERM, queue: .main)
        source.setEventHandler { [weak self] in
            guard let self = self else { exit(0) }
            self.report(status: "terminating")
            self.stopAndExit(self.ready ? 0 : 1)
        }
        source.resume()
        signalSource = source
    }

    func report(status: String, extra: String = "") {
        let tail = extra.isEmpty ? "" : "," + extra
        let line = """
        {"status":"\(status)","elapsed_ms":\(elapsedMs()),"ready_pattern":"\(readyPattern)","console_log":"\(config.console_log_path)"\(tail)}
        """
        print(line)
        fflush(stdout)
    }

    func stopAndExit(_ code: Int32) {
        if bridgeFd >= 0 {
            bridgeSource?.cancel()
            close(bridgeFd)
            if let path = config.vsock_bridge_path { unlink(path) }
            bridgeFd = -1
        }
        guard machine.canStop else { exit(code) }
        machine.stop { _ in exit(code) }
    }

    func run() {
        let output = Pipe()
        let input = Pipe()
        let configuration = buildConfiguration(
            config, consoleOutput: output.fileHandleForWriting, consoleInput: input.fileHandleForReading)
        machine = VZVirtualMachine(configuration: configuration, queue: .main)
        machine.delegate = self
        consoleInput = input.fileHandleForWriting

        output.fileHandleForReading.readabilityHandler = { handle in
            let chunk = handle.availableData
            if chunk.isEmpty { return }
            DispatchQueue.main.async { self.consume(chunk) }
        }

        DispatchQueue.main.asyncAfter(deadline: .now() + options.timeoutSeconds) {
            if self.ready && (self.probe == nil || self.probeDone) { return }
            self.report(status: self.ready ? "probe-timeout" : "timeout")
            self.stopAndExit(2)
        }

        installSignalHandler()
        report(status: "platform", extra: "\"nested_virtualization\":\"\(nestedVirtualizationState)\"")
        machine.start { result in
            if case .failure(let error) = result {
                FileHandle.standardError.write(
                    "vesper-vmlauncher: start failed: \(error)\n".data(using: .utf8)!)
                self.report(status: "start-failed")
                exit(1)
            }
            if let bridge = self.config.vsock_bridge_path, !bridge.isEmpty {
                self.startBridge(bridge)
            }
        }
        dispatchMain()
    }

    func guestDidStop(_ virtualMachine: VZVirtualMachine) {
        if let path = config.vsock_bridge_path, bridgeFd >= 0 {
            bridgeSource?.cancel()
            close(bridgeFd)
            unlink(path)
            bridgeFd = -1
        }
        let ok = ready && (probe == nil || probeDone)
        report(status: ok ? "stopped" : (ready ? "stopped-before-probe" : "stopped-before-ready"))
        exit(ok ? 0 : 1)
    }

    func virtualMachine(_ virtualMachine: VZVirtualMachine, didStopWithError error: Error) {
        FileHandle.standardError.write(
            "vesper-vmlauncher: VM stopped with error: \(error)\n".data(using: .utf8)!)
        report(status: "error")
        exit(1)
    }
}

let options = parseOptions()
if options.hostCapabilities {
    print(hostCapabilitiesJSON())
    exit(0)
}
let config = loadConfig(options.configPath)
guard VZVirtualMachine.isSupported else {
    fail(3, "Virtualization.framework is not supported on this host")
}
Launcher(options: options, config: config).run()
