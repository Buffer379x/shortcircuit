use std::fmt::{self, Write};

use qrcode::{Color, EcLevel, QrCode};

mod logo;

pub(crate) const MAX_LOGO_BYTES: usize = 256 * 1024;
const MIN_QUIET_ZONE: i64 = 4;
const MAX_QUIET_ZONE: i64 = 12;
const MIN_SIZE: u16 = 128;
const MAX_SIZE: u16 = 1024;
const DEFAULT_SIZE: u16 = 384;
const MIN_CONTRAST: f64 = 4.5;
// Outside the quiet zone: one clear module, the outline, and one outer clear module.
const OUTLINE_PADDING: usize = 3;
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
    pub(crate) frame: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum QrError {
    InvalidSize,
    InvalidColor,
    InvalidContrast,
    InvalidCorrection,
    InvalidQuietZone,
    InvalidFrame,
    InvalidLogo,
    InvalidLogoDimensions,
    InvalidLogoCorrection,
    LogoUnreadable,
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
    outline: bool,
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
        let outline = match options.frame.as_deref() {
            Some("outline") => true,
            None | Some("none") => false,
            _ => return Err(QrError::InvalidFrame),
        };
        Ok(Self {
            foreground,
            background,
            transparent: options.transparent.unwrap_or(false),
            correction,
            quiet_zone: quiet_zone as usize,
            outline,
        })
    }
}

impl Options {
    fn render_settings(&self) -> Result<(u16, Style), QrError> {
        let size = self.size.unwrap_or(DEFAULT_SIZE);
        if !(MIN_SIZE..=MAX_SIZE).contains(&size) {
            return Err(QrError::InvalidSize);
        }
        Ok((size, Style::parse(self)?))
    }

    pub(crate) fn validate(&self) -> Result<(), QrError> {
        self.render_settings().map(|_| ())
    }
}

pub(crate) fn generate(value: &str, format: Format, options: &Options) -> Result<Vec<u8>, QrError> {
    render(value, format, options, None)
}

pub(crate) fn generate_with_logo(
    value: &str,
    format: Format,
    options: &Options,
    bytes: &[u8],
) -> Result<Vec<u8>, QrError> {
    render(value, format, options, Some(bytes))
}

fn render(
    value: &str,
    format: Format,
    options: &Options,
    logo: Option<&[u8]>,
) -> Result<Vec<u8>, QrError> {
    let (size, mut style) = options.render_settings()?;
    if value.len() > MAX_VALUE_BYTES {
        return Err(QrError::DataTooLong);
    }
    if logo.is_some() {
        if options.correction.is_some() && style.correction != EcLevel::H {
            return Err(QrError::InvalidLogoCorrection);
        }
        style.correction = EcLevel::H;
    }

    let code = QrCode::with_error_correction_level(value.as_bytes(), style.correction)
        .map_err(|_| QrError::EncodingFailed)?;
    let size = usize::from(size);
    let width = code.width();
    let padding = style.quiet_zone + if style.outline { OUTLINE_PADDING } else { 0 };
    let padded_width = width + 2 * padding;
    let scale = size / padded_width;
    if scale == 0 {
        return Err(QrError::InvalidSize);
    }
    let start = (size - padded_width * scale) / 2;
    let margin = start + padding * scale;
    let dark_modules = code
        .into_colors()
        .into_iter()
        .enumerate()
        .filter(|&(_, color)| color == Color::Dark)
        .map(|(index, _)| {
            Rectangle::new(
                margin + index % width * scale,
                margin + index / width * scale,
                scale,
                scale,
            )
        });
    let frame = style
        .outline
        .then(|| outline(start + scale, (padded_width - 2) * scale, scale));
    let rectangles = dark_modules.chain(frame.into_iter().flatten());

    if let Some(bytes) = logo {
        let overlay = logo::overlay(bytes, width, scale, margin, style.background)?;
        let rectangles: Vec<_> = rectangles.collect();
        let pixels = logo::verified_pixels(value, size, &style, &rectangles, &overlay)?;
        return match format {
            Format::Svg => Ok(svg(size, &style, rectangles.into_iter(), &overlay)),
            Format::Png => logo::encode_png(size, &pixels),
        };
    }
    match format {
        Format::Svg => Ok(svg(size, &style, rectangles, &[])),
        Format::Png => png(size, &style, rectangles),
    }
}

struct Rectangle {
    x: usize,
    y: usize,
    width: usize,
    height: usize,
}

impl Rectangle {
    fn new(x: usize, y: usize, width: usize, height: usize) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }
}

fn outline(start: usize, side: usize, stroke: usize) -> [Rectangle; 4] {
    let end = start + side - stroke;
    let interior = side - 2 * stroke;
    [
        Rectangle::new(start, start, side, stroke),
        Rectangle::new(start, end, side, stroke),
        Rectangle::new(start, start + stroke, stroke, interior),
        Rectangle::new(end, start + stroke, stroke, interior),
    ]
}

fn svg(
    size: usize,
    style: &Style,
    rectangles: impl Iterator<Item = Rectangle>,
    overlay: &[(Rectangle, Rgb)],
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
    for Rectangle {
        x,
        y,
        width,
        height,
    } in rectangles
    {
        write!(output, "M{x} {y}h{width}v{height}h-{width}z")
            .expect("writing to a String cannot fail");
    }
    output.push_str("\"/>");
    for (
        Rectangle {
            x,
            y,
            width,
            height,
        },
        color,
    ) in overlay
    {
        write!(
            output,
            r#"<rect x="{x}" y="{y}" width="{width}" height="{height}" fill="{color}"/>"#
        )
        .expect("writing to a String cannot fail");
    }
    output.push_str("</svg>");
    output.into_bytes()
}

fn png(
    size: usize,
    style: &Style,
    rectangles: impl Iterator<Item = Rectangle>,
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
    for Rectangle {
        x,
        y,
        width,
        height,
    } in rectangles
    {
        for row in y..y + height {
            let start = row * size + x;
            pixels[start..start + width].fill(0);
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
        assert_eq!(decode(350, &svg_pixels(&transparent, 350, 10, 70)).1, VALUE);
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
    fn outline_keeps_the_clear_margin_and_the_exact_payload_in_both_formats() {
        const VALUE: &str = "https://q.co/A";
        const SIZE: usize = 350;
        let options = Options {
            size: Some(350),
            frame: Some("outline".to_owned()),
            ..Options::default()
        };
        let svg = String::from_utf8(generate(VALUE, Format::Svg, &options).unwrap()).unwrap();
        let svg_bitmap = paint_svg_rectangles(SIZE, &svg_rectangles(&svg));
        let png = generate(VALUE, Format::Png, &options).unwrap();
        let mut reader = png::Decoder::new(Cursor::new(png)).read_info().unwrap();
        let mut pixels = vec![0; reader.output_buffer_size().unwrap()];
        let image = reader.next_frame(&mut pixels).unwrap();
        assert_eq!((image.width, image.height), (350, 350));
        assert_eq!(image.color_type, png::ColorType::Grayscale);
        assert_eq!(pixels, svg_bitmap);
        // With 21 QR modules, a four-module quiet zone and three reserved modules
        // per edge, each module occupies exactly 10 pixels.
        let row = &pixels[70 * SIZE..71 * SIZE];
        assert_eq!(&row[..10], &[255; 10]);
        assert_eq!(&row[10..20], &[0; 10]);
        assert_eq!(&row[20..70], &[255; 50]);
        assert_eq!(&row[70..80], &[0; 10]);
        assert_eq!(&row[330..340], &[0; 10]);
        assert_eq!(&row[340..], &[255; 10]);
        assert_eq!(decode(SIZE, &pixels).1, VALUE);
        assert_eq!(decode(SIZE, &svg_bitmap).1, VALUE);
    }

    #[test]
    fn transparent_outline_is_scannable_at_small_and_large_resolutions() {
        let long = format!("https://go.example/QR/{}", "z".repeat(196));
        for (size, value) in [(128, "https://q.co/A"), (768, long.as_str())] {
            let options = Options {
                size: Some(size),
                transparent: Some(true),
                correction: Some("H".to_owned()),
                frame: Some("outline".to_owned()),
                ..Options::default()
            };
            let png = generate(value, Format::Png, &options).unwrap();
            let mut reader = png::Decoder::new(Cursor::new(png)).read_info().unwrap();
            assert_eq!(reader.info().trns.as_deref(), Some(&[255, 0][..]));
            let mut pixels = vec![0; reader.output_buffer_size().unwrap()];
            reader.next_frame(&mut pixels).unwrap();
            assert_eq!(pixels[0], 1);
            for pixel in &mut pixels {
                // Composite the frame and transparent cells on the reference background.
                *pixel *= 255;
            }
            assert_eq!(decode(size.into(), &pixels).1, value);
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
        let rectangles = svg_rectangles(svg);
        assert_eq!(rectangles[0], (margin, margin, scale, scale));
        let bounds = margin..=size - margin - scale;
        for &(x, y, width, height) in &rectangles {
            assert!(bounds.contains(&x) && bounds.contains(&y));
            assert_eq!(((x - margin) % scale, (y - margin) % scale), (0, 0));
            assert_eq!((width, height), (scale, scale));
        }
        paint_svg_rectangles(size, &rectangles)
    }

    fn svg_rectangles(svg: &str) -> Vec<(usize, usize, usize, usize)> {
        let path = svg
            .split_once(" d=\"")
            .unwrap()
            .1
            .split_once('"')
            .unwrap()
            .0;
        path.trim_start_matches('M')
            .split('M')
            .map(|rectangle| {
                let (origin, sides) = rectangle.split_once('h').unwrap();
                let (x, y) = origin.split_once(' ').unwrap();
                let (width, remainder) = sides.split_once('v').unwrap();
                let (height, closing) = remainder.split_once("h-").unwrap();
                assert_eq!(closing.strip_suffix('z').unwrap(), width);
                (
                    x.parse().unwrap(),
                    y.parse().unwrap(),
                    width.parse().unwrap(),
                    height.parse().unwrap(),
                )
            })
            .collect()
    }

    fn paint_svg_rectangles(size: usize, rectangles: &[(usize, usize, usize, usize)]) -> Vec<u8> {
        let mut pixels = vec![255; size * size];
        for &(x, y, width, height) in rectangles {
            assert!(x + width <= size && y + height <= size);
            for row in y..y + height {
                let start = row * size + x;
                pixels[start..start + width].fill(0);
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
