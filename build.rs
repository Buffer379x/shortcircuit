use std::{env, fs, io, path::PathBuf};

fn main() -> io::Result<()> {
    println!("cargo:rerun-if-changed=migrations");
    println!("cargo:rerun-if-changed=frontend/dist");
    // Pass the build revision explicitly and rebuild when it changes.
    println!("cargo:rerun-if-env-changed=SHORTCIRCUIT_BUILD_REVISION");
    if let Ok(revision) = env::var("SHORTCIRCUIT_BUILD_REVISION") {
        println!("cargo:rustc-env=SHORTCIRCUIT_BUILD_REVISION={revision}");
    }
    let out = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo supplies OUT_DIR"));
    let ready = fs::exists("frontend/dist/index.html")?;
    for (source, file) in [
        ("index.html", "admin.html"),
        ("assets/admin.js", "admin.js"),
        ("assets/admin.css", "admin.css"),
        // Local Geist fonts (SIL Open Font License), named by frontend/vite.config.ts.
        ("assets/geist-latin-wght-normal.woff2", "geist-latin.woff2"),
        (
            "assets/geist-latin-ext-wght-normal.woff2",
            "geist-latin-ext.woff2",
        ),
        (
            "assets/geist-mono-latin-wght-normal.woff2",
            "geist-mono-latin.woff2",
        ),
    ] {
        let bytes = if ready {
            fs::read(format!("frontend/dist/{source}"))?
        } else {
            Vec::new()
        };
        fs::write(out.join(file), bytes)?;
    }
    Ok(())
}
