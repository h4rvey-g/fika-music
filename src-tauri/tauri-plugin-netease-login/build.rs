fn main() {
    // No JavaScript commands: this bridge is called only by the host login flow.
    tauri_plugin::Builder::new(&[])
        .android_path("android")
        .ios_path("ios")
        .build();
}
