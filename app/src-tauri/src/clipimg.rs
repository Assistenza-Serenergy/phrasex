//! Clipboard pictures: Windows hands out a DIB (bitmap header + pixels), the history stores PNG.

/// CF_DIB bytes (24 or 32 bits per pixel, uncompressed or bit fields) to an RGBA PNG.
pub fn dib_to_png(dib: &[u8]) -> Option<Vec<u8>> {
    let u32_at = |o: usize| Some(u32::from_le_bytes(dib.get(o..o + 4)?.try_into().ok()?));
    let header = u32_at(0)? as usize;
    let width = u32_at(4)? as i32;
    let height = u32_at(8)? as i32;
    let bpp = u16::from_le_bytes(dib.get(14..16)?.try_into().ok()?) as usize;
    let compression = u32_at(16)?;
    let colors = u32_at(32)? as usize;
    if width <= 0 || height == 0 || !(bpp == 24 || bpp == 32) || !(compression == 0 || compression == 3) {
        return None;
    }
    let (w, h) = (width as usize, height.unsigned_abs() as usize);
    if w.checked_mul(h)? > 100_000_000 {
        return None; // malformed header: never trust its sizes
    }
    let masks = if compression == 3 && header == 40 { 12 } else { 0 };
    let start = header.checked_add(masks)?.checked_add(colors.checked_mul(4)?)?;
    let stride = (w * bpp).div_ceil(32) * 4;
    let pixels = dib.get(start..start.checked_add(stride.checked_mul(h)?)?)?;
    let mut rgba = Vec::with_capacity(w * h * 4);
    for y in 0..h {
        let row = if height > 0 { h - 1 - y } else { y }; // positive height = bottom-up rows
        for px in pixels[row * stride..].chunks(bpp / 8).take(w) {
            rgba.extend_from_slice(&[px[2], px[1], px[0], if bpp == 32 { px[3] } else { 255 }]);
        }
    }
    if bpp == 32 && rgba.chunks(4).all(|p| p[3] == 0) {
        rgba.chunks_mut(4).for_each(|p| p[3] = 255); // most apps leave the alpha byte empty
    }
    let mut out = Vec::new();
    let mut enc = png::Encoder::new(&mut out, w as u32, h as u32);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header().ok()?.write_image_data(&rgba).ok()?;
    Some(out)
}

/// A PNG written by `dib_to_png` back to CF_DIB bytes (32 bits, bottom-up), ready to paste.
pub fn png_to_dib(bytes: &[u8]) -> Option<Vec<u8>> {
    let mut reader = png::Decoder::new(std::io::Cursor::new(bytes)).read_info().ok()?;
    let mut buf = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buf).ok()?;
    if info.color_type != png::ColorType::Rgba || info.bit_depth != png::BitDepth::Eight {
        return None;
    }
    let (w, h) = (info.width as usize, info.height as usize);
    let mut dib = Vec::with_capacity(40 + w * h * 4);
    for v in [40u32, w as u32, h as u32] {
        dib.extend_from_slice(&v.to_le_bytes());
    }
    dib.extend_from_slice(&1u16.to_le_bytes()); // planes
    dib.extend_from_slice(&32u16.to_le_bytes()); // bits per pixel
    for v in [0u32, (w * h * 4) as u32, 0, 0, 0, 0] {
        dib.extend_from_slice(&v.to_le_bytes()); // BI_RGB, image size, resolution, palette
    }
    for y in (0..h).rev() {
        for p in buf[y * w * 4..(y + 1) * w * 4].chunks(4) {
            dib.extend_from_slice(&[p[2], p[1], p[0], p[3]]);
        }
    }
    Some(dib)
}

#[cfg(test)]
mod tests {
    #[test]
    fn dib_png_round_trip() {
        // 2x2, 24 bits, bottom-up: rows are padded to 4 bytes
        let mut dib = vec![0u8; 40];
        dib[0] = 40;
        dib[4] = 2;
        dib[8] = 2;
        dib[12] = 1;
        dib[14] = 24;
        dib.extend_from_slice(&[0, 0, 255, 0, 255, 0, 0, 0]); // bottom row: red, green
        dib.extend_from_slice(&[255, 0, 0, 255, 255, 255, 0, 0]); // top row: blue, white
        let png = super::dib_to_png(&dib).unwrap();
        let back = super::png_to_dib(&png).unwrap();
        assert_eq!(&back[4..12], &[2, 0, 0, 0, 2, 0, 0, 0]);
        // 32-bit bottom-up BGRA: bottom row first
        assert_eq!(&back[40..48], &[0, 0, 255, 255, 0, 255, 0, 255]);
        assert_eq!(&back[48..56], &[255, 0, 0, 255, 255, 255, 255, 255]);
        assert!(super::dib_to_png(&dib[..30]).is_none(), "short data is refused, not read past the end");
        dib[8..12].copy_from_slice(&i32::MIN.to_le_bytes());
        assert!(super::dib_to_png(&dib).is_none(), "absurd height is refused, no overflow");
    }
}
