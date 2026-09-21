// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "Convoy",
    platforms: [.macOS(.v14)],
    products: [.executable(name: "Convoy", targets: ["Convoy"]), .executable(name: "ConvoyStatus", targets: ["ConvoyStatus"])],
    dependencies: [.package(url: "https://github.com/migueldeicaza/SwiftTerm.git", exact: "1.20.0")],
    targets: [
        .target(name: "UsageTelemetry"),
        .executableTarget(name: "ConvoyStatus", dependencies: ["UsageTelemetry"]),
        .executableTarget(name: "Convoy", dependencies: ["SwiftTerm", "UsageTelemetry"]),
        .testTarget(name: "ConvoyTests", dependencies: ["Convoy", "UsageTelemetry"])
    ]
)
