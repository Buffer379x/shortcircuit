use std::fmt::{self, Write};

use qrcode::{Color, EcLevel, QrCode};

const MIN_QUIET_ZONE: i64 = 4;
const MAX_QUIET_ZONE: i64 = 12;
const MIN_SIZE: u16 = 128;
const MAX_SIZE: u16 = 1024;
const DEFAULT_SIZE: u16 = 384;
const MIN_CONTRAST: f64 = 4.5;
// The longest configured host and slug fit within this limit, including the scheme and port.
const MAX_VALUE_BYTES: usize = 512;

#[derive(Clone, Copy)]
pub(crate) enum Format {
    Svg,
    Png,
}

#[derive(Default)]
pub(crate) struct Options {
    pub(crate) size: Option<u16>,
    pub(crate) foreground: Option<String>,
    pub(crate) background: Option<String>,
    pub(crate) transparent: Option<bool>,
    pub(crate) correction: Option<String>,
    pub(crate) quiet_zone: Option<i64>,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum QrError {
    InvalidSize,
    InvalidColor,
    InvalidContrast,
    InvalidCorrection,
    InvalidQuietZone,
    DataTooLong,
    EncodingFailed,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct Rgb([u8; 3]);

impl Rgb {
    const BLACK: Self = Self([0; 3]);
    const WHITE: Self = Self([255; 3]);

    fn parse(value: &str) -> Result<Self, QrError> {
        if value.len() != 6 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(QrError::InvalidColor);
        }
        let [_, red, green, blue] = u32::from_str_radix(value, 16)
            .map_err(|_| QrError::InvalidColor)?
            .to_be_bytes();
        Ok(Self([red, green, blue]))
    }

    fn luminance(self) -> f64 {
        let [red, green, blue] = self.0.map(|channel| {
            let channel = f64::from(channel) / 255.0;
            if channel <= 0.04045 {
                channel / 12.92
            } else {
                ((channel + 0.055) / 1.055).powf(2.4)
            }
        });
        0.2126 * red + 0.7152 * green + 0.0722 * blue
    }
}

impl fmt::Display for Rgb {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::BLACK => formatter.write_str("#000"),
            Self::WHITE => formatter.write_str("#fff"),
            Self([red, green, blue]) => write!(formatter, "#{red:02x}{green:02x}{blue:02x}"),
        }
    }
}

struct Style {
    foreground: Rgb,
    background: Rgb,
    transparent: bool,
    correction: EcLevel,
    quiet_zone: usize,
}

impl Style {
    fn parse(options: &Options) -> Result<Self, QrError> {
        let foreground = options
            .foreground
            .as_deref()
            .map(Rgb::parse)
            .transpose()?
            .unwrap_or(Rgb::BLACK);
        let background = options
            .background
            .as_deref()
            .map(Rgb::parse)
            .transpose()?
            .unwrap_or(Rgb::WHITE);
        if (background.luminance() + 0.05) / (foreground.luminance() + 0.05) < MIN_CONTRAST {
            return Err(QrError::InvalidContrast);
        }
        let correction = match options.correction.as_deref().unwrap_or("M") {
            "L" => EcLevel::L,
            "M" => EcLevel::M,
            "Q" => EcLevel::Q,
            "H" => EcLevel::H,
            _ => return Err(QrError::InvalidCorrection),
        };
        let quiet_zone = options.quiet_zone.unwrap_or(MIN_QUIET_ZONE);
        if !(MIN_QUIET_ZONE..=MAX_QUIET_ZONE).contains(&quiet_zone) {
            return Err(QrError::InvalidQuietZone);
        }
        Ok(Self {
            foreground,
            background,
            transparent: options.transparent.unwrap_or(false),
            correction,
            quiet_zone: quiet_zone as usize,
        })
    }
}

pub(crate) fn generate(value: &str, format: Format, options: &Options) -> Result<Vec<u8>, QrError> {
    let size = options.size.unwrap_or(DEFAULT_SIZE);
    if !(MIN_SIZE..=MAX_SIZE).contains(&size) {
        return Err(QrError::InvalidSize);
    }
    if value.len() > MAX_VALUE_BYTES {
        return Err(QrError::DataTooLong);
    }

    let style = Style::parse(options)?;
    let code = QrCode::with_error_correction_level(value.as_bytes(), style.correction)
        .map_err(|_| QrError::EncodingFailed)?;
    let size = usize::from(size);
    let width = code.width();
    let padded_width = width + 2 * style.quiet_zone;
    let scale = size / padded_width;
    if scale == 0 {
        return Err(QrError::InvalidSize);
    }
    let margin = style.quiet_zone * scale + (size - padded_width * scale) / 2;
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
        Format::Svg => Ok(svg(size, scale, &style, dark_modules)),
        Format::Png => png(size, scale, &style, dark_modules),
    }
}

fn svg(
    size: usize,
    scale: usize,
    style: &Style,
    dark_modules: impl Iterator<Item = (usize, usize)>,
) -> Vec<u8> {
    let mut output = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{size}" height="{size}" viewBox="0 0 {size} {size}" shape-rendering="crispEdges">"#,
    );
    if !style.transparent {
        write!(
            output,
            r#"<rect width="{size}" height="{size}" fill="{}"/>"#,
            style.background
        )
        .expect("writing to a String cannot fail");
    }
    write!(output, r#"<path fill="{}" d=""#, style.foreground)
        .expect("writing to a String cannot fail");
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
    style: &Style,
    dark_modules: impl Iterator<Item = (usize, usize)>,
) -> Result<Vec<u8>, QrError> {
    let mut output = Vec::new();
    let mut encoder = png::Encoder::new(&mut output, size as u32, size as u32);
    encoder.set_depth(png::BitDepth::Eight);
    let light_pixel =
        if style.foreground == Rgb::BLACK && style.background == Rgb::WHITE && !style.transparent {
            encoder.set_color(png::ColorType::Grayscale);
            255
        } else {
            encoder.set_color(png::ColorType::Indexed);
            let [red, green, blue] = style.foreground.0;
            let [light_red, light_green, light_blue] = style.background.0;
            encoder.set_palette(vec![red, green, blue, light_red, light_green, light_blue]);
            if style.transparent {
                encoder.set_trns(vec![255, 0]);
            }
            1
        };
    let mut pixels = vec![light_pixel; size * size];
    for (x, y) in dark_modules {
        for row in y..y + scale {
            let start = row * size + x;
            pixels[start..start + scale].fill(0);
        }
    }

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

    use super::{Format, Options, QrError, generate};

    #[test]
    fn png_decodes_to_the_exact_short_url_at_the_requested_size() {
        const VALUE: &str = "http://[::1]:8123/Case/Snake_42";
        let options = Options {
            size: Some(257),
            ..Options::default()
        };
        let png = generate(VALUE, Format::Png, &options).unwrap();
        assert_eq!(png, generate(VALUE, Format::Png, &options).unwrap());

        let mut reader = png::Decoder::new(Cursor::new(png)).read_info().unwrap();
        let mut pixels = vec![0; reader.output_buffer_size().unwrap()];
        let frame = reader.next_frame(&mut pixels).unwrap();
        assert_eq!((frame.width, frame.height), (257, 257));
        assert_eq!(frame.color_type, png::ColorType::Grayscale);
        assert_eq!(decode(257, &pixels).1, VALUE);
    }

    #[test]
    fn svg_keeps_a_four_module_white_border_and_does_not_insert_payload_text() {
        let payload = "https://q.co/<script>";
        let options = Options {
            size: Some(330),
            ..Options::default()
        };
        let data = generate(payload, Format::Svg, &options).unwrap();
        assert_eq!(data, generate(payload, Format::Svg, &options).unwrap());
        let svg = String::from_utf8(data).unwrap();
        assert!(!svg.contains(payload));
        assert!(!svg.contains("<script"));
        assert!(svg.contains(r#"viewBox="0 0 330 330""#));
        assert!(svg.contains(r##"<rect width="330" height="330" fill="#fff"/>"##));
        assert!(svg.contains(r##"<path fill="#000""##));

        assert_eq!(decode(330, &svg_pixels(&svg, 330, 10, 40)).1, payload);
    }

    #[test]
    fn styled_svg_has_the_selected_margin_and_background() {
        const VALUE: &str = "https://q.co/A";
        let mut options = Options {
            size: Some(350),
            foreground: Some("10203A".to_owned()),
            background: Some("F6F4EE".to_owned()),
            quiet_zone: Some(7),
            ..Options::default()
        };
        let opaque = String::from_utf8(generate(VALUE, Format::Svg, &options).unwrap()).unwrap();
        assert!(opaque.contains(r##"<rect width="350" height="350" fill="#f6f4ee"/>"##));
        assert!(opaque.contains(r##"<path fill="#10203a""##));
        assert_eq!(decode(350, &svg_pixels(&opaque, 350, 10, 70)).1, VALUE);

        options.transparent = Some(true);
        let transparent =
            String::from_utf8(generate(VALUE, Format::Svg, &options).unwrap()).unwrap();
        assert!(!transparent.contains("<rect"));
        assert!(transparent.contains(r##"<path fill="#10203a""##));
        assert_eq!(
            decode(350, &svg_pixels(&transparent, 350, 10, 70)).1,
            VALUE
        );
    }

    #[test]
    fn styled_png_keeps_its_palette_and_alpha() {
        const VALUE: &str = "https://go.example/Team/Guide_12";
        for transparent in [false, true] {
            let options = Options {
                foreground: Some("10203A".to_owned()),
                background: Some("F6F4EE".to_owned()),
                transparent: Some(transparent),
                ..Options::default()
            };
            let data = generate(VALUE, Format::Png, &options).unwrap();
            let mut reader = png::Decoder::new(Cursor::new(data)).read_info().unwrap();
            assert_eq!(
                reader.info().palette.as_deref(),
                Some(&[0x10, 0x20, 0x3a, 0xf6, 0xf4, 0xee][..])
            );
            assert_eq!(
                reader.info().trns.as_deref(),
                transparent.then_some(&[255, 0][..])
            );
            let mut pixels = vec![0; reader.output_buffer_size().unwrap()];
            let frame = reader.next_frame(&mut pixels).unwrap();
            assert_eq!(frame.color_type, png::ColorType::Indexed);
            assert_eq!((frame.width, frame.height), (384, 384));
            assert_eq!(pixels[0], 1);
            for pixel in &mut pixels {
                // Composite the two palette entries on the selected reference background.
                *pixel = match *pixel {
                    0 => 30,
                    1 => 245,
                    value => panic!("unexpected palette entry {value}"),
                };
            }
            assert_eq!(decode(384, &pixels).1, VALUE);
        }
    }

    #[test]
    fn png_encodes_the_requested_error_correction() {
        const VALUE: &str = "https://go.example/Team/Guide_12";
        // The two format bits are 01, 00, 11 and 10 for levels L, M, Q and H.
        for (correction, expected) in [("L", 1), ("M", 0), ("Q", 3), ("H", 2)] {
            let options = Options {
                correction: Some(correction.to_owned()),
                ..Options::default()
            };
            let data = generate(VALUE, Format::Png, &options).unwrap();
            let mut reader = png::Decoder::new(Cursor::new(data)).read_info().unwrap();
            let mut pixels = vec![0; reader.output_buffer_size().unwrap()];
            let frame = reader.next_frame(&mut pixels).unwrap();
            let (metadata, decoded) = decode(frame.width as usize, &pixels);
            assert_eq!(metadata.ecc_level, expected);
            assert_eq!(decoded, VALUE);
        }
    }

    #[test]
    fn rejects_invalid_style_options() {
        let value = "https://q.co/A";
        for color in ["fff", "#ffffff", "+fffff", "gg0000", "äffff"] {
            let options = Options {
                foreground: Some(color.to_owned()),
                ..Options::default()
            };
            assert_eq!(
                generate(value, Format::Svg, &options),
                Err(QrError::InvalidColor)
            );
        }
        for zone in [-1, 3, 13] {
            let options = Options {
                quiet_zone: Some(zone),
                ..Options::default()
            };
            assert_eq!(
                generate(value, Format::Svg, &options),
                Err(QrError::InvalidQuietZone)
            );
        }
        let options = Options {
            correction: Some("X".to_owned()),
            ..Options::default()
        };
        assert_eq!(
            generate(value, Format::Svg, &options),
            Err(QrError::InvalidCorrection)
        );
    }

    #[test]
    fn enforces_direction_and_threshold_of_contrast_even_when_transparent() {
        let value = "https://q.co/A";
        let mut options = Options {
            background: Some("747474".to_owned()),
            transparent: Some(true),
            ..Options::default()
        };
        assert_eq!(
            generate(value, Format::Png, &options),
            Err(QrError::InvalidContrast)
        );
        options.background = Some("757575".to_owned());
        assert!(generate(value, Format::Png, &options).is_ok());

        options.foreground = Some("FFFFFF".to_owned());
        options.background = Some("000000".to_owned());
        assert_eq!(
            generate(value, Format::Svg, &options),
            Err(QrError::InvalidContrast)
        );
    }

    fn svg_pixels(svg: &str, size: usize, scale: usize, margin: usize) -> Vec<u8> {
        let path = svg
            .split_once(" d=\"")
            .unwrap()
            .1
            .split_once('"')
            .unwrap()
            .0;
        let prefix = format!("M{margin} {margin}h{scale}v{scale}h-{scale}z");
        assert!(path.starts_with(prefix.as_str()));
        let mut pixels = vec![255; size * size];
        let suffix = format!("h{scale}v{scale}h-{scale}z");
        let bounds = margin..=size - margin - scale;
        for square in path.trim_start_matches('M').split('M') {
            let (x, y) = square
                .strip_suffix(suffix.as_str())
                .unwrap()
                .split_once(' ')
                .unwrap();
            let x: usize = x.parse().unwrap();
            let y: usize = y.parse().unwrap();
            assert!(bounds.contains(&x) && bounds.contains(&y));
            assert_eq!(((x - margin) % scale, (y - margin) % scale), (0, 0));
            for row in y..y + scale {
                let start = row * size + x;
                pixels[start..start + scale].fill(0);
            }
        }
        pixels
    }

    #[test]
    fn generation_bounds_payload_and_image_size() {
        let value = "https://q.co/A";
        for invalid in [0, 127, 1025, u16::MAX] {
            let options = Options {
                size: Some(invalid),
                ..Options::default()
            };
            assert_eq!(
                generate(value, Format::Png, &options),
                Err(QrError::InvalidSize),
            );
        }
        for valid in [128, 1024] {
            let options = Options {
                size: Some(valid),
                ..Options::default()
            };
            let svg = String::from_utf8(generate(value, Format::Svg, &options).unwrap()).unwrap();
            let view_box = format!(r#"viewBox="0 0 {valid} {valid}""#);
            assert!(svg.contains(view_box.as_str()));
        }
        assert_eq!(
            generate(&"a".repeat(513), Format::Svg, &Options::default()),
            Err(QrError::DataTooLong),
        );
    }

    fn decode(size: usize, pixels: &[u8]) -> (rqrr::MetaData, String) {
        let mut image =
            rqrr::PreparedImage::prepare_from_greyscale(size, size, |x, y| pixels[y * size + x]);
        let grids = image.detect_grids();
        assert_eq!(grids.len(), 1);
        grids[0].decode().unwrap()
    }
}
