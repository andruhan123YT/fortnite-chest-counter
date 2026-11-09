//! Embeds the app icon and version info into the Windows executable.

use std::path::Path;

#[path = "src/icon.rs"]
#[allow(dead_code)]
mod icon;

fn main() {
    println!("cargo:rerun-if-changed=src/icon.rs");
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let out_dir = std::env::var("OUT_DIR").expect("cargo sets OUT_DIR");
    let out_dir = Path::new(&out_dir);
    let ico = out_dir.join("app.ico");
    std::fs::write(&ico, ico_file(&[16, 24, 32, 48, 64, 128, 256])).expect("write icon");

    let version = env!("CARGO_PKG_VERSION");
    let numeric = version.split('.').chain(std::iter::repeat("0")).take(4).collect::<Vec<_>>().join(",");
    let rc = format!(
        r#"1 ICON "{icon}"
1 VERSIONINFO
FILEVERSION {numeric}
PRODUCTVERSION {numeric}
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904b0"
    BEGIN
      VALUE "FileDescription", "Fortnite Chest Counter"
      VALUE "ProductName", "Fortnite Chest Counter"
      VALUE "FileVersion", "{version}"
      VALUE "ProductVersion", "{version}"
      VALUE "OriginalFilename", "fortnite-chest-counter.exe"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x409, 1200
  END
END
"#,
        icon = ico.display().to_string().replace('\\', "/"),
    );
    let rc_path = out_dir.join("app.rc");
    std::fs::write(&rc_path, rc).expect("write resource script");
    embed_resource::compile(&rc_path, embed_resource::NONE).manifest_optional().expect("compile resources");
}

/// A multi-size `.ico` with 32-bit BMP entries.
fn ico_file(sizes: &[u32]) -> Vec<u8> {
    let images: Vec<Vec<u8>> = sizes.iter().map(|&s| bmp_entry(s)).collect();
    let mut out = Vec::new();
    out.extend_from_slice(&[0, 0, 1, 0]);
    out.extend_from_slice(&(sizes.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * sizes.len() as u32;
    for (&size, image) in sizes.iter().zip(&images) {
        let dim = if size >= 256 { 0 } else { size as u8 };
        out.extend_from_slice(&[dim, dim, 0, 0]);
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&32u16.to_le_bytes());
        out.extend_from_slice(&(image.len() as u32).to_le_bytes());
        out.extend_from_slice(&offset.to_le_bytes());
        offset += image.len() as u32;
    }
    for image in images {
        out.extend_from_slice(&image);
    }
    out
}

fn bmp_entry(size: u32) -> Vec<u8> {
    let rgba = icon::chest_rgba(size);
    let mask_row = (size as usize).div_ceil(32) * 4;
    let mut out = Vec::new();
    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&(size as i32).to_le_bytes());
    out.extend_from_slice(&(size as i32 * 2).to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&32u16.to_le_bytes());
    out.extend_from_slice(&[0; 24]);
    // Rows are stored bottom-up as BGRA.
    for y in (0..size as usize).rev() {
        for x in 0..size as usize {
            let i = (y * size as usize + x) * 4;
            out.extend_from_slice(&[rgba[i + 2], rgba[i + 1], rgba[i], rgba[i + 3]]);
        }
    }
    // The AND mask is unused with 32-bit images; alpha does the job.
    out.extend(std::iter::repeat_n(0u8, mask_row * size as usize));
    out
}
