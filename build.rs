fn main() {
    // `embed_migrations!` only reads the folder at compile time, so a new migration has to trigger a rebuild
    println!("cargo:rerun-if-changed=migrations");
}
