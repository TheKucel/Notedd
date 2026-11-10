//! Windows: embeds the icon and the version info (Explorer, the taskbar and shortcuts show them).
//! The manifest comes from GPUI (its `windows-manifest` feature).

use std::path::{Path, PathBuf};

/// rc.exe from the newest Windows SDK
fn rc_exe() -> Option<PathBuf> {
    let base = Path::new(r"C:\Program Files (x86)\Windows Kits\10\bin");
    let mut versions: Vec<PathBuf> = std::fs::read_dir(base).ok()?.flatten().map(|e| e.path())
        .filter(|p| p.join("x64").join("rc.exe").exists()).collect();
    versions.sort();
    versions.pop().map(|p| p.join("x64").join("rc.exe"))
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") { return; }
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let icon = dir.join("..").join("assets").join("notedd.ico");
    println!("cargo:rerun-if-changed={}", icon.display());
    let version = env!("CARGO_PKG_VERSION");
    let nums: Vec<&str> = version.split('.').chain(std::iter::repeat("0")).take(4).collect();
    let commas = nums.join(",");
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let rc = out.join("notedd.rc");
    let res = out.join("notedd.res");
    let text = format!(r#"1 ICON "{icon}"
1 VERSIONINFO
FILEVERSION {commas}
PRODUCTVERSION {commas}
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "041904B0"
    BEGIN
      VALUE "CompanyName", "Notedd"
      VALUE "FileDescription", "Notedd"
      VALUE "FileVersion", "{version}"
      VALUE "ProductName", "Notedd"
      VALUE "ProductVersion", "{version}"
      VALUE "OriginalFilename", "notedd.exe"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x419, 1200
  END
END
"#, icon = icon.display().to_string().replace('\\', "\\\\"));
    std::fs::write(&rc, text).unwrap();
    let Some(rc_exe) = rc_exe() else { println!("cargo:warning=rc.exe not found: no icon in the exe"); return };
    let ok = std::process::Command::new(rc_exe).arg("/nologo").arg("/fo").arg(&res).arg(&rc).status().map(|s| s.success()).unwrap_or(false);
    if ok { println!("cargo:rustc-link-arg-bins={}", res.display()); } else { println!("cargo:warning=rc.exe failed: no icon in the exe"); }
}
