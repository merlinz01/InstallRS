use std::path::Path;

use anyhow::{Context, Result};
use image::imageops::FilterType;
use ico::{IconDir, IconDirEntry, IconImage, ResourceType};

/// Convert a PNG file to a multi-resolution `.ico` file.
///
/// Each entry in `sizes` produces one icon frame (must be 1..=256).
pub fn png_to_ico(png_path: &Path, dest_path: &Path, sizes: &[u32]) -> Result<()> {
    let img = image::open(png_path)
        .with_context(|| format!("failed to open PNG: {}", png_path.display()))?;

    let mut icon_dir = IconDir::new(ResourceType::Icon);

    for &size in sizes {
        let resized = img.resize_exact(size, size, FilterType::Lanczos3);
        let rgba = resized.to_rgba8();
        let (w, h) = rgba.dimensions();
        let icon_image = IconImage::from_rgba_data(w, h, rgba.into_raw());
        let entry = IconDirEntry::encode(&icon_image)
            .with_context(|| format!("failed to encode {size}x{size} icon frame"))?;
        icon_dir.add_entry(entry);
    }

    let out_file = std::fs::File::create(dest_path)
        .with_context(|| format!("failed to create ICO: {}", dest_path.display()))?;
    icon_dir
        .write(out_file)
        .with_context(|| format!("failed to write ICO: {}", dest_path.display()))?;

    log::info!(
        "Converted {} → {} ({} sizes)",
        png_path.display(),
        dest_path.display(),
        sizes.len()
    );
    Ok(())
}
