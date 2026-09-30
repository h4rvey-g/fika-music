// swift-tools-version:5.3
import PackageDescription

let package = Package(
    name: "tauri-plugin-netease-login",
    platforms: [.iOS(.v13)],
    products: [.library(name: "tauri-plugin-netease-login", type: .static, targets: ["tauri-plugin-netease-login"])],
    dependencies: [.package(name: "Tauri", path: "../.tauri/tauri-api")],
    targets: [.target(name: "tauri-plugin-netease-login", dependencies: [.byName(name: "Tauri")], path: "Sources")]
)

