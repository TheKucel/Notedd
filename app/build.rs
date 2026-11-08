//! Embeds the manifest, the icon and the version info (Explorer, the taskbar and shortcuts show them).

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
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let m = dir.join("notedd.manifest");
    println!("cargo:rerun-if-changed=notedd.manifest");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rustc-link-arg-bins=/MANIFEST:EMBED");
    println!("cargo:rustc-link-arg-bins=/MANIFESTINPUT:{}", m.display());

    let icon = dir.join("..").join("assets").join("notedd.ico");
    println!("cargo:rerun-if-changed={}", icon.display());
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let rc = out.join("notedd.rc");
    let res = out.join("notedd.res");
    let text = format!(r#"1 ICON "{}"
1 VERSIONINFO
FILEVERSION 1,2,0,0
PRODUCTVERSION 1,2,0,0
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "041904B0"
    BEGIN
      VALUE "CompanyName", "Notedd"
      VALUE "FileDescription", "Notedd"
      VALUE "FileVersion", "1.2.0"
      VALUE "ProductName", "Notedd"
      VALUE "ProductVersion", "1.2.0"
      VALUE "OriginalFilename", "notedd.exe"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x419, 1200
  END
END
"#, icon.display().to_string().replace('\\', "\\\\"));
    std::fs::write(&rc, text).unwrap();
    let Some(rc_exe) = rc_exe() else { println!("cargo:warning=rc.exe not found: no icon in the exe"); return };
    let ok = std::process::Command::new(rc_exe).arg("/nologo").arg("/fo").arg(&res).arg(&rc).status().map(|s| s.success()).unwrap_or(false);
    if ok { println!("cargo:rustc-link-arg-bins={}", res.display()); } else { println!("cargo:warning=rc.exe failed: no icon in the exe"); }
}
