// swift-tools-version:5.9
// Thin Virtualization.framework helper for the Vesper System VM (WOR-27, GH #12).
// Logic stays in Rust (src/systemvm.rs); this package only boots a launch config.
import PackageDescription

let package = Package(
    name: "vesper-vmlauncher",
    platforms: [.macOS(.v13)],
    targets: [
        .executableTarget(
            name: "vesper-vmlauncher",
            path: "Sources/vesper-vmlauncher",
            linkerSettings: [.linkedFramework("Virtualization")]
        )
    ],
    swiftLanguageVersions: [.v5]
)
