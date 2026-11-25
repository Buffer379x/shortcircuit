use std::fmt::Write;

use qrcode::{Color, EcLevel, QrCode};

const QUIET_MODULES: usize = 4;
const MIN_SIZE: u16 = 128;
const MAX_SIZE: u16 = 1024;
const DEFAULT_SIZE: u16 = 384;
// The longest configured host and slug fit within this limit, including the scheme and port.
const MAX_VALUE_BYTES: usize = 512;

#[derive(Clone, Copy)]
pub(crate) enum Format {
    Svg,
    Png,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum QrError {
    InvalidSize,
    DataTooLong,
    EncodingFailed,
}

pub(crate) fn generate(value: &str, format: Format, size: Option<u16>) -> Result<Vec<u8>, QrError> {
    let size = size.unwrap_or(DEFAULT_SIZE);
    if !(MIN_SIZE..=MAX_SIZE).contains(&size) {
        return Err(QrError::InvalidSize);
    }
    if value.len() > MAX_VALUE_BYTES {
        return Err(QrError::DataTooLong);
    }

    let code = QrCode::with_error_correction_level(value.as_bytes(), EcLevel::M)
        .map_err(|_| QrError::EncodingFailed)?;
    let size = usize::from(size);
    let width = code.width();
    let padded_width = width + 2 * QUIET_MODULES;
    let scale = size / padded_width;
    if scale == 0 {
        return Err(QrError::InvalidSize);
    }
    let margin = QUIET_MODULES * scale + (size - padded_width * scale) / 2;
    let dark_modules = code
        .into_colors()
        .into_iter()
        .enumerate()
        .filter(|&(_, color)| color == Color::Dark)
        .map(|(index, _)| {
            (
                margin + index % width * scale,
                margin + index / width * scale,
            )
        });

    match format {
        Format::Svg => Ok(svg(size, scale, dark_modules)),
        Format::Png => png(size, scale, dark_modules),
    }
}

fn svg(size: usize, scale: usize, dark_modules: impl Iterator<Item = (usize, usize)>) -> Vec<u8> {
    let mut output = format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="{size}" height="{size}" viewBox="0 0 {size} {size}" shape-rendering="crispEdges"><rect width="{size}" height="{size}" fill="#fff"/><path fill="#000" d=""##,
    );
    for (x, y) in dark_modules {
        write!(output, "M{x} {y}h{scale}v{scale}h-{scale}z")
            .expect("writing to a String cannot fail");
    }
    output.push_str("\"/></svg>");
    output.into_bytes()
}

fn png(
    size: usize,
    scale: usize,
    dark_modules: impl Iterator<Item = (usize, usize)>,
) -> Result<Vec<u8>, QrError> {
    let mut pixels = vec![255; size * size];
    for (x, y) in dark_modules {
        for row in y..y + scale {
            let start = row * size + x;
            pixels[start..start + scale].fill(0);
        }
    }

    let mut output = Vec::new();
    let mut encoder = png::Encoder::new(&mut output, size as u32, size as u32);
    encoder.set_color(png::ColorType::Grayscale);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder
        .write_header()
        .map_err(|_| QrError::EncodingFailed)?;
    writer
        .write_image_data(&pixels)
        .map_err(|_| QrError::EncodingFailed)?;
    writer.finish().map_err(|_| QrError::EncodingFailed)?;
    Ok(output)
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::{Format, QrError, generate};

    #[test]
    fn png_decodes_to_the_exact_short_url_at_the_requested_size() {
        const VALUE: &str = "http://[::1]:8123/Case/Snake_42";
        let png = generate(VALUE, Format::Png, Some(257)).unwrap();
        assert_eq!(png, generate(VALUE, Format::Png, Some(257)).unwrap());

        let mut reader = png::Decoder::new(Cursor::new(png)).read_info().unwrap();
        let mut pixels = vec![0; reader.output_buffer_size().unwrap()];
        let frame = reader.next_frame(&mut pixels).unwrap();
        assert_eq!((frame.width, frame.height), (257, 257));
        assert_eq!(frame.color_type, png::ColorType::Grayscale);
        assert_eq!(decode(257, &pixels), VALUE);
    }

    #[test]
    fn svg_keeps_a_four_module_white_border_and_does_not_insert_payload_text() {
        let payload = "https://q.co/<script>";
        let data = generate(payload, Format::Svg, Some(330)).unwrap();
        assert_eq!(data, generate(payload, Format::Svg, Some(330)).unwrap());
        let svg = String::from_utf8(data).unwrap();
        assert!(!svg.contains(payload));
        assert!(!svg.contains("<script"));
        assert!(svg.contains(r#"viewBox="0 0 330 330""#));
        assert!(svg.contains(r##"<rect width="330" height="330" fill="#fff"/>"##));
        assert!(svg.contains(r##"<path fill="#000""##));

        let path = svg
            .split_once(" d=\"")
            .unwrap()
            .1
            .split_once('"')
            .unwrap()
            .0;
        assert!(path.starts_with("M40 40h10v10h-10z"));
        let mut pixels = vec![255; 330 * 330];
        for square in path.trim_start_matches('M').split('M') {
            let (x, y) = square
                .strip_suffix("h10v10h-10z")
                .unwrap()
                .split_once(' ')
                .unwrap();
            let x: usize = x.parse().unwrap();
            let y: usize = y.parse().unwrap();
            assert!((40..=280).contains(&x) && (40..=280).contains(&y));
            assert_eq!((x % 10, y % 10), (0, 0));
            for row in y..y + 10 {
                let start = row * 330 + x;
                pixels[start..start + 10].fill(0);
            }
        }
        assert_eq!(decode(330, &pixels), payload);
    }

    #[test]
    fn generation_bounds_payload_and_image_size() {
        let value = "https://q.co/A";
        for invalid in [0, 127, 1025, u16::MAX] {
            assert_eq!(
                generate(value, Format::Png, Some(invalid)),
                Err(QrError::InvalidSize),
            );
        }
        for valid in [128, 1024] {
            let svg =
                String::from_utf8(generate(value, Format::Svg, Some(valid)).unwrap()).unwrap();
            let view_box = format!(r#"viewBox="0 0 {valid} {valid}""#);
            assert!(svg.contains(view_box.as_str()));
        }
        assert_eq!(
            generate(&"a".repeat(513), Format::Svg, None),
            Err(QrError::DataTooLong),
        );
    }

    fn decode(size: usize, pixels: &[u8]) -> String {
        let mut image =
            rqrr::PreparedImage::prepare_from_greyscale(size, size, |x, y| pixels[y * size + x]);
        let grids = image.detect_grids();
        assert_eq!(grids.len(), 1);
        grids[0].decode().unwrap().1
    }
}
