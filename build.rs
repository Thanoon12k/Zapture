use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=assets/cadre.rc");
    println!("cargo:rerun-if-changed=assets/cadre.ico");
    println!("cargo:rerun-if-changed=assets/cadre-rec.ico");
    println!("cargo:rerun-if-env-changed=CARGO_PKG_VERSION");
    // The icon/version resources need rc.exe (or windres); embed them when
    // building on Windows for Windows, skip quietly everywhere else.
    if env::var_os("CARGO_CFG_WINDOWS").is_some() {
        let rc = stamp_rc();
        embed_resource::compile(&rc, embed_resource::NONE);
    }
}

/// Copy assets/cadre.rc into OUT_DIR with every version field replaced by
/// CARGO_PKG_VERSION, so the version is maintained in Cargo.toml alone.
/// Icon references become absolute paths because the stamped copy no longer
/// sits next to the .ico files.
fn stamp_rc() -> PathBuf {
    let manifest = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let assets = manifest.join("assets");
    let version = env::var("CARGO_PKG_VERSION").expect("CARGO_PKG_VERSION");
    let mut nums = version.split('.').map(|p| {
        p.chars().take_while(|c| c.is_ascii_digit()).collect::<String>().parse::<u32>().unwrap_or(0)
    });
    let (maj, min, pat) =
        (nums.next().unwrap_or(0), nums.next().unwrap_or(0), nums.next().unwrap_or(0));

    let src = fs::read_to_string(assets.join("cadre.rc")).expect("read assets/cadre.rc");
    let mut out = String::with_capacity(src.len() + 64);
    for line in src.lines() {
        let t = line.trim_start();
        if t.starts_with("FILEVERSION") {
            out.push_str(&format!("FILEVERSION {maj},{min},{pat},0"));
        } else if t.starts_with("PRODUCTVERSION") {
            out.push_str(&format!("PRODUCTVERSION {maj},{min},{pat},0"));
        } else if t.contains("VALUE \"FileVersion\"") {
            out.push_str(&format!("      VALUE \"FileVersion\", \"{version}\""));
        } else if t.contains("VALUE \"ProductVersion\"") {
            out.push_str(&format!("      VALUE \"ProductVersion\", \"{version}\""));
        } else if t.contains(" ICON ") {
            // `1 ICON "cadre.ico"` → `1 ICON "<abs>/assets/cadre.ico"`.
            match line.split_once('"').and_then(|(head, rest)| {
                rest.split_once('"').map(|(name, _)| (head, assets.join(name)))
            }) {
                Some((head, icon)) => out.push_str(&format!(
                    "{head}\"{}\"",
                    icon.display().to_string().replace('\\', "/")
                )),
                None => out.push_str(line),
            }
        } else {
            out.push_str(line);
        }
        out.push('\n');
    }

    let dst = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR")).join("cadre.rc");
    fs::write(&dst, out).expect("write stamped cadre.rc");
    dst
}
