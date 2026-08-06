// swift-tools-version: 5.9
//
// A package rather than an Xcode project, because a project file is generated
// noise that cannot be reviewed. `swift build` checks the client compiles;
// wrapping it in an app target is the next person's first task.

import PackageDescription

let package = Package(
    name: "Omt",
    platforms: [.iOS(.v16), .macOS(.v13)],
    products: [
        .library(name: "OmtClient", targets: ["OmtClient"]),
        // An executable rather than a test target, so the assertions run with
        // the command line tools alone. XCTest needs a full Xcode, and a check
        // that only runs on one machine is a check nobody runs.
        .executable(name: "omt-client-check", targets: ["OmtClientCheck"])
    ],
    targets: [
        .target(name: "OmtClient", path: "Omt/Sources/Omt"),
        .executableTarget(
            name: "OmtClientCheck",
            dependencies: ["OmtClient"],
            path: "Omt/Check"
        )
    ]
)
