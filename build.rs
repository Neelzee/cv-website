use std::process::Command;

fn main() {
    // `embed_migrations!` only reads the folder at compile time, so a new migration has to trigger a rebuild
    println!("cargo:rerun-if-changed=migrations");
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/refs");
    println!("cargo:rerun-if-env-changed=GITHUB_SHA");

    // CI containers may not trust the checkout owner, so prefer the sha GitHub hands us
    let hash = std::env::var("GITHUB_SHA")
        .ok()
        .map(|sha| sha.chars().take(7).collect::<String>())
        .or_else(|| git(&["rev-parse", "--short=7", "HEAD"]))
        .unwrap_or_else(|| "unknown".to_string());
    println!("cargo:rustc-env=GIT_HASH={hash}");

    let subject = git(&["log", "-1", "--format=%s"]).unwrap_or_default();
    println!("cargo:rustc-env=GIT_SUBJECT={subject}");
}

fn git(args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}
