fn main() {
    // settings.rs bakes the distribution's Azure client ID in at compile time.
    println!("cargo:rerun-if-env-changed=MC_LAUNCHER_CLIENT_ID");
}
