//! Windows only: embed the app icon and version info into biblewolf.exe.

use std::{env, fs, path::PathBuf};

fn main() {
    println!("cargo:rerun-if-changed=assets/icon/biblewolf.ico");
    println!("cargo:rerun-if-changed=build.rs");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    // Absolute path so rc.exe doesn't depend on its working directory;
    // backslashes are escaped because .rc strings use C escapes.
    let ico = manifest
        .join("assets")
        .join("icon")
        .join("biblewolf.ico")
        .display()
        .to_string()
        .replace('/', "\\")
        .replace('\\', "\\\\");
    let version = env::var("CARGO_PKG_VERSION").unwrap();
    let mut parts = version
        .split(['.', '-'])
        .map(|p| p.parse::<u16>().unwrap_or(0));
    let (ma, mi, pa) = (
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
    );
    let rc = format!(
        r#"1 ICON "{ico}"

1 VERSIONINFO
FILEVERSION {ma},{mi},{pa},0
PRODUCTVERSION {ma},{mi},{pa},0
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "080904B0"
    BEGIN
      VALUE "CompanyName", "IntelligentWolf Ltd"
      VALUE "FileDescription", "BibleWolf"
      VALUE "FileVersion", "{version}"
      VALUE "InternalName", "biblewolf"
      VALUE "LegalCopyright", "(C) 2026 IntelligentWolf Ltd. MIT License."
      VALUE "OriginalFilename", "biblewolf.exe"
      VALUE "ProductName", "BibleWolf"
      VALUE "ProductVersion", "{version}"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x809, 1200
  END
END
"#
    );
    let out = PathBuf::from(env::var("OUT_DIR").unwrap()).join("biblewolf.rc");
    fs::write(&out, rc).expect("write biblewolf.rc");
    embed_resource::compile(&out, embed_resource::NONE)
        .manifest_required()
        .expect("embedding the Windows icon failed");
}
