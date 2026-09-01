//! Create a non-destructive grid from the highest-sum per-user heatmaps.
//!
//! Every selected EXR is cropped to its upper-left square and normalized by
//! that crop's own maximum component.  The clean grid, a transparent username
//! overlay, a composited preview, and reproducibility metadata are written to a
//! separate output directory.  Source EXRs are only ever opened for reading.

use anyhow::{bail, Context, Result};
use clap::{Parser, ValueEnum};
use image::{ImageBuffer, ImageFormat, Rgb};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const DEFAULT_COUNT: usize = 25;
const DEFAULT_COLUMNS: usize = 5;
const DEFAULT_TILE_SIZE: u32 = 460;
const DEFAULT_PADDING: u32 = 24;
const DEFAULT_GUTTER: u32 = 16;
const LABEL_BAND_HEIGHT: u32 = 42;

#[derive(Debug, Clone, Copy, ValueEnum)]
enum LabelPlacement {
    /// Draw labels over the top of each heatmap tile.
    Overlay,
    /// Draw labels in the padding immediately above each heatmap tile.
    Above,
}

impl LabelPlacement {
    fn manifest_name(self) -> &'static str {
        match self {
            Self::Overlay => "overlay",
            Self::Above => "above tile in padding",
        }
    }
}

#[derive(Debug, Parser)]
#[command(about = "Build a normalized grid of the highest-pixel-sum user EXRs")]
struct Args {
    /// Directory containing the original per-user EXRs and manifest.json.
    #[arg(long, default_value = "images_users")]
    input: PathBuf,

    /// Separate destination directory; source files are never modified.
    #[arg(long, default_value = "images_users_top25_grid")]
    output: PathBuf,

    /// Prefix used for generated asset filenames.
    #[arg(long, default_value = "top25")]
    output_prefix: String,

    /// Number of highest-sum maps to select.
    #[arg(long, default_value_t = DEFAULT_COUNT)]
    count: usize,

    /// Number of tile columns.
    #[arg(long, default_value_t = DEFAULT_COLUMNS)]
    columns: usize,

    /// Width and height of each upper-left crop.
    #[arg(long, default_value_t = DEFAULT_TILE_SIZE)]
    tile_size: u32,

    /// Padding around the outside of the grid.
    #[arg(long, default_value_t = DEFAULT_PADDING)]
    padding: u32,

    /// Spacing between neighboring tiles.
    #[arg(long, default_value_t = DEFAULT_GUTTER)]
    gutter: u32,

    /// Whether labels cover tiles or sit in the padding above them.
    #[arg(long, value_enum, default_value_t = LabelPlacement::Overlay)]
    label_placement: LabelPlacement,

    /// Replace only this program's known outputs in the destination directory.
    #[arg(long)]
    overwrite: bool,
}

#[derive(Debug, Deserialize)]
struct SourceManifest {
    width: u32,
    height: u32,
    users: Vec<SourceUser>,
}

#[derive(Debug, Deserialize)]
struct SourceUser {
    canonical_username: String,
    filename: String,
}

#[derive(Debug)]
struct RankedUser {
    canonical_username: String,
    filename: String,
    full_image_pixel_sum: f64,
}

#[derive(Debug, Serialize)]
struct GridManifest {
    source_directory: String,
    source_manifest: String,
    ranking_basis: &'static str,
    crop: CropDescription,
    normalization: &'static str,
    layout: LayoutDescription,
    outputs: OutputDescription,
    users: Vec<GridUser>,
}

#[derive(Debug, Serialize)]
struct CropDescription {
    origin_x: u32,
    origin_y: u32,
    width: u32,
    height: u32,
}

#[derive(Debug, Serialize)]
struct LayoutDescription {
    columns: usize,
    rows: usize,
    tile_size: u32,
    outer_padding: u32,
    gutter: u32,
    canvas_width: u32,
    canvas_height: u32,
    label_placement: &'static str,
}

#[derive(Debug, Serialize)]
struct OutputDescription {
    clean_exr: String,
    clean_png: String,
    username_overlay_svg: String,
    username_overlay_png: String,
    labeled_png: String,
}

#[derive(Debug, Serialize)]
struct GridUser {
    rank: usize,
    canonical_username: String,
    source_filename: String,
    full_image_pixel_sum: f64,
    crop_max_component: f32,
    column: usize,
    row: usize,
    grid_x: u32,
    grid_y: u32,
}

struct OutputPaths {
    clean_exr: PathBuf,
    clean_png: PathBuf,
    overlay_svg: PathBuf,
    overlay_png: PathBuf,
    labeled_png: PathBuf,
    manifest: PathBuf,
}

fn main() -> Result<()> {
    let args = Args::parse();
    validate_args(&args)?;

    let source_manifest_path = args.input.join("manifest.json");
    let source_manifest: SourceManifest = serde_json::from_slice(
        &fs::read(&source_manifest_path)
            .with_context(|| format!("failed to read {}", source_manifest_path.display()))?,
    )
    .with_context(|| format!("failed to parse {}", source_manifest_path.display()))?;

    if args.tile_size > source_manifest.width || args.tile_size > source_manifest.height {
        bail!(
            "{}x{} crop does not fit source dimensions {}x{}",
            args.tile_size,
            args.tile_size,
            source_manifest.width,
            source_manifest.height
        );
    }
    if args.count > source_manifest.users.len() {
        bail!(
            "requested {} users, but the source manifest contains only {}",
            args.count,
            source_manifest.users.len()
        );
    }

    fs::create_dir_all(&args.output)
        .with_context(|| format!("failed to create {}", args.output.display()))?;
    let paths = output_paths(&args.output, &args.output_prefix);
    prepare_outputs(&paths, args.overwrite)?;

    eprintln!(
        "Ranking {} source EXRs by full-image mean-RGB pixel sum...",
        source_manifest.users.len()
    );
    let mut ranked: Vec<RankedUser> = source_manifest
        .users
        .par_iter()
        .map(|user| {
            rank_user(
                &args.input,
                user,
                source_manifest.width,
                source_manifest.height,
            )
        })
        .collect::<Result<Vec<_>>>()?;
    ranked.sort_by(|left, right| {
        right
            .full_image_pixel_sum
            .total_cmp(&left.full_image_pixel_sum)
            .then_with(|| left.canonical_username.cmp(&right.canonical_username))
    });
    ranked.truncate(args.count);

    let rows = args.count.div_ceil(args.columns);
    let canvas_width = canvas_extent(args.columns, args.tile_size, args.padding, args.gutter)?;
    let canvas_height = canvas_extent(rows, args.tile_size, args.padding, args.gutter)?;
    let mut grid: ImageBuffer<Rgb<f32>, Vec<f32>> = ImageBuffer::new(canvas_width, canvas_height);
    let mut grid_users = Vec::with_capacity(ranked.len());

    eprintln!(
        "Cropping and individually normalizing {} maps onto a {}x{} canvas...",
        ranked.len(),
        canvas_width,
        canvas_height
    );
    for (index, user) in ranked.iter().enumerate() {
        let image_path = args.input.join(&user.filename);
        let image = image::open(&image_path)
            .with_context(|| format!("failed to decode {}", image_path.display()))?
            .into_rgb32f();
        let crop_max_component = crop_max(&image, args.tile_size)?;
        let column = index % args.columns;
        let row = index / args.columns;
        let grid_x = args.padding + column as u32 * (args.tile_size + args.gutter);
        let grid_y = args.padding + row as u32 * (args.tile_size + args.gutter);

        for y in 0..args.tile_size {
            for x in 0..args.tile_size {
                let source = image.get_pixel(x, y).0;
                let normalized = if crop_max_component > 0.0 {
                    source.map(|component| (component / crop_max_component).clamp(0.0, 1.0))
                } else {
                    [0.0; 3]
                };
                grid.put_pixel(grid_x + x, grid_y + y, Rgb(normalized));
            }
        }

        grid_users.push(GridUser {
            rank: index + 1,
            canonical_username: user.canonical_username.clone(),
            source_filename: user.filename.clone(),
            full_image_pixel_sum: user.full_image_pixel_sum,
            crop_max_component,
            column,
            row,
            grid_x,
            grid_y,
        });
    }

    save_grid_images(&grid, &paths)?;
    let overlay_svg = build_overlay_svg(
        canvas_width,
        canvas_height,
        args.tile_size,
        args.label_placement,
        &grid_users,
    );
    write_atomic(&paths.overlay_svg, overlay_svg.as_bytes())?;
    render_overlay_and_composite(&paths)?;

    let manifest = GridManifest {
        source_directory: args.input.display().to_string(),
        source_manifest: source_manifest_path.display().to_string(),
        ranking_basis: "sum of mean RGB intensity for every pixel in each full source EXR",
        crop: CropDescription {
            origin_x: 0,
            origin_y: 0,
            width: args.tile_size,
            height: args.tile_size,
        },
        normalization: "each crop divided by its own maximum RGB component",
        layout: LayoutDescription {
            columns: args.columns,
            rows,
            tile_size: args.tile_size,
            outer_padding: args.padding,
            gutter: args.gutter,
            canvas_width,
            canvas_height,
            label_placement: args.label_placement.manifest_name(),
        },
        outputs: OutputDescription {
            clean_exr: output_filename(&paths.clean_exr),
            clean_png: output_filename(&paths.clean_png),
            username_overlay_svg: output_filename(&paths.overlay_svg),
            username_overlay_png: output_filename(&paths.overlay_png),
            labeled_png: output_filename(&paths.labeled_png),
        },
        users: grid_users,
    };
    let manifest_json = serde_json::to_vec_pretty(&manifest)?;
    write_atomic(&paths.manifest, &manifest_json)?;

    verify_output_dimensions(&paths, canvas_width, canvas_height)?;
    eprintln!(
        "Wrote clean, overlay, and labeled grid outputs to {}",
        args.output.display()
    );
    Ok(())
}

fn validate_args(args: &Args) -> Result<()> {
    if args.count == 0 {
        bail!("--count must be greater than zero");
    }
    if args.columns == 0 {
        bail!("--columns must be greater than zero");
    }
    if args.tile_size == 0 {
        bail!("--tile-size must be greater than zero");
    }
    if args.output_prefix.is_empty()
        || !args
            .output_prefix
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        bail!("--output-prefix may contain only ASCII letters, digits, '_' and '-'");
    }
    if matches!(args.label_placement, LabelPlacement::Above)
        && (args.padding < LABEL_BAND_HEIGHT || args.gutter < LABEL_BAND_HEIGHT)
    {
        bail!(
            "--label-placement above requires --padding and --gutter of at least {} pixels",
            LABEL_BAND_HEIGHT
        );
    }
    Ok(())
}

fn output_paths(output: &Path, prefix: &str) -> OutputPaths {
    OutputPaths {
        clean_exr: output.join(format!("{prefix}_grid.exr")),
        clean_png: output.join(format!("{prefix}_grid.png")),
        overlay_svg: output.join(format!("{prefix}_usernames_overlay.svg")),
        overlay_png: output.join(format!("{prefix}_usernames_overlay.png")),
        labeled_png: output.join(format!("{prefix}_grid_with_usernames.png")),
        manifest: output.join("manifest.json"),
    }
}

fn output_filename(path: &Path) -> String {
    path.file_name()
        .unwrap_or(path.as_os_str())
        .to_string_lossy()
        .into_owned()
}

fn prepare_outputs(paths: &OutputPaths, overwrite: bool) -> Result<()> {
    for path in [
        &paths.clean_exr,
        &paths.clean_png,
        &paths.overlay_svg,
        &paths.overlay_png,
        &paths.labeled_png,
        &paths.manifest,
    ] {
        if path.exists() && !overwrite {
            bail!(
                "refusing to replace {}; pass --overwrite to replace generated grid outputs",
                path.display()
            );
        }
    }
    Ok(())
}

fn rank_user(
    input: &Path,
    user: &SourceUser,
    expected_width: u32,
    expected_height: u32,
) -> Result<RankedUser> {
    let path = input.join(&user.filename);
    let image = image::open(&path)
        .with_context(|| format!("failed to decode {}", path.display()))?
        .into_rgb32f();
    if image.width() != expected_width || image.height() != expected_height {
        bail!(
            "{} is {}x{}, expected {}x{}",
            path.display(),
            image.width(),
            image.height(),
            expected_width,
            expected_height
        );
    }

    let mut sum = 0.0_f64;
    for pixel in image.pixels() {
        let components = pixel.0;
        if components.iter().any(|value| !value.is_finite()) {
            bail!("{} contains a non-finite pixel", path.display());
        }
        sum += (components[0] as f64 + components[1] as f64 + components[2] as f64) / 3.0;
    }
    Ok(RankedUser {
        canonical_username: user.canonical_username.clone(),
        filename: user.filename.clone(),
        full_image_pixel_sum: sum,
    })
}

fn crop_max(image: &ImageBuffer<Rgb<f32>, Vec<f32>>, tile_size: u32) -> Result<f32> {
    let mut maximum = 0.0_f32;
    for y in 0..tile_size {
        for x in 0..tile_size {
            for component in image.get_pixel(x, y).0 {
                if !component.is_finite() {
                    bail!("crop contains a non-finite pixel");
                }
                maximum = maximum.max(component);
            }
        }
    }
    Ok(maximum)
}

fn canvas_extent(count: usize, tile_size: u32, padding: u32, gutter: u32) -> Result<u32> {
    let count = u32::try_from(count).context("grid is too large")?;
    let tiles = count.checked_mul(tile_size).context("grid is too large")?;
    let gaps = count
        .saturating_sub(1)
        .checked_mul(gutter)
        .context("grid is too large")?;
    tiles
        .checked_add(gaps)
        .and_then(|value| value.checked_add(padding.saturating_mul(2)))
        .context("grid is too large")
}

fn save_grid_images(grid: &ImageBuffer<Rgb<f32>, Vec<f32>>, paths: &OutputPaths) -> Result<()> {
    let exr_temporary = temporary_path(&paths.clean_exr);
    grid.save_with_format(&exr_temporary, ImageFormat::OpenExr)
        .with_context(|| format!("failed to write {}", exr_temporary.display()))?;
    replace_with_temporary(&exr_temporary, &paths.clean_exr)?;

    let mut png: ImageBuffer<Rgb<u16>, Vec<u16>> = ImageBuffer::new(grid.width(), grid.height());
    for (x, y, pixel) in grid.enumerate_pixels() {
        let converted = pixel
            .0
            .map(|component| (component.clamp(0.0, 1.0) * u16::MAX as f32).round() as u16);
        png.put_pixel(x, y, Rgb(converted));
    }
    let png_temporary = temporary_path(&paths.clean_png);
    png.save_with_format(&png_temporary, ImageFormat::Png)
        .with_context(|| format!("failed to write {}", png_temporary.display()))?;
    replace_with_temporary(&png_temporary, &paths.clean_png)?;
    Ok(())
}

fn build_overlay_svg(
    width: u32,
    height: u32,
    tile_size: u32,
    placement: LabelPlacement,
    users: &[GridUser],
) -> String {
    let mut svg = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width}\" height=\"{height}\" viewBox=\"0 0 {width} {height}\">\n"
    );
    for user in users {
        let label = display_label(&user.canonical_username);
        let character_count = label.chars().count().max(1) as f32;
        let font_size = (432.0 / (character_count * 0.62)).clamp(12.0, 24.0).floor() as u32;
        let label_y = match placement {
            LabelPlacement::Overlay => user.grid_y,
            LabelPlacement::Above => user.grid_y - LABEL_BAND_HEIGHT,
        };
        let baseline = label_y + 29;
        svg.push_str(&format!(
            "  <rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"black\" fill-opacity=\"0.68\"/>\n",
            user.grid_x, label_y, tile_size, LABEL_BAND_HEIGHT
        ));
        svg.push_str(&format!(
            "  <text x=\"{}\" y=\"{}\" fill=\"white\" font-family=\"DejaVu Sans, sans-serif\" font-size=\"{}\" font-weight=\"bold\">{}</text>\n",
            user.grid_x + 12,
            baseline,
            font_size,
            xml_escape(&label)
        ));
    }
    svg.push_str("</svg>\n");
    svg
}

fn display_label(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect()
}

fn xml_escape(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '\"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&apos;"),
            _ => escaped.push(character),
        }
    }
    escaped
}

fn render_overlay_and_composite(paths: &OutputPaths) -> Result<()> {
    run_command(
        Command::new("convert")
            .arg("-background")
            .arg("none")
            .arg(&paths.overlay_svg)
            .arg(&paths.overlay_png),
        "render transparent username overlay",
    )?;
    run_command(
        Command::new("convert")
            .arg(&paths.clean_png)
            .arg(&paths.overlay_png)
            .arg("-compose")
            .arg("over")
            .arg("-composite")
            .arg(&paths.labeled_png),
        "composite username overlay",
    )?;
    Ok(())
}

fn run_command(command: &mut Command, description: &str) -> Result<()> {
    let status = command
        .status()
        .with_context(|| format!("failed to launch ImageMagick to {description}"))?;
    if !status.success() {
        bail!("ImageMagick failed to {description}: {status}");
    }
    Ok(())
}

fn verify_output_dimensions(paths: &OutputPaths, width: u32, height: u32) -> Result<()> {
    for path in [
        &paths.clean_exr,
        &paths.clean_png,
        &paths.overlay_png,
        &paths.labeled_png,
    ] {
        let dimensions = image::image_dimensions(path)
            .with_context(|| format!("failed to inspect {}", path.display()))?;
        if dimensions != (width, height) {
            bail!(
                "{} is {}x{}, expected {}x{}",
                path.display(),
                dimensions.0,
                dimensions.1,
                width,
                height
            );
        }
    }
    Ok(())
}

fn write_atomic(destination: &Path, bytes: &[u8]) -> Result<()> {
    let temporary = temporary_path(destination);
    fs::write(&temporary, bytes)
        .with_context(|| format!("failed to write {}", temporary.display()))?;
    replace_with_temporary(&temporary, destination)
}

fn temporary_path(destination: &Path) -> PathBuf {
    let filename = destination
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("output");
    destination.with_file_name(format!(".{filename}.tmp"))
}

fn replace_with_temporary(temporary: &Path, destination: &Path) -> Result<()> {
    if destination.exists() {
        fs::remove_file(destination)
            .with_context(|| format!("failed to replace {}", destination.display()))?;
    }
    fs::rename(temporary, destination).with_context(|| {
        format!(
            "failed to move completed output {} to {}",
            temporary.display(),
            destination.display()
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extent_includes_padding_and_only_internal_gutters() {
        assert_eq!(canvas_extent(5, 460, 24, 16).unwrap(), 2412);
    }

    #[test]
    fn xml_escaping_preserves_safe_unicode() {
        assert_eq!(xml_escape("A&B <👀>"), "A&amp;B &lt;👀&gt;");
    }

    #[test]
    fn display_label_removes_line_controls() {
        assert_eq!(display_label("name\nother"), "name other");
    }
}
