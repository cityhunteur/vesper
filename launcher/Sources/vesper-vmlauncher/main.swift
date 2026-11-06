// vesper-vmlauncher: boot one Vesper System VM launch config on Virtualization.framework.
//
// Reads build/systemvm/launch-config.json (written by `systemvmctl launch-config`), boots the
// kernel with the initramfs and/or block rootfs it names, attaches a NAT network device and a
// virtio console, streams the console to `console_log_path`, and exits as soon as the console
// shows `--ready-pattern` (default "login:").
//
// Exit codes: 0 ready pattern seen and VM stopped; 1 VM failed to start or stopped before ready;
// 2 timeout waiting for the ready pattern; 3 bad arguments or configuration.

import Foundation
import Virtualization

struct LaunchConfig: Decodable {
    let kernel_path: String
    let rootfs_path: String?
    let initrd_path: String?
    let boot_media_path: String?
    let kernel_cmdline: String
    let ready_pattern: String?
    let network_mode: String
    let cpu_count: Int
    let memory_mib: UInt64
    let console_log_path: String
}

struct Options {
    var configPath = "build/systemvm/launch-config.json"
    var readyPattern: String? = nil
    var timeoutSeconds: Double = 60
    var keepRunning = false
}

func usage() -> String {
    """
    usage: vesper-vmlauncher [--config PATH] [--ready-pattern TEXT] [--timeout-seconds N] [--keep-running]
      --config PATH          launch config JSON (default build/systemvm/launch-config.json)
      --ready-pattern TEXT   console text that marks a healthy boot (default: ready_pattern from the config, else "login:")
      --timeout-seconds N    give up after N seconds (default 60)
      --keep-running         do not stop the VM after the ready pattern; wait for guest shutdown
    """
}

func fail(_ code: Int32, _ message: String) -> Never {
    FileHandle.standardError.write(("vesper-vmlauncher: " + message + "\n").data(using: .utf8)!)
    exit(code)
}

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

    init(options: Options, config: LaunchConfig) {
        self.options = options
        self.config = config
        let pattern = options.readyPattern ?? config.ready_pattern ?? "login:"
        if pattern.isEmpty { fail(3, "ready pattern must not be empty") }
        self.readyPattern = pattern
        self.patternData = pattern.data(using: .utf8)!
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
        if !ready, tail.range(of: patternData) != nil {
            ready = true
            report(status: "ready")
            if options.keepRunning { return }
            stopAndExit(0)
        }
    }

    func report(status: String) {
        let line = """
        {"status":"\(status)","elapsed_ms":\(elapsedMs()),"ready_pattern":"\(readyPattern)","console_log":"\(config.console_log_path)"}
        """
        print(line)
        fflush(stdout)
    }

    func stopAndExit(_ code: Int32) {
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

        output.fileHandleForReading.readabilityHandler = { handle in
            let chunk = handle.availableData
            if chunk.isEmpty { return }
            DispatchQueue.main.async { self.consume(chunk) }
        }

        DispatchQueue.main.asyncAfter(deadline: .now() + options.timeoutSeconds) {
            if self.ready { return }
            self.report(status: "timeout")
            self.stopAndExit(2)
        }

        machine.start { result in
            if case .failure(let error) = result {
                FileHandle.standardError.write(
                    "vesper-vmlauncher: start failed: \(error)\n".data(using: .utf8)!)
                self.report(status: "start-failed")
                exit(1)
            }
        }
        dispatchMain()
    }

    func guestDidStop(_ virtualMachine: VZVirtualMachine) {
        report(status: ready ? "stopped" : "stopped-before-ready")
        exit(ready ? 0 : 1)
    }

    func virtualMachine(_ virtualMachine: VZVirtualMachine, didStopWithError error: Error) {
        FileHandle.standardError.write(
            "vesper-vmlauncher: VM stopped with error: \(error)\n".data(using: .utf8)!)
        report(status: "error")
        exit(1)
    }
}

let options = parseOptions()
let config = loadConfig(options.configPath)
guard VZVirtualMachine.isSupported else {
    fail(3, "Virtualization.framework is not supported on this host")
}
Launcher(options: options, config: config).run()
