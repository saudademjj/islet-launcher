fn main() {
    // settings.rs, curseforge.rs and update.rs bake distribution defaults in at compile time.
    println!("cargo:rerun-if-env-changed=MC_LAUNCHER_CLIENT_ID");
    println!("cargo:rerun-if-env-changed=MC_LAUNCHER_CURSEFORGE_KEY");
    println!("cargo:rerun-if-env-changed=MC_LAUNCHER_CURSEFORGE_URL");
    println!("cargo:rerun-if-env-changed=MC_LAUNCHER_UPDATE_URL");
}
