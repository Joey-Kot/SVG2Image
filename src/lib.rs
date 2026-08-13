use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result, ensure};
use clap::{Parser, ValueEnum};
use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::{CompressionType, FilterType, PngEncoder};
use image::{ExtendedColorType, ImageEncoder};
use resvg::tiny_skia::{IntSize, Pixmap, Transform};
use resvg::usvg::fontdb::{Database, Family as DatabaseFamily, Query, Weight};
use resvg::usvg::{Font, FontFamily, FontResolver, Options, Tree};
use webp::Encoder as WebpEncoder;

const DEFAULT_QUALITY: f32 = 0.85;
const MAX_RENDER_DIMENSION: u32 = u16::MAX as u32;
const MAX_RENDER_PIXELS: u64 = 64_000_000;
const MAX_WEBP_DIMENSION: u32 = 16_383;

/// Convert an SVG file into an image.
#[derive(Debug, Parser)]
#[command(
    name = "svg2image",
    version,
    about = "将 SVG 转换为 WebP、PNG 或 JPG 图像",
    long_about = None,
    after_help = "用法示例：\n  svg2image --file image.svg --size '1000:-1'\n  svg2image --file image.svg --size '-1@1000' --format png\n  svg2image --file image.svg --size '1600*-1' --format jpg --quality 0.9"
)]
struct Cli {
    /// 要转换的 SVG 文件
    #[arg(long, value_name = "SVG")]
    file: PathBuf,

    /// 输出图像尺寸；将宽或高设为 -1 可按原始比例自动计算
    #[arg(
        long,
        value_name = "宽:高",
        allow_hyphen_values = true,
        value_parser = parse_size
    )]
    size: SizeSpec,

    /// 输出格式
    #[arg(
        long,
        value_name = "格式",
        value_enum,
        ignore_case = true,
        default_value = "webp"
    )]
    format: OutputFormat,

    /// 图像质量，范围为 0.0 到 1.0；PNG 为无损格式，该参数不改变画质
    #[arg(long, value_name = "质量", default_value_t = DEFAULT_QUALITY, value_parser = parse_quality)]
    quality: f32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
enum OutputFormat {
    Webp,
    Png,
    #[value(alias = "jpeg")]
    Jpg,
}

impl OutputFormat {
    const fn extension(self) -> &'static str {
        match self {
            Self::Webp => "webp",
            Self::Png => "png",
            Self::Jpg => "jpg",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct OutputSize {
    width: u32,
    height: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SizeSpec {
    Exact(OutputSize),
    FixedWidth(u32),
    FixedHeight(u32),
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct RenderPlan {
    size: OutputSize,
    scale_x: f32,
    scale_y: f32,
}

#[derive(Clone, Default)]
struct MissingFontWarnings {
    families: Arc<Mutex<BTreeSet<String>>>,
}

impl MissingFontWarnings {
    fn install(&self, options: &mut Options<'_>) {
        let warnings = self.clone();
        let default_selector = FontResolver::default_font_selector();
        options.font_resolver.select_font = Box::new(move |font, database| {
            if !has_declared_font_match(font, database) {
                warnings.record(font);
            }
            default_selector(font, database)
        });
    }

    fn record(&self, font: &Font) {
        let unresolved_families = font
            .families()
            .iter()
            .map(font_family_name)
            .collect::<Vec<_>>()
            .join(", ");
        let mut families = self
            .families
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        families.insert(unresolved_families);
    }

    fn message(&self) -> Option<String> {
        let families = self
            .families
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if families.is_empty() {
            return None;
        }

        let families = families
            .iter()
            .map(|family| format!("“{family}”"))
            .collect::<Vec<_>>()
            .join("、");
        Some(format!(
            "警告：本地未找到以下 SVG 字体族组合：{families}；将继续按现有字体回退规则转换，相关文本可能显示异常或缺失。"
        ))
    }

    fn write_to(&self, output: &mut dyn Write) {
        if let Some(message) = self.message() {
            let _ = writeln!(output, "{message}");
        }
    }
}

fn has_declared_font_match(font: &Font, database: &Database) -> bool {
    let families = font
        .families()
        .iter()
        .map(database_family)
        .collect::<Vec<_>>();
    database
        .query(&Query {
            families: &families,
            weight: Weight(font.weight()),
            stretch: font.stretch().into(),
            style: font.style().into(),
        })
        .is_some()
}

fn database_family(family: &FontFamily) -> DatabaseFamily<'_> {
    match family {
        FontFamily::Serif => DatabaseFamily::Serif,
        FontFamily::SansSerif => DatabaseFamily::SansSerif,
        FontFamily::Cursive => DatabaseFamily::Cursive,
        FontFamily::Fantasy => DatabaseFamily::Fantasy,
        FontFamily::Monospace => DatabaseFamily::Monospace,
        FontFamily::Named(name) => DatabaseFamily::Name(name),
    }
}

fn font_family_name(family: &FontFamily) -> String {
    match family {
        FontFamily::Serif => "serif".to_owned(),
        FontFamily::SansSerif => "sans-serif".to_owned(),
        FontFamily::Cursive => "cursive".to_owned(),
        FontFamily::Fantasy => "fantasy".to_owned(),
        FontFamily::Monospace => "monospace".to_owned(),
        FontFamily::Named(name) => name.clone(),
    }
}

/// Parse command-line arguments and run the conversion.
pub fn run() -> Result<()> {
    convert(Cli::parse())
}

fn convert(cli: Cli) -> Result<()> {
    let stderr = std::io::stderr();
    convert_with_warning_output(cli, &mut stderr.lock())
}

fn convert_with_warning_output(cli: Cli, warning_output: &mut dyn Write) -> Result<()> {
    convert_with_options(cli, warning_output, |options| {
        options.fontdb_mut().load_system_fonts();
    })
}

fn convert_with_options(
    cli: Cli,
    warning_output: &mut dyn Write,
    load_fonts: impl FnOnce(&mut Options<'_>),
) -> Result<()> {
    validate_svg_path(&cli.file)?;

    let mut options = Options {
        resources_dir: cli.file.parent().map(Path::to_path_buf),
        ..Options::default()
    };
    // usvg resolves font references with the system font database while parsing.
    // Text is represented as paths in the render tree before rasterization.
    load_fonts(&mut options);
    let missing_font_warnings = MissingFontWarnings::default();
    missing_font_warnings.install(&mut options);

    let svg_bytes =
        fs::read(&cli.file).with_context(|| format!("无法读取 SVG 文件 {}", cli.file.display()))?;
    let tree = Tree::from_data(&svg_bytes, &options)
        .map_err(|error| anyhow::anyhow!("无法解析 SVG 文件 {}：{error}", cli.file.display()))?;
    ensure!(
        tree.size().width() > 0.0 && tree.size().height() > 0.0,
        "SVG 画布尺寸无效"
    );

    let plan = resolve_render_plan(cli.size, tree.size().width(), tree.size().height())?;
    validate_format_dimensions(cli.format, plan.size)?;

    let output_path = output_path(&cli.file, cli.format)?;
    missing_font_warnings.write_to(warning_output);

    println!(
        "开始转换：格式 {}，尺寸 {}x{}，输出文件 {}",
        cli.format.extension(),
        plan.size.width,
        plan.size.height,
        output_path.display()
    );

    let mut pixmap = create_pixmap(plan.size)?;
    resvg::render(
        &tree,
        Transform::from_scale(plan.scale_x, plan.scale_y),
        &mut pixmap.as_mut(),
    );

    let rgba = pixmap.take_demultiplied();
    let encoded = encode_rgba(&rgba, plan.size, cli.format, cli.quality).context("图像编码失败")?;
    write_output_file(&output_path, &encoded)?;

    println!("转换完成：{}", output_path.display());
    Ok(())
}

fn validate_svg_path(path: &Path) -> Result<()> {
    ensure!(
        path.is_file(),
        "SVG 文件不存在或不是普通文件：{}",
        path.display()
    );
    let is_svg = path
        .extension()
        .and_then(OsStr::to_str)
        .is_some_and(|extension| extension.eq_ignore_ascii_case("svg"));
    ensure!(is_svg, "输入文件必须使用 .svg 扩展名：{}", path.display());
    Ok(())
}

fn output_path(svg_path: &Path, format: OutputFormat) -> Result<PathBuf> {
    let stem = svg_path
        .file_stem()
        .context("无法从 SVG 文件名确定输出文件名称")?;
    Ok(svg_path
        .with_file_name(stem)
        .with_extension(format.extension()))
}

fn create_pixmap(size: OutputSize) -> Result<Pixmap> {
    let pixel_count = u64::from(size.width) * u64::from(size.height);
    let byte_count = pixel_count
        .checked_mul(4)
        .and_then(|count| usize::try_from(count).ok())
        .context("输出图像尺寸过大")?;
    let mut pixels = Vec::new();
    pixels
        .try_reserve_exact(byte_count)
        .map_err(|error| anyhow::anyhow!("无法为渲染画布分配内存：{error}"))?;
    pixels.resize(byte_count, 0);

    let size = IntSize::from_wh(size.width, size.height).context("输出图像尺寸无效")?;
    Pixmap::from_vec(pixels, size).context("无法创建渲染画布")
}

fn write_output_file(path: &Path, data: &[u8]) -> Result<()> {
    let mut file = match OpenOptions::new().write(true).create_new(true).open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            anyhow::bail!("输出文件已存在，已停止以避免覆盖：{}", path.display());
        }
        Err(error) => {
            return Err(error).with_context(|| format!("无法创建图像 {}", path.display()));
        }
    };

    if let Err(error) = file.write_all(data) {
        drop(file);
        let cleanup_error = fs::remove_file(path).err();
        let cleanup_message = cleanup_error
            .map(|error| format!("；同时无法删除不完整文件：{error}"))
            .unwrap_or_default();
        anyhow::bail!("无法写入图像 {}：{error}{cleanup_message}", path.display());
    }

    Ok(())
}

fn parse_size(value: &str) -> std::result::Result<SizeSpec, String> {
    let (width, height) = value
        .split_once([':', '@', '*'])
        .ok_or_else(|| "图像尺寸必须使用“宽:高”、“宽@高”或“宽*高”格式，例如 1000:-1".to_owned())?;
    if height.contains([':', '@', '*']) {
        return Err("图像尺寸必须使用“宽:高”、“宽@高”或“宽*高”格式，例如 1000:-1".to_owned());
    }
    let width = parse_size_component(width.trim(), "宽")?;
    let height = parse_size_component(height.trim(), "高")?;

    match (width, height) {
        (Some(width), Some(height)) => {
            let size = OutputSize { width, height };
            validate_pixel_count(size)?;
            Ok(SizeSpec::Exact(size))
        }
        (Some(width), None) => Ok(SizeSpec::FixedWidth(width)),
        (None, Some(height)) => Ok(SizeSpec::FixedHeight(height)),
        (None, None) => Err("图像宽和高不能同时为 -1".to_owned()),
    }
}

fn parse_size_component(value: &str, name: &str) -> std::result::Result<Option<u32>, String> {
    if value == "-1" {
        return Ok(None);
    }

    let parsed = value
        .parse::<u32>()
        .map_err(|_| format!("图像{name}必须是正整数或 -1"))?;
    if parsed == 0 || parsed > MAX_RENDER_DIMENSION {
        return Err(format!(
            "图像{name}必须为 -1，或在 1 到 {MAX_RENDER_DIMENSION} 之间"
        ));
    }
    Ok(Some(parsed))
}

fn parse_quality(value: &str) -> std::result::Result<f32, String> {
    let quality = value
        .parse::<f32>()
        .map_err(|_| "质量必须是 0.0 到 1.0 之间的数字".to_owned())?;
    if !quality.is_finite() || !(0.0..=1.0).contains(&quality) {
        return Err("质量必须在 0.0 到 1.0 之间".to_owned());
    }
    Ok(quality)
}

fn validate_pixel_count(size: OutputSize) -> std::result::Result<(), String> {
    let pixel_count = u64::from(size.width) * u64::from(size.height);
    if pixel_count > MAX_RENDER_PIXELS {
        return Err(format!(
            "输出图像总像素数不能超过 {MAX_RENDER_PIXELS}，当前为 {pixel_count}"
        ));
    }
    Ok(())
}

fn resolve_render_plan(
    spec: SizeSpec,
    source_width: f32,
    source_height: f32,
) -> Result<RenderPlan> {
    ensure!(
        source_width.is_finite()
            && source_height.is_finite()
            && source_width > 0.0
            && source_height > 0.0,
        "SVG 画布尺寸无效：{source_width}x{source_height}"
    );

    let plan = match spec {
        SizeSpec::Exact(size) => RenderPlan {
            size,
            scale_x: size.width as f32 / source_width,
            scale_y: size.height as f32 / source_height,
        },
        SizeSpec::FixedWidth(width) => {
            let scale = f64::from(width) / f64::from(source_width);
            let height = scaled_dimension(f64::from(source_height) * scale, "高")?;
            RenderPlan {
                size: OutputSize { width, height },
                scale_x: scale as f32,
                scale_y: scale as f32,
            }
        }
        SizeSpec::FixedHeight(height) => {
            let scale = f64::from(height) / f64::from(source_height);
            let width = scaled_dimension(f64::from(source_width) * scale, "宽")?;
            RenderPlan {
                size: OutputSize { width, height },
                scale_x: scale as f32,
                scale_y: scale as f32,
            }
        }
    };

    ensure!(
        plan.scale_x.is_finite()
            && plan.scale_y.is_finite()
            && plan.scale_x > 0.0
            && plan.scale_y > 0.0,
        "计算得到的缩放比例无效"
    );
    validate_pixel_count(plan.size).map_err(anyhow::Error::msg)?;
    Ok(plan)
}

fn scaled_dimension(value: f64, name: &str) -> Result<u32> {
    ensure!(value.is_finite() && value > 0.0, "计算得到的图像{name}无效");
    let rounded = value.round().max(1.0);
    ensure!(
        rounded <= f64::from(MAX_RENDER_DIMENSION),
        "按比例计算的图像{name}超过 {MAX_RENDER_DIMENSION} 像素"
    );
    Ok(rounded as u32)
}

fn validate_format_dimensions(format: OutputFormat, size: OutputSize) -> Result<()> {
    validate_pixel_count(size).map_err(anyhow::Error::msg)?;
    if format == OutputFormat::Webp {
        ensure!(
            size.width <= MAX_WEBP_DIMENSION && size.height <= MAX_WEBP_DIMENSION,
            "WebP 的单边尺寸不能超过 {MAX_WEBP_DIMENSION} 像素，当前为 {}x{}",
            size.width,
            size.height
        );
    }
    Ok(())
}

fn encode_rgba(
    rgba: &[u8],
    size: OutputSize,
    format: OutputFormat,
    quality: f32,
) -> Result<Vec<u8>> {
    match format {
        OutputFormat::Webp => encode_webp(rgba, size, quality),
        OutputFormat::Png => encode_png(rgba, size),
        OutputFormat::Jpg => encode_jpeg(rgba, size, quality),
    }
}

fn encode_webp(rgba: &[u8], size: OutputSize, quality: f32) -> Result<Vec<u8>> {
    let encoded = WebpEncoder::from_rgba(rgba, size.width, size.height)
        .encode_simple(false, quality * 100.0)
        .map_err(|error| anyhow::anyhow!("WebP 编码器返回错误：{error:?}"))?;
    Ok(encoded.to_vec())
}

fn encode_png(rgba: &[u8], size: OutputSize) -> Result<Vec<u8>> {
    let mut output = Vec::new();
    PngEncoder::new_with_quality(&mut output, CompressionType::Default, FilterType::Adaptive)
        .write_image(rgba, size.width, size.height, ExtendedColorType::Rgba8)
        .context("PNG 编码器返回错误")?;
    Ok(output)
}

fn encode_jpeg(rgba: &[u8], size: OutputSize, quality: f32) -> Result<Vec<u8>> {
    let rgb_len = (size.width as usize) * (size.height as usize) * 3;
    let mut rgb = Vec::new();
    rgb.try_reserve_exact(rgb_len)
        .map_err(|error| anyhow::anyhow!("无法为 JPG 编码分配内存：{error}"))?;
    for pixel in rgba.chunks_exact(4) {
        let alpha = u16::from(pixel[3]);
        for channel in &pixel[..3] {
            let composited =
                ((u16::from(*channel) * alpha + 255 * (255 - alpha) + 127) / 255) as u8;
            rgb.push(composited);
        }
    }

    let jpeg_quality = ((quality * 100.0).round() as u8).clamp(1, 100);
    let mut output = Vec::new();
    JpegEncoder::new_with_quality(&mut output, jpeg_quality)
        .write_image(&rgb, size.width, size.height, ExtendedColorType::Rgb8)
        .context("JPG 编码器返回错误")?;
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;
    use image::GenericImageView;
    use resvg::usvg::fontdb::{Family, Query, Stretch, Style, Weight};

    #[test]
    fn parses_output_size() {
        let exact = SizeSpec::Exact(OutputSize {
            width: 2048,
            height: 1024,
        });
        assert_eq!(parse_size("2048:1024").unwrap(), exact);
        assert_eq!(parse_size("2048@1024").unwrap(), exact);
        assert_eq!(parse_size("2048*1024").unwrap(), exact);
        assert_eq!(parse_size("1000:-1").unwrap(), SizeSpec::FixedWidth(1000));
        assert_eq!(parse_size("1000@-1").unwrap(), SizeSpec::FixedWidth(1000));
        assert_eq!(parse_size("1000*-1").unwrap(), SizeSpec::FixedWidth(1000));
        assert_eq!(parse_size("-1:1000").unwrap(), SizeSpec::FixedHeight(1000));
        assert_eq!(parse_size("-1@1000").unwrap(), SizeSpec::FixedHeight(1000));
        assert_eq!(parse_size("-1*1000").unwrap(), SizeSpec::FixedHeight(1000));
        assert!(parse_size("2048x1024").is_err());
        assert!(parse_size("2048:1024:768").is_err());
        assert!(parse_size("0:1024").is_err());
        assert!(parse_size("65536:1024").is_err());
        assert!(parse_size("-2:1024").is_err());
        assert!(parse_size("-1:-1").is_err());
        assert!(parse_size("8001:8000").is_err());
        assert!(parse_size("8000:8000").is_ok());
    }

    #[test]
    fn resolves_adaptive_size_without_stretching() {
        let by_width = resolve_render_plan(SizeSpec::FixedWidth(1000), 400.0, 200.0).unwrap();
        assert_eq!(
            by_width.size,
            OutputSize {
                width: 1000,
                height: 500
            }
        );
        assert_eq!(by_width.scale_x, by_width.scale_y);

        let by_height = resolve_render_plan(SizeSpec::FixedHeight(1000), 400.0, 200.0).unwrap();
        assert_eq!(
            by_height.size,
            OutputSize {
                width: 2000,
                height: 1000
            }
        );
        assert_eq!(by_height.scale_x, by_height.scale_y);
    }

    #[test]
    fn parses_quality() {
        assert_eq!(parse_quality("0.85").unwrap(), 0.85);
        assert_eq!(parse_quality("0").unwrap(), 0.0);
        assert_eq!(parse_quality("1").unwrap(), 1.0);
        assert!(parse_quality("-0.1").is_err());
        assert!(parse_quality("1.01").is_err());
        assert!(parse_quality("NaN").is_err());
    }

    #[test]
    fn command_line_defaults_to_webp_and_point_eighty_five_quality() {
        let cli =
            Cli::try_parse_from(["svg2image", "--file", "image.svg", "--size", "-1:1024"]).unwrap();
        assert_eq!(cli.size, SizeSpec::FixedHeight(1024));
        assert_eq!(cli.format, OutputFormat::Webp);
        assert_eq!(cli.quality, DEFAULT_QUALITY);
    }

    #[test]
    fn help_includes_quoted_size_examples() {
        let help = Cli::command().render_help().to_string();
        assert!(help.contains("--size '1000:-1'"));
        assert!(help.contains("--size '-1@1000'"));
        assert!(help.contains("--size '1600*-1'"));
    }

    #[test]
    fn encodes_png_and_jpeg() {
        let size = OutputSize {
            width: 2,
            height: 3,
        };
        let rgba = [240, 128, 64, 255].repeat(6);
        let png = encode_rgba(&rgba, size, OutputFormat::Png, DEFAULT_QUALITY).unwrap();
        assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"));
        let decoded_png =
            image::load_from_memory_with_format(&png, image::ImageFormat::Png).unwrap();
        assert_eq!(decoded_png.dimensions(), (2, 3));

        let jpeg = encode_rgba(&rgba, size, OutputFormat::Jpg, DEFAULT_QUALITY).unwrap();
        assert!(jpeg.starts_with(&[0xff, 0xd8]));
        let decoded_jpeg =
            image::load_from_memory_with_format(&jpeg, image::ImageFormat::Jpeg).unwrap();
        assert_eq!(decoded_jpeg.dimensions(), (2, 3));
    }

    #[test]
    fn converts_an_svg_with_text_to_webp() {
        let temp = tempfile::tempdir().unwrap();
        let svg_path = temp.path().join("sample.svg");
        fs::write(&svg_path, r#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="50"><rect width="100" height="50" fill="red"/><text x="10" y="35" fill="white" font-family="DejaVu Sans" font-size="24">Hi</text></svg>"#).unwrap();

        convert(Cli {
            file: svg_path,
            size: SizeSpec::FixedWidth(200),
            format: OutputFormat::Webp,
            quality: DEFAULT_QUALITY,
        })
        .unwrap();

        let bytes = fs::read(temp.path().join("sample.webp")).unwrap();
        let image = image::load_from_memory_with_format(&bytes, image::ImageFormat::WebP).unwrap();
        assert_eq!(image.dimensions(), (200, 100));
    }

    #[test]
    fn warns_for_the_default_font_when_no_local_fonts_are_available() {
        let temp = tempfile::tempdir().unwrap();
        let svg_path = temp.path().join("no-local-fonts.svg");
        fs::write(
            &svg_path,
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="50"><text x="5" y="30">Text</text></svg>"#,
        )
        .unwrap();

        let mut warnings = Vec::new();
        convert_with_options(
            Cli {
                file: svg_path,
                size: SizeSpec::Exact(OutputSize {
                    width: 100,
                    height: 50,
                }),
                format: OutputFormat::Png,
                quality: DEFAULT_QUALITY,
            },
            &mut warnings,
            |_| {},
        )
        .unwrap();

        assert!(temp.path().join("no-local-fonts.png").is_file());
        assert_eq!(
            String::from_utf8(warnings).unwrap(),
            "警告：本地未找到以下 SVG 字体族组合：“Times New Roman”；将继续按现有字体回退规则转换，相关文本可能显示异常或缺失。\n"
        );
    }

    #[test]
    fn warns_once_for_missing_declared_fonts_and_still_converts() {
        let temp = tempfile::tempdir().unwrap();
        let svg_path = temp.path().join("missing-fonts.svg");
        fs::write(
            &svg_path,
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="80">
                <rect width="200" height="80" fill="white"/>
                <text x="5" y="30" font-family="Definitely Missing B, Definitely Missing A">One</text>
                <text x="5" y="60" font-family="Definitely Missing A">Two</text>
            </svg>"#,
        )
        .unwrap();

        let mut warnings = Vec::new();
        convert_with_warning_output(
            Cli {
                file: svg_path,
                size: SizeSpec::Exact(OutputSize {
                    width: 200,
                    height: 80,
                }),
                format: OutputFormat::Png,
                quality: DEFAULT_QUALITY,
            },
            &mut warnings,
        )
        .unwrap();

        assert!(temp.path().join("missing-fonts.png").is_file());
        assert_eq!(
            String::from_utf8(warnings).unwrap(),
            "警告：本地未找到以下 SVG 字体族组合：“Definitely Missing A”、“Definitely Missing B, Definitely Missing A”；将继续按现有字体回退规则转换，相关文本可能显示异常或缺失。\n"
        );
    }

    #[test]
    fn does_not_warn_when_a_later_declared_font_is_available() {
        let mut options = Options::default();
        options.fontdb_mut().load_system_fonts();
        if options
            .fontdb
            .query(&Query {
                families: &[DatabaseFamily::Name("DejaVu Serif")],
                weight: Weight::NORMAL,
                stretch: resvg::usvg::fontdb::Stretch::Normal,
                style: resvg::usvg::fontdb::Style::Normal,
            })
            .is_none()
        {
            #[cfg(target_os = "linux")]
            panic!("Linux test environment must provide DejaVu Serif");
            #[cfg(not(target_os = "linux"))]
            return;
        }

        let warnings = MissingFontWarnings::default();
        warnings.install(&mut options);
        Tree::from_str(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="50"><text x="0" y="30" font-family="Definitely Missing, DejaVu Serif">Text</text></svg>"#,
            &options,
        )
        .unwrap();

        assert_eq!(warnings.message(), None);
    }

    #[test]
    fn font_fallback_does_not_replace_an_entire_mixed_script_run() {
        let mut options = Options::default();
        options.fontdb_mut().load_system_fonts();

        let primary = options.fontdb.query(&Query {
            families: &[Family::Name("DejaVu Serif")],
            weight: Weight::NORMAL,
            stretch: Stretch::Normal,
            style: Style::Normal,
        });
        if primary.is_none() {
            // DejaVu is available on the Linux release/test runners. Keep the
            // test portable for contributors using a different local OS.
            #[cfg(target_os = "linux")]
            panic!("Linux test environment must provide DejaVu Serif");
            #[cfg(not(target_os = "linux"))]
            return;
        }
        let primary = primary.unwrap();

        let tree = Tree::from_str(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="400" height="100"><text x="0" y="50" font-family="DejaVu Serif" font-size="40">y, אTest</text></svg>"#,
            &options,
        )
        .unwrap();

        let text = tree
            .root()
            .children()
            .iter()
            .find_map(|node| match node {
                resvg::usvg::Node::Text(text) => Some(text),
                _ => None,
            })
            .unwrap();
        let glyphs = text
            .layouted()
            .iter()
            .flat_map(|span| span.positioned_glyphs.iter())
            .collect::<Vec<_>>();

        let latin_fonts = glyphs
            .iter()
            .filter(|glyph| {
                glyph
                    .text
                    .chars()
                    .any(|character| character.is_ascii_alphabetic())
            })
            .map(|glyph| glyph.font)
            .collect::<Vec<_>>();
        assert!(!latin_fonts.is_empty());
        assert!(latin_fonts.iter().all(|font| *font == primary));

        let fallback_fonts = glyphs
            .iter()
            .filter(|glyph| glyph.text.contains('א'))
            .map(|glyph| glyph.font)
            .collect::<Vec<_>>();
        assert_eq!(fallback_fonts.len(), 1);
        assert_ne!(fallback_fonts[0], primary);
        assert_eq!(
            tree.fontdb().face(fallback_fonts[0]).unwrap().weight,
            Weight::NORMAL
        );
    }

    #[test]
    fn font_fallback_follows_the_declared_family_order_and_weight() {
        let mut options = Options::default();
        options.fontdb_mut().load_system_fonts();

        let primary = options.fontdb.query(&Query {
            families: &[Family::Name("DejaVu Serif")],
            weight: Weight::BOLD,
            stretch: Stretch::Normal,
            style: Style::Normal,
        });
        let declared_fallback = options.fontdb.query(&Query {
            families: &[Family::Name("DejaVu Sans Mono")],
            weight: Weight::BOLD,
            stretch: Stretch::Normal,
            style: Style::Normal,
        });
        if primary.is_none() || declared_fallback.is_none() {
            // DejaVu is available on the Linux release/test runners. Keep the
            // test portable for contributors using a different local OS.
            #[cfg(target_os = "linux")]
            panic!("Linux test environment must provide bold DejaVu Serif and Sans Mono");
            #[cfg(not(target_os = "linux"))]
            return;
        }
        let primary = primary.unwrap();
        let declared_fallback = declared_fallback.unwrap();

        let tree = Tree::from_str(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="400" height="100"><text x="0" y="50" font-family="Missing Font, DejaVu Serif, DejaVu Sans Mono" font-size="40" font-weight="600">y, اTest</text></svg>"#,
            &options,
        )
        .unwrap();

        let text = tree
            .root()
            .children()
            .iter()
            .find_map(|node| match node {
                resvg::usvg::Node::Text(text) => Some(text),
                _ => None,
            })
            .unwrap();
        let glyphs = text
            .layouted()
            .iter()
            .flat_map(|span| span.positioned_glyphs.iter())
            .collect::<Vec<_>>();

        let latin_fonts = glyphs
            .iter()
            .filter(|glyph| {
                glyph
                    .text
                    .chars()
                    .any(|character| character.is_ascii_alphabetic())
            })
            .map(|glyph| glyph.font)
            .collect::<Vec<_>>();
        assert!(!latin_fonts.is_empty());
        assert!(latin_fonts.iter().all(|font| *font == primary));

        let fallback_fonts = glyphs
            .iter()
            .filter(|glyph| glyph.text.contains('ا'))
            .map(|glyph| glyph.font)
            .collect::<Vec<_>>();
        assert_eq!(fallback_fonts, vec![declared_fallback]);
        assert_eq!(
            tree.fontdb().face(fallback_fonts[0]).unwrap().weight,
            Weight::BOLD
        );
    }

    #[test]
    fn converts_using_an_adaptive_height() {
        let temp = tempfile::tempdir().unwrap();
        let svg_path = temp.path().join("adaptive.svg");
        fs::write(
            &svg_path,
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="400" height="200"><rect width="400" height="200" fill="blue"/></svg>"#,
        )
        .unwrap();

        convert(Cli {
            file: svg_path,
            size: SizeSpec::FixedHeight(250),
            format: OutputFormat::Png,
            quality: DEFAULT_QUALITY,
        })
        .unwrap();

        let bytes = fs::read(temp.path().join("adaptive.png")).unwrap();
        let image = image::load_from_memory_with_format(&bytes, image::ImageFormat::Png).unwrap();
        assert_eq!(image.dimensions(), (500, 250));
    }

    #[test]
    fn preserves_color_channels_for_transparent_png_pixels() {
        let temp = tempfile::tempdir().unwrap();
        let svg_path = temp.path().join("transparent.svg");
        fs::write(
            &svg_path,
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"><rect width="10" height="10" fill="red" fill-opacity="0.5"/></svg>"#,
        )
        .unwrap();

        convert(Cli {
            file: svg_path,
            size: SizeSpec::Exact(OutputSize {
                width: 10,
                height: 10,
            }),
            format: OutputFormat::Png,
            quality: DEFAULT_QUALITY,
        })
        .unwrap();

        let bytes = fs::read(temp.path().join("transparent.png")).unwrap();
        let image = image::load_from_memory_with_format(&bytes, image::ImageFormat::Png)
            .unwrap()
            .into_rgba8();
        let pixel = image.get_pixel(5, 5).0;
        assert_eq!(&pixel[..3], &[255, 0, 0]);
        assert!((127..=128).contains(&pixel[3]));
    }

    #[test]
    fn atomically_rejects_existing_output_file() {
        let temp = tempfile::tempdir().unwrap();
        let output = temp.path().join("image.png");
        fs::write(&output, b"keep me").unwrap();
        let error = write_output_file(&output, b"replacement").unwrap_err();
        assert!(error.to_string().contains("已存在"));
        assert_eq!(fs::read(output).unwrap(), b"keep me");
    }
}
