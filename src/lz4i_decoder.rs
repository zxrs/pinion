use anyhow::{anyhow, ensure, Context, Result};
use image::{DynamicImage, ImageBuffer, Rgb, Rgba};
use std::fs;
use std::mem;

extern "C" {
    fn LZ4_decompress_safe(
        src: *const u8,
        dst: *mut u8,
        compressed_size: i32,
        dst_capacity: i32,
    ) -> i32;
}

#[repr(packed)]
struct Lz4iHeader {
    sig: [u8; 4],
    width: u32,
    height: u32,
    channels: u8,
    _colorspace: u8,
}

fn lz4_decomp(header: &Lz4iHeader, src: &[u8]) -> Result<Vec<u8>> {
    let width = header.width.to_be();
    let height = header.height.to_be();
    let dst_capacity = width
        .checked_mul(height)
        .context("u32 overflow")?
        .checked_mul(header.channels as u32)
        .context("u32 overflow")? as usize;
    let mut dst = vec![0; dst_capacity];
    unsafe {
        LZ4_decompress_safe(
            src.as_ptr(),
            dst.as_mut_ptr(),
            src.len() as i32,
            dst_capacity as i32,
        )
    };
    Ok(dst)
}

pub fn read_lz4i(file_path: &str) -> Result<DynamicImage> {
    let raw_lz4i = fs::read(file_path)?;
    let header = unsafe { &*(raw_lz4i.as_ptr() as *const Lz4iHeader) };
    ensure!(header.sig[..].eq(b"lz4i"), "Invalid LZ4I format.");

    let width = header.width.to_be();
    let height = header.height.to_be();

    let header_size = mem::size_of::<Lz4iHeader>();

    let decomped = lz4_decomp(header, &raw_lz4i[header_size..])?;

    let img = if header.channels == 3 {
        let buf =
            ImageBuffer::<Rgb<_>, _>::from_raw(width, height, decomped).context("buf overflow.")?;
        DynamicImage::ImageRgb8(buf)
    } else if header.channels == 4 {
        let buf = ImageBuffer::<Rgba<_>, _>::from_raw(width, height, decomped)
            .context("buf overflow.")?;
        DynamicImage::ImageRgba8(buf)
    } else {
        return Err(anyhow!("Unsupported LZ4I channels: {}.", header.channels));
    };

    Ok(img)
}
