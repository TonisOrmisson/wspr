// Embeds the icon, manifest and version info into wspr.exe on Windows targets.
fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let res = std::path::Path::new(&std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("res");
    let ver = std::env::var("CARGO_PKG_VERSION").unwrap();
    let nums = ver.replace('.', ",");
    let rc = format!(
        r#"1 ICON "{icon}"
1 24 "{manifest}"
1 VERSIONINFO
FILEVERSION {nums},0
PRODUCTVERSION {nums},0
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904b0"
    BEGIN
      VALUE "FileDescription", "wspr dictation"
      VALUE "ProductName", "wspr"
      VALUE "FileVersion", "{ver}"
      VALUE "ProductVersion", "{ver}"
      VALUE "OriginalFilename", "wspr.exe"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x409, 1200
  END
END
"#,
        icon = res
            .join("wspr.ico")
            .display()
            .to_string()
            .replace('\\', "/"),
        manifest = res
            .join("wspr.manifest")
            .display()
            .to_string()
            .replace('\\', "/"),
    );
    let out = std::path::Path::new(&std::env::var("OUT_DIR").unwrap()).join("wspr.rc");
    std::fs::write(&out, rc).unwrap();
    println!("cargo:rerun-if-changed=res");
    embed_resource::compile(&out, embed_resource::NONE)
        .manifest_required()
        .unwrap();
}
