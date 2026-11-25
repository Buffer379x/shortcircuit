use std::{env, fs, io, path::PathBuf};

fn main() -> io::Result<()> {
    println!("cargo:rerun-if-changed=migrations");
    println!("cargo:rerun-if-changed=frontend/dist");
    let out = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo supplies OUT_DIR"));
    let ready = fs::exists("frontend/dist/index.html")?;
    for (source, file) in [
        ("index.html", "admin.html"),
        ("assets/admin.js", "admin.js"),
        ("assets/admin.css", "admin.css"),
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
