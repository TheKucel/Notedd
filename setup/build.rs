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
    // the app the installer carries: build it first (cargo build --release in ../app),
    // or point NOTEDD_APP_EXE at a ready notedd.exe
    println!("cargo:rerun-if-env-changed=NOTEDD_APP_EXE");
    let app = std::env::var_os("NOTEDD_APP_EXE").map(PathBuf::from)
        .unwrap_or_else(|| dir.join("..").join("app").join("target").join("release").join("notedd.exe"));
    if !app.exists() { panic!("build Notedd first: {} is missing", app.display()); }
    println!("cargo:rerun-if-changed={}", app.display());
    println!("cargo:rustc-env=NOTEDD_EXE={}", app.display());
    let m = dir.join("setup.manifest");
    println!("cargo:rerun-if-changed=setup.manifest");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rustc-link-arg-bins=/MANIFEST:EMBED");
    println!("cargo:rustc-link-arg-bins=/MANIFESTINPUT:{}", m.display());

    let icon = dir.join("..").join("assets").join("notedd.ico");
    let license = dir.join("..").join("LICENSE");
    let font_license = dir.join("..").join("assets").join("fonts").join("OFL-Geologica.txt");
    println!("cargo:rerun-if-changed={}", icon.display());
    println!("cargo:rerun-if-changed={}", license.display());
    println!("cargo:rerun-if-changed={}", font_license.display());
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let rc = out.join("setup.rc");
    let res = out.join("setup.res");
    let rc_path = |path: &Path| path.display().to_string().replace('\\', "\\\\");
    let text = format!(r#"1 ICON "{}"
2 RCDATA "{}"
3 RCDATA "{}"
1 VERSIONINFO
FILEVERSION 2,1,1,0
PRODUCTVERSION 2,1,1,0
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "041904B0"
    BEGIN
      VALUE "CompanyName", "Notedd"
      VALUE "FileDescription", "Установка Notedd"
      VALUE "FileVersion", "2.1.1"
      VALUE "ProductName", "Notedd"
      VALUE "ProductVersion", "2.1.1"
      VALUE "OriginalFilename", "notedd-setup.exe"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x419, 1200
  END
END
"#, rc_path(&icon), rc_path(&license), rc_path(&font_license));
    std::fs::write(&rc, text).unwrap();
    let Some(rc_exe) = rc_exe() else { println!("cargo:warning=rc.exe not found: no icon in the exe"); return };
    let ok = std::process::Command::new(rc_exe).arg("/nologo").arg("/fo").arg(&res).arg(&rc).status().map(|s| s.success()).unwrap_or(false);
    if ok { println!("cargo:rustc-link-arg-bins={}", res.display()); } else { println!("cargo:warning=rc.exe failed: no icon in the exe"); }
}
