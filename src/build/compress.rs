use std::io::Write;

use anyhow::{anyhow, Context, Result};

pub fn compress(data: &[u8], method: &str) -> Result<Vec<u8>> {
    match method {
        "none" | "" => Ok(data.to_vec()),
        #[cfg(feature = "lzma")]
        "lzma" => {
            let mut out = Vec::new();
            lzma_rs::lzma_compress(&mut std::io::Cursor::new(data), &mut out)
                .context("LZMA compression failed")?;
            Ok(out)
        }
        #[cfg(feature = "gzip")]
        "gzip" => {
            let mut encoder =
                flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
            encoder.write_all(data).context("gzip write failed")?;
            encoder.finish().context("gzip finish failed")
        }
        #[cfg(feature = "bzip2")]
        "bzip2" => {
            let mut encoder =
                bzip2::write::BzEncoder::new(Vec::new(), bzip2::Compression::best());
            encoder.write_all(data).context("bzip2 write failed")?;
            encoder.finish().context("bzip2 finish failed")
        }
        other => Err(anyhow!("unsupported compression method: {other}")),
    }
}

pub fn validate_method(method: &str) -> Result<()> {
    match method {
        #[cfg(feature = "lzma")]
        "lzma" => Ok(()),
        #[cfg(feature = "gzip")]
        "gzip" => Ok(()),
        #[cfg(feature = "bzip2")]
        "bzip2" => Ok(()),
        "none" => Ok(()),
        other => Err(anyhow!(
            "unsupported compression method: {other} (choose lzma, gzip, bzip2, or none)"
        )),
    }
}
