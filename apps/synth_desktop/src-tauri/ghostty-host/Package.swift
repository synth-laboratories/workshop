// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "SynthGhosttyHost",
    platforms: [.macOS(.v14)],
    products: [
        .library(
            name: "SynthGhosttyHost",
            type: .dynamic,
            targets: ["SynthGhosttyHost"]
        ),
    ],
    dependencies: [
        .package(
            url: "https://github.com/Lakr233/libghostty-spm.git",
            exact: "1.5.2"
        ),
    ],
    targets: [
        .target(
            name: "SynthGhosttyHost",
            dependencies: [
                .product(name: "GhosttyTerminal", package: "libghostty-spm"),
            ]
        ),
    ]
)
