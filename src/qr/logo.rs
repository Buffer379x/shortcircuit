use std::io::Cursor;

use super::{MAX_LOGO_BYTES, QrError, Rectangle, Rgb, Style};

const MAX_INPUT_SIDE: u32 = 512;
const MAX_SAMPLES_PER_SIDE: usize = 64;
const DECODER_BUDGET: usize = 4 * 1024 * 1024;

struct Image {
    width: usize,
    height: usize,
    channels: usize,
    pixels: Vec<u8>,
}

impl Image {
    fn decode(bytes: &[u8]) -> Result<Self, QrError> {
        if bytes.len() > MAX_LOGO_BYTES {
            return Err(QrError::InvalidLogo);
        }
        let mut decoder = png::Decoder::new_with_limits(
            Cursor::new(bytes),
            png::Limits {
                bytes: DECODER_BUDGET,
            },
        );
        decoder.set_ignore_text_chunk(true);
        decoder.set_ignore_iccp_chunk(true);
        decoder.set_transformations(png::Transformations::normalize_to_color8());
        let header = decoder
            .read_header_info()
            .map_err(|_| QrError::InvalidLogo)?;
        if !(1..=MAX_INPUT_SIDE).contains(&header.width)
            || !(1..=MAX_INPUT_SIDE).contains(&header.height)
        {
            return Err(QrError::InvalidLogoDimensions);
        }
        let mut reader = decoder.read_info().map_err(|_| QrError::InvalidLogo)?;
        if reader.info().animation_control.is_some() || reader.info().frame_control.is_some() {
            return Err(QrError::InvalidLogo);
        }
        let length = reader.output_buffer_size().ok_or(QrError::InvalidLogo)?;
        let mut pixels = vec![0; length];
        let frame = reader
            .next_frame(&mut pixels)
            .map_err(|_| QrError::InvalidLogo)?;
        reader.finish().map_err(|_| QrError::InvalidLogo)?;
        if reader.info().animation_control.is_some() || frame.color_type == png::ColorType::Indexed
        {
            return Err(QrError::InvalidLogo);
        }
        Ok(Self {
            width: frame.width as usize,
            height: frame.height as usize,
            channels: frame.color_type.samples(),
            pixels,
        })
    }

    fn on_background(&self, x: usize, y: usize, background: Rgb) -> Rgb {
        let start = (y * self.width + x) * self.channels;
        let pixel = &self.pixels[start..start + self.channels];
        let (rgb, alpha) = match *pixel {
            [gray] => ([gray; 3], 255),
            [gray, alpha] => ([gray; 3], alpha),
            [r, g, b] => ([r, g, b], 255),
            [r, g, b, alpha] => ([r, g, b], alpha),
            _ => unreachable!("the PNG decoder returns at most four channels"),
        };
        let alpha = u16::from(alpha);
        Rgb(std::array::from_fn(|channel| {
            let foreground = u16::from(rgb[channel]);
            let background = u16::from(background.0[channel]);
            ((foreground * alpha + background * (255 - alpha) + 127) / 255) as u8
        }))
    }
}

pub(super) fn overlay(
    bytes: &[u8],
    modules: usize,
    scale: usize,
    margin: usize,
    background: Rgb,
) -> Result<Vec<(Rectangle, Rgb)>, QrError> {
    let image = Image::decode(bytes)?;
    // Cover a small, odd number of modules centered on the symbol.
    let covered = (modules / 5).min(11) | 1;
    let tile = covered * scale;
    let start = margin + (modules - covered) / 2 * scale;
    let available = tile - 2 * scale;
    let source_side = image.width.max(image.height);
    let width = (image.width * available / source_side).max(1);
    let height = (image.height * available / source_side).max(1);
    let left = start + (tile - width) / 2;
    let top = start + (tile - height) / 2;
    let columns = width.min(image.width).min(MAX_SAMPLES_PER_SIDE);
    let rows = height.min(image.height).min(MAX_SAMPLES_PER_SIDE);

    let mut rectangles = Vec::with_capacity(1 + columns * rows);
    rectangles.push((Rectangle::new(start, start, tile, tile), background));
    for y in 0..rows {
        let y0 = top + y * height / rows;
        let y1 = top + (y + 1) * height / rows;
        let source_y = (2 * y + 1) * image.height / (2 * rows);
        for x in 0..columns {
            let x0 = left + x * width / columns;
            let x1 = left + (x + 1) * width / columns;
            let source_x = (2 * x + 1) * image.width / (2 * columns);
            let color = image.on_background(source_x, source_y, background);
            if color != background {
                rectangles.push((Rectangle::new(x0, y0, x1 - x0, y1 - y0), color));
            }
        }
    }
    Ok(rectangles)
}

pub(super) fn verified_pixels(
    value: &str,
    size: usize,
    style: &Style,
    symbol: &[Rectangle],
    overlay: &[(Rectangle, Rgb)],
) -> Result<Vec<u8>, QrError> {
    let [r, g, b] = style.background.0;
    let alpha = if style.transparent { 0 } else { 255 };
    let mut pixels = [r, g, b, alpha].repeat(size * size);
    for area in symbol {
        fill(&mut pixels, size, area, style.foreground);
    }
    for (area, color) in overlay {
        fill(&mut pixels, size, area, *color);
    }

    // Transparent pixels already carry the configured reference color in RGB.
    let mut image = rqrr::PreparedImage::prepare_from_greyscale(size, size, |x, y| {
        let offset = (y * size + x) * 4;
        let r = u32::from(pixels[offset]);
        let g = u32::from(pixels[offset + 1]);
        let b = u32::from(pixels[offset + 2]);
        ((299 * r + 587 * g + 114 * b) / 1000) as u8
    });
    let grids = image.detect_grids();
    let mut decoded = grids.iter().filter_map(|grid| grid.decode().ok());
    if !decoded.next().is_some_and(|(_, data)| data == value) || decoded.next().is_some() {
        return Err(QrError::LogoUnreadable);
    }
    Ok(pixels)
}

fn fill(pixels: &mut [u8], size: usize, area: &Rectangle, color: Rgb) {
    let [r, g, b] = color.0;
    let rgba = [r, g, b, 255];
    for row in area.y..area.y + area.height {
        let first = (row * size + area.x) * 4;
        let last = first + area.width * 4;
        for pixel in pixels[first..last].as_chunks_mut::<4>().0 {
            *pixel = rgba;
        }
    }
}

pub(super) fn encode_png(size: usize, pixels: &[u8]) -> Result<Vec<u8>, QrError> {
    let mut output = Vec::new();
    let mut encoder = png::Encoder::new(&mut output, size as u32, size as u32);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_color(png::ColorType::Rgba);
    let mut writer = encoder
        .write_header()
        .map_err(|_| QrError::EncodingFailed)?;
    writer
        .write_image_data(pixels)
        .map_err(|_| QrError::EncodingFailed)?;
    writer.finish().map_err(|_| QrError::EncodingFailed)?;
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::qr::{Format, Options, generate_with_logo};

    fn fixture(width: u32, height: u32, animated: bool) -> Vec<u8> {
        let mut image = Vec::new();
        let mut encoder = png::Encoder::new(&mut image, width, height);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_color(png::ColorType::Rgba);
        if animated {
            encoder.set_animated(1, 0).unwrap();
        }
        let mut writer = encoder.write_header().unwrap();
        writer
            .write_image_data(&[33, 99, 165, 128].repeat((width * height) as usize))
            .unwrap();
        writer.finish().unwrap();
        image
    }

    #[test]
    fn rejects_oversized_and_animated_pngs() {
        let options = Options::default();
        assert_eq!(
            generate_with_logo(
                "https://q.co/A",
                Format::Png,
                &options,
                &fixture(513, 2, false)
            ),
            Err(QrError::InvalidLogoDimensions)
        );
        assert_eq!(
            generate_with_logo(
                "https://q.co/A",
                Format::Png,
                &options,
                &fixture(16, 8, true)
            ),
            Err(QrError::InvalidLogo)
        );
    }

    #[test]
    fn both_formats_blend_the_center_logo_and_decode_the_short_address() {
        const VALUE: &str = "https://go.example/Team/Guide_12";
        let logo = fixture(16, 8, false);
        let options = Options {
            size: Some(512),
            transparent: Some(true),
            frame: Some("outline".to_owned()),
            ..Options::default()
        };
        let svg =
            String::from_utf8(generate_with_logo(VALUE, Format::Svg, &options, &logo).unwrap())
                .unwrap();
        assert!(svg.contains(r##"fill="#90b1d2""##));
        assert!(!svg.contains("<image"));
        let png = generate_with_logo(VALUE, Format::Png, &options, &logo).unwrap();
        let mut reader = png::Decoder::new(Cursor::new(png)).read_info().unwrap();
        let mut pixels = vec![0; reader.output_buffer_size().unwrap()];
        let frame = reader.next_frame(&mut pixels).unwrap();
        assert_eq!(
            (frame.width, frame.height, frame.color_type),
            (512, 512, png::ColorType::Rgba)
        );
        assert_eq!(pixels[3], 0);
        let center = (256 * 512 + 256) * 4;
        assert_eq!(&pixels[center..center + 4], &[144, 177, 210, 255]);
        let mut image = rqrr::PreparedImage::prepare_from_greyscale(512, 512, |x, y| {
            pixels[(y * 512 + x) * 4 + 1]
        });
        let grids = image.detect_grids();
        assert_eq!(grids[0].decode().unwrap().1, VALUE);
    }
}
