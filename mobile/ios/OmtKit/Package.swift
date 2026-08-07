// swift-tools-version: 5.9
//
// A package rather than an Xcode project, because a project file is generated
// noise that cannot be reviewed. `swift build` checks the client compiles;
// wrapping it in an app target is the next person's first task.

import PackageDescription

let package = Package(
    // Named apart from the app target on purpose. Two things called `Omt` —
    // the package and the Xcode project — share a DerivedData directory, and
    // the build quietly produces the package's bare executable instead of the
    // app bundle. It looks like a successful build right up until you try to
    // install it.
    name: "OmtKit",
    // macOS 14 as well as iOS 17, because the views use APIs that arrived
    // together on both — and building on a Mac is what makes these compile in
    // CI rather than only in Xcode.
    platforms: [.iOS(.v17), .macOS(.v14)],
    products: [
        .library(name: "OmtClient", targets: ["OmtClient"]),
        .library(name: "OmtApp", targets: ["OmtApp"]),
        // An executable rather than a test target, so the assertions run with
        // the command line tools alone. XCTest needs a full Xcode, and a check
        // that only runs on one machine is a check nobody runs.
        // The app is defined in `project.yml` and built from the generated
        // Xcode project, not here. A SwiftPM executable called `Omt` shadowed
        // the app target of the same name and the build quietly produced a
        // command-line binary instead of an `.app` — which looks like success
        // right up until you try to install it.
        .executable(name: "omt-client-check", targets: ["OmtClientCheck"])
    ],
    targets: [
        .target(name: "OmtClient", path: "Sources/Omt"),
        // The app's own code, as a library. An `.iOSApplication` product needs
        // Xcode; keeping the views here means they are compiled by `swift
        // build` on any machine, which is where the mistakes actually are.
        .target(
            name: "OmtApp",
            dependencies: ["OmtClient"],
            path: "Sources/OmtApp",
            // The icon travels with the code rather than living only in an
            // Xcode project, so a checkout has everything an app needs and the
            // project file is not a place state hides.
            resources: [.copy("../../Resources/Assets.xcassets")]
        ),
        .executableTarget(
            name: "OmtClientCheck",
            dependencies: ["OmtClient", "OmtApp"],
            path: "Check"
        )
    ]
)
