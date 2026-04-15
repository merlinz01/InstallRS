use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use image::imageops::FilterType;
use ico::{IconDir, IconDirEntry, IconImage, ResourceType};
use sha2::{Digest, Sha256};

/// Convert a PNG file to a multi-resolution `.ico` file, with caching.
///
/// The output is stored in `build_dir/icons/<hash>.ico` where the hash is
/// derived from the PNG contents and the requested sizes. If the cached file
/// already exists, conversion is skipped. Returns the path to the `.ico`.
pub fn png_to_ico(png_path: &Path, build_dir: &Path, sizes: &[u32]) -> Result<PathBuf> {
    let png_data = std::fs::read(png_path)
        .with_context(|| format!("failed to read PNG: {}", png_path.display()))?;

    // Hash PNG contents + sizes to form a cache key
    let mut hasher = Sha256::new();
    hasher.update(&png_data);
    for &s in sizes {
        hasher.update(s.to_le_bytes());
    }
    let hash = hex::encode(hasher.finalize());
    let short_hash = &hash[..16];

    let icons_dir = build_dir.join("icons");
    std::fs::create_dir_all(&icons_dir)
        .context("failed to create icons cache directory")?;

    let ico_path = icons_dir.join(format!("{short_hash}.ico"));

    if ico_path.exists() {
        log::info!("Using cached icon: {}", ico_path.display());
        return Ok(ico_path);
    }

    let img = image::load_from_memory(&png_data)
        .with_context(|| format!("failed to decode PNG: {}", png_path.display()))?;

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

    let out_file = std::fs::File::create(&ico_path)
        .with_context(|| format!("failed to create ICO: {}", ico_path.display()))?;
    icon_dir
        .write(out_file)
        .with_context(|| format!("failed to write ICO: {}", ico_path.display()))?;

    log::info!(
        "Converted {} → {} ({} sizes)",
        png_path.display(),
        ico_path.display(),
        sizes.len()
    );
    Ok(ico_path)
}
