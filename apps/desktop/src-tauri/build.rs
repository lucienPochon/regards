fn main() {
    // The icons are compiled into the program: the .exe's own on Windows, the
    // window's and the tray's everywhere. tauri-build watches only its config,
    // so without this a changed icon leaves the old one in the next build.
    println!("cargo:rerun-if-changed=icons");
    tauri_build::build();
}
