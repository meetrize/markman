//! Mermaid fenced-block parsing and SVG rendering helpers.

use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use anyhow::{Context as _, anyhow};
use directories::ProjectDirs;

use crate::app_identity::MARKMAN_PROJECT_QUALIFIER;

const MERMAID_MIN_DISPLAY_SCALE: f32 = 0.1;
const MERMAID_MAX_DISPLAY_SCALE: f32 = 4.0;

/// Opening fence metadata for a Mermaid fenced code block.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct MermaidFence {
    /// Fence marker, either backtick or tilde.
    pub(crate) marker: char,
    /// Opening fence run length.
    pub(crate) len: usize,
}

/// Parsed Mermaid source preserved from Markdown.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct MermaidSource {
    /// Full Markdown source, including the opening and closing fences.
    pub(crate) raw: String,
    /// Mermaid diagram source between the fences.
    pub(crate) body: String,
    /// The full info string after the opening fence.
    pub(crate) info: String,
}

/// Result of rendering a Mermaid diagram into a cached display image.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct MermaidSvgRender {
    /// Path to the PNG file consumed by GPUI's image element.
    pub(crate) path: PathBuf,
    /// Scaled SVG document content, used by export paths.
    pub(crate) svg: String,
    /// Logical display width in pixels.
    pub(crate) display_width: f32,
    /// Logical display height in pixels.
    pub(crate) display_height: f32,
    /// Scale applied to the renderer's intrinsic SVG size for editor display.
    pub(crate) display_scale: f32,
}

/// Concrete dimensions encoded into a display SVG.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct MermaidSvgSize {
    pub(crate) width: f32,
    pub(crate) height: f32,
}

/// Returns true when a fenced code info string declares Mermaid content.
pub(crate) fn is_mermaid_info_string(info: Option<&str>) -> bool {
    info.and_then(|info| info.split_whitespace().next())
        .is_some_and(|first| {
            first.eq_ignore_ascii_case("mermaid") || first.eq_ignore_ascii_case("mmd")
        })
}

/// Parse a line as a Mermaid opening fence.
pub(crate) fn parse_mermaid_fence_start(line: &str) -> Option<MermaidFence> {
    let trimmed = strip_fence_indent(line)?.trim_end();
    let marker = trimmed.chars().next()?;
    if marker != '`' && marker != '~' {
        return None;
    }

    let len = trimmed.chars().take_while(|ch| *ch == marker).count();
    if len < 3 {
        return None;
    }

    let info = trimmed[marker.len_utf8() * len..].trim();
    if marker == '`' && info.contains('`') {
        return None;
    }

    is_mermaid_info_string((!info.is_empty()).then_some(info))
        .then_some(MermaidFence { marker, len })
}

/// Returns true when `line` closes the given Mermaid fence.
pub(crate) fn is_mermaid_closing_fence(line: &str, fence: MermaidFence) -> bool {
    let Some(trimmed) = strip_fence_indent(line).map(str::trim_end) else {
        return false;
    };
    if !trimmed.starts_with(fence.marker) {
        return false;
    }

    let len = trimmed.chars().take_while(|ch| *ch == fence.marker).count();
    len >= fence.len && trimmed[fence.marker.len_utf8() * len..].trim().is_empty()
}

/// Parse raw fenced Markdown into the Mermaid diagram source it contains.
pub(crate) fn parse_mermaid_fence_source(raw: &str) -> Option<MermaidSource> {
    let raw = raw.trim_matches('\n').to_string();
    let lines = raw.split('\n').collect::<Vec<_>>();
    if lines.len() < 2 {
        return None;
    }

    let opening = strip_fence_indent(lines[0])?.trim_end();
    let fence = parse_mermaid_fence_start(opening)?;
    let info = opening[fence.marker.len_utf8() * fence.len..]
        .trim()
        .to_string();
    if !is_mermaid_closing_fence(lines.last()?, fence) {
        return None;
    }

    let body = lines[1..lines.len() - 1].join("\n");
    Some(MermaidSource { raw, body, info })
}

/// Render Mermaid source into a cached PNG sized for editor display.
pub(crate) fn render_mermaid_svg_for_display(
    source: &MermaidSource,
    available_width: f32,
    available_height: f32,
    device_scale: f32,
) -> anyhow::Result<MermaidSvgRender> {
    render_mermaid_svg_for_display_with(
        source,
        available_width,
        available_height,
        device_scale,
        render_mermaid_raw,
    )
}

fn render_mermaid_svg_for_display_with(
    source: &MermaidSource,
    available_width: f32,
    available_height: f32,
    device_scale: f32,
    renderer: MermaidRenderer,
) -> anyhow::Result<MermaidSvgRender> {
    let base_key = mermaid_cache_key(&source.body);
    let base_path = mermaid_base_cache_path(&base_key)?;
    let base_svg = render_mermaid_to_svg_cached_with(&source.body, &base_path, renderer)?;
    let intrinsic = mermaid_svg_intrinsic_size(&base_svg)?;
    let scale = mermaid_display_scale(
        &source.body,
        intrinsic.width,
        intrinsic.height,
        available_width,
        available_height,
    );

    let display_key = mermaid_display_cache_key(&source.body, scale, device_scale);
    let display_path = mermaid_display_cache_path(&display_key)?;
    if display_path.exists() {
        let (svg, size) = scale_mermaid_svg_for_display(&base_svg, scale)?;
        return Ok(MermaidSvgRender {
            path: display_path,
            svg,
            display_width: size.width,
            display_height: size.height,
            display_scale: scale,
        });
    }

    let (svg, size) = scale_mermaid_svg_for_display(&base_svg, scale)?;
    let png = rasterize_mermaid_svg_for_display(&svg, size, device_scale)?;
    fs::write(&display_path, &png).with_context(|| {
        format!(
            "failed to write Mermaid display PNG cache '{}'",
            display_path.display()
        )
    })?;
    Ok(MermaidSvgRender {
        path: display_path,
        svg,
        display_width: size.width,
        display_height: size.height,
        display_scale: scale,
    })
}

/// Render a Mermaid diagram body into cached SVG text.
pub(crate) fn render_mermaid_to_svg(source: &str) -> anyhow::Result<String> {
    let key = mermaid_cache_key(source);
    let path = mermaid_base_cache_path(&key)?;
    render_mermaid_to_svg_cached_with(source, &path, render_mermaid_raw)
}

type MermaidRenderer = fn(&str) -> anyhow::Result<String>;

fn render_mermaid_to_svg_cached_with(
    source: &str,
    path: &Path,
    renderer: MermaidRenderer,
) -> anyhow::Result<String> {
    if path.exists() {
        return fs::read_to_string(path).with_context(|| {
            format!("failed to read Mermaid base SVG cache '{}'", path.display())
        });
    }

    let svg = renderer(source)?;
    fs::write(path, &svg).with_context(|| {
        format!(
            "failed to write Mermaid base SVG cache '{}'",
            path.display()
        )
    })?;
    Ok(svg)
}

fn render_mermaid_raw(source: &str) -> anyhow::Result<String> {
    if !looks_like_supported_mermaid_source(source) {
        return Err(anyhow::anyhow!("unsupported Mermaid diagram"));
    }
    let mut options = mermaid_rs_renderer::RenderOptions::modern();
    options.theme.font_family = mermaid_theme_font_family().to_string();
    options.theme.font_size = 16.0;
    let svg =
        mermaid_rs_renderer::render_with_options(source, options).map_err(|err| anyhow::anyhow!("{err}"))?;
    if svg.contains("class=\"error-text\"") || svg.contains("Syntax error in text") {
        return Err(anyhow::anyhow!("Mermaid syntax error"));
    }
    Ok(patch_mermaid_svg_for_display(&svg))
}

/// Embedded Source Han Sans SC (思源黑体) for consistent Mermaid label rendering.
const SOURCE_HAN_SANS_SC_REGULAR: &[u8] =
    include_bytes!("../../../assets/fonts/SourceHanSansSC-Regular.otf");

/// Font stack passed to the Mermaid renderer for layout measurement.
fn mermaid_theme_font_family() -> &'static str {
    "Source Han Sans SC, sans-serif"
}

/// Primary font resolved by our resvg font database.
fn mermaid_primary_font_name() -> &'static str {
    "Source Han Sans SC"
}

/// Quoted `font-family` stack embedded into SVG before rasterization.
fn mermaid_resvg_font_stack() -> &'static str {
    "'Source Han Sans SC',sans-serif"
}

/// Rewrites Mermaid SVG so resvg can resolve CJK labels reliably.
fn patch_mermaid_svg_for_display(svg: &str) -> String {
    let mut patched = inject_mermaid_svg_font_fallbacks(svg);
    patched = substitute_unrenderable_emoji_in_svg(&patched);
    patched
}

fn mermaid_display_font_stack() -> String {
    mermaid_resvg_font_stack().to_string()
}

fn inject_mermaid_svg_font_fallbacks(svg: &str) -> String {
    let display_stack = mermaid_display_font_stack();
    let mut out = rewrite_all_svg_font_families(svg, &display_stack);
    if !out.contains("svg,text,tspan{font-family:") {
        if let Some(end) = out.find('>') {
            let style = format!("<style>svg,text,tspan{{font-family:{display_stack};}}</style>");
            out.insert_str(end + 1, &style);
        }
    }
    out
}

fn rewrite_all_svg_font_families(svg: &str, stack: &str) -> String {
    const MARKER: &str = "font-family";
    let mut out = String::with_capacity(svg.len() + stack.len() + 32);
    let mut index = 0;
    while let Some(rel) = svg[index..].find(MARKER) {
        let marker_start = index + rel;
        out.push_str(&svg[index..marker_start]);
        out.push_str(MARKER);
        let mut cursor = marker_start + MARKER.len();
        while cursor < svg.len() && svg.as_bytes()[cursor].is_ascii_whitespace() {
            cursor += 1;
        }
        if cursor >= svg.len() {
            index = cursor;
            break;
        }
        let delim = svg.as_bytes()[cursor];
        if delim != b'=' && delim != b':' {
            index = cursor;
            continue;
        }
        cursor += 1;
        while cursor < svg.len() && svg.as_bytes()[cursor].is_ascii_whitespace() {
            cursor += 1;
        }
        if cursor >= svg.len() {
            index = cursor;
            break;
        }

        let quote = svg.as_bytes()[cursor];
        if quote == b'"' || quote == b'\'' {
            cursor += 1;
            while cursor < svg.len() && svg.as_bytes()[cursor] != quote {
                cursor += 1;
            }
            if cursor < svg.len() {
                cursor += 1;
            }
            if delim == b'=' {
                out.push_str("=\"");
                out.push_str(stack);
                out.push('"');
            } else {
                out.push(':');
                out.push_str(stack);
            }
        } else {
            while cursor < svg.len() {
                let byte = svg.as_bytes()[cursor];
                if byte == b';' || byte == b'}' {
                    break;
                }
                cursor += 1;
            }
            if delim == b':' {
                out.push(':');
                out.push_str(stack);
            } else {
                out.push_str("=\"");
                out.push_str(stack);
                out.push('"');
            }
        }
        index = cursor;
    }
    out.push_str(&svg[index..]);
    out
}

fn configure_mermaid_fontdb() -> usvg::fontdb::Database {
    let mut db = usvg::fontdb::Database::new();
    db.load_system_fonts();
    db.load_font_data(SOURCE_HAN_SANS_SC_REGULAR.to_vec());
    db.set_sans_serif_family(mermaid_primary_font_name());
    db
}

fn rasterize_mermaid_svg_for_display(
    svg: &str,
    size: MermaidSvgSize,
    device_scale: f32,
) -> anyhow::Result<Vec<u8>> {
    let db = std::sync::Arc::new(configure_mermaid_fontdb());
    let options = usvg::Options {
        fontdb: db,
        font_family: mermaid_primary_font_name().to_string(),
        ..Default::default()
    };
    let tree = usvg::Tree::from_str(svg, &options)
        .map_err(|err| anyhow!("failed to parse Mermaid SVG for rasterization: {err}"))?;
    let intrinsic = tree.size();
    let raster_scale = device_scale.clamp(1.0, 3.0);
    let target_width = (size.width * raster_scale).ceil().max(1.0) as u32;
    let target_height = (size.height * raster_scale).ceil().max(1.0) as u32;
    let scale = (target_width as f32 / intrinsic.width())
        .min(target_height as f32 / intrinsic.height())
        .max(0.1);
    let mut pixmap = resvg::tiny_skia::Pixmap::new(target_width, target_height)
        .ok_or_else(|| anyhow!("failed to allocate Mermaid raster buffer"))?;
    pixmap.fill(resvg::tiny_skia::Color::WHITE);
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    pixmap
        .encode_png()
        .map_err(|err| anyhow!("failed to encode Mermaid PNG: {err}"))
}

#[cfg(test)]
fn png_dimensions(png: &[u8]) -> Option<(f32, f32)> {
    const PNG_SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";
    if png.len() < 24 || png.get(0..8)? != PNG_SIGNATURE {
        return None;
    }
    let width = u32::from_be_bytes(png.get(16..20)?.try_into().ok()?);
    let height = u32::from_be_bytes(png.get(20..24)?.try_into().ok()?);
    Some((width as f32, height as f32))
}

/// Replaces emoji that resvg cannot rasterize with plain-text symbols from the main font.
fn substitute_unrenderable_emoji_in_svg(svg: &str) -> String {
    svg.replace('✅', " ✓")
        .replace('☑', " ✓")
        .replace('❌', " ✗")
        .replace('⏳', " …")
}

/// Stable cache key for Mermaid content.
pub(crate) fn mermaid_cache_key(source: &str) -> String {
    let mut hasher = DefaultHasher::new();
    "mermaid-svg-v8".hash(&mut hasher);
    source.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

/// Stable cache key for editor display PNG content, layout scale, and device scale.
pub(crate) fn mermaid_display_cache_key(source: &str, scale: f32, device_scale: f32) -> String {
    let mut hasher = DefaultHasher::new();
    mermaid_cache_key(source).hash(&mut hasher);
    scale.max(0.1).to_bits().hash(&mut hasher);
    device_scale.clamp(1.0, 3.0).to_bits().hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

/// Counts diagram lines that materially contribute to rendered complexity.
#[cfg(test)]
pub(crate) fn semantic_mermaid_line_count(source: &str) -> usize {
    let mut in_frontmatter = false;
    source
        .lines()
        .filter(|line| {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                return false;
            }
            if trimmed == "---" {
                in_frontmatter = !in_frontmatter;
                return false;
            }
            !(in_frontmatter || trimmed.starts_with("%%"))
        })
        .count()
}

/// Display scale used by the editor for rendered Mermaid diagrams.
pub(crate) fn mermaid_display_scale(
    _source: &str,
    intrinsic_width: f32,
    intrinsic_height: f32,
    available_width: f32,
    available_height: f32,
) -> f32 {
    let intrinsic_width = intrinsic_width.max(1.0);
    let intrinsic_height = intrinsic_height.max(1.0);
    let available_width = available_width.max(1.0);
    let available_height = available_height.max(1.0);
    let width_scale = available_width / intrinsic_width;
    let height_scale = available_height / intrinsic_height;
    width_scale
        .min(height_scale)
        .clamp(MERMAID_MIN_DISPLAY_SCALE, MERMAID_MAX_DISPLAY_SCALE)
}

fn strip_fence_indent(line: &str) -> Option<&str> {
    let indent = line.bytes().take_while(|byte| *byte == b' ').count();
    (indent <= 3).then_some(&line[indent..])
}

/// Rewrites the root SVG element so GPUI sees the intended intrinsic size.
pub(crate) fn scale_mermaid_svg_for_display(
    svg: &str,
    scale: f32,
) -> anyhow::Result<(String, MermaidSvgSize)> {
    let scale = scale.max(0.1);
    let (start, end) = svg_root_tag_range(svg)?;
    let root_tag = &svg[start..end];
    let base_size = svg_root_size(root_tag)?;
    let size = MermaidSvgSize {
        width: (base_size.width * scale).max(1.0),
        height: (base_size.height * scale).max(1.0),
    };
    let rewritten_root = rewrite_svg_root_tag(root_tag, size)?;
    let mut rewritten = String::with_capacity(svg.len() + 48);
    rewritten.push_str(&svg[..start]);
    rewritten.push_str(&rewritten_root);
    rewritten.push_str(&svg[end..]);
    Ok((rewritten, size))
}

fn mermaid_svg_intrinsic_size(svg: &str) -> anyhow::Result<MermaidSvgSize> {
    let (start, end) = svg_root_tag_range(svg)?;
    svg_root_size(&svg[start..end])
}

fn svg_root_tag_range(svg: &str) -> anyhow::Result<(usize, usize)> {
    let start = svg
        .find("<svg")
        .ok_or_else(|| anyhow!("Mermaid renderer output did not contain an SVG root"))?;
    let bytes = svg.as_bytes();
    let mut quote = None;
    let mut index = start;
    while index < bytes.len() {
        let byte = bytes[index];
        if let Some(active_quote) = quote {
            if byte == active_quote {
                quote = None;
            }
        } else if byte == b'"' || byte == b'\'' {
            quote = Some(byte);
        } else if byte == b'>' {
            return Ok((start, index + 1));
        }
        index += 1;
    }
    Err(anyhow!(
        "Mermaid renderer output had an unterminated SVG root tag"
    ))
}

fn svg_root_size(root_tag: &str) -> anyhow::Result<MermaidSvgSize> {
    if let Some(view_box) = svg_root_attr(root_tag, "viewBox")
        && let Some(size) = parse_view_box_size(&view_box)
    {
        return Ok(size);
    }

    let width = svg_root_attr(root_tag, "width")
        .and_then(|value| parse_svg_length(&value))
        .ok_or_else(|| anyhow!("Mermaid SVG root did not expose a usable width"))?;
    let height = svg_root_attr(root_tag, "height")
        .and_then(|value| parse_svg_length(&value))
        .ok_or_else(|| anyhow!("Mermaid SVG root did not expose a usable height"))?;
    Ok(MermaidSvgSize { width, height })
}

fn parse_view_box_size(view_box: &str) -> Option<MermaidSvgSize> {
    let values = view_box
        .split(|ch: char| ch.is_ascii_whitespace() || ch == ',')
        .filter(|part| !part.is_empty())
        .map(str::parse::<f32>)
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    (values.len() == 4 && values[2].is_finite() && values[3].is_finite()).then_some(
        MermaidSvgSize {
            width: values[2].max(1.0),
            height: values[3].max(1.0),
        },
    )
}

fn parse_svg_length(value: &str) -> Option<f32> {
    let value = value.trim();
    let end = value
        .char_indices()
        .take_while(|(_, ch)| ch.is_ascii_digit() || matches!(ch, '.' | '-' | '+' | 'e' | 'E'))
        .map(|(index, ch)| index + ch.len_utf8())
        .last()?;
    let parsed = value[..end].parse::<f32>().ok()?;
    (parsed.is_finite() && parsed > 0.0).then_some(parsed)
}

fn svg_root_attr(root_tag: &str, attr_name: &str) -> Option<String> {
    svg_root_attrs(root_tag)
        .into_iter()
        .find(|attr| attr.name.eq_ignore_ascii_case(attr_name))
        .and_then(|attr| attr.value)
}

fn rewrite_svg_root_tag(root_tag: &str, size: MermaidSvgSize) -> anyhow::Result<String> {
    let attrs = svg_root_attrs(root_tag)
        .into_iter()
        .filter(|attr| {
            !["width", "height", "style"]
                .iter()
                .any(|name| attr.name.eq_ignore_ascii_case(name))
        })
        .map(|attr| attr.raw)
        .collect::<Vec<_>>();

    let mut rewritten = String::from("<svg");
    for attr in attrs {
        rewritten.push(' ');
        rewritten.push_str(attr.trim());
    }
    rewritten.push_str(&format!(
        " width=\"{:.3}\" height=\"{:.3}\">",
        size.width, size.height
    ));
    Ok(rewritten)
}

#[derive(Debug)]
struct SvgRootAttr {
    name: String,
    value: Option<String>,
    raw: String,
}

fn svg_root_attrs(root_tag: &str) -> Vec<SvgRootAttr> {
    let Some(mut index) = root_tag.find("<svg").map(|index| index + "<svg".len()) else {
        return Vec::new();
    };
    let end = root_tag.rfind('>').unwrap_or(root_tag.len());
    let bytes = root_tag.as_bytes();
    let mut attrs = Vec::new();

    while index < end {
        while index < end && bytes[index].is_ascii_whitespace() {
            index += 1;
        }
        if index >= end || bytes[index] == b'/' {
            break;
        }

        let attr_start = index;
        while index < end
            && !bytes[index].is_ascii_whitespace()
            && bytes[index] != b'='
            && bytes[index] != b'/'
        {
            index += 1;
        }
        let name = root_tag[attr_start..index].to_string();
        if name.is_empty() {
            break;
        }

        while index < end && bytes[index].is_ascii_whitespace() {
            index += 1;
        }

        let mut value = None;
        if index < end && bytes[index] == b'=' {
            index += 1;
            while index < end && bytes[index].is_ascii_whitespace() {
                index += 1;
            }

            if index < end && (bytes[index] == b'"' || bytes[index] == b'\'') {
                let quote = bytes[index];
                index += 1;
                let value_start = index;
                while index < end && bytes[index] != quote {
                    index += 1;
                }
                value = Some(root_tag[value_start..index].to_string());
                if index < end {
                    index += 1;
                }
            } else {
                let value_start = index;
                while index < end && !bytes[index].is_ascii_whitespace() && bytes[index] != b'/' {
                    index += 1;
                }
                value = Some(root_tag[value_start..index].to_string());
            }
        }

        let raw = root_tag[attr_start..index].trim().to_string();
        attrs.push(SvgRootAttr { name, value, raw });
    }

    attrs
}

fn mermaid_cache_dir() -> anyhow::Result<PathBuf> {
    let root = ProjectDirs::from("com", "manyougz", MARKMAN_PROJECT_QUALIFIER)
        .map(|dirs| dirs.cache_dir().to_path_buf())
        .unwrap_or_else(|| std::env::temp_dir().join(MARKMAN_PROJECT_QUALIFIER));
    let dir = root.join("mermaid-svg");
    fs::create_dir_all(&dir)
        .with_context(|| format!("failed to create Mermaid SVG cache '{}'", dir.display()))?;
    Ok(dir)
}

fn mermaid_base_cache_path(key: &str) -> anyhow::Result<PathBuf> {
    mermaid_cache_file_path("base", key)
}

fn mermaid_display_cache_path(key: &str) -> anyhow::Result<PathBuf> {
    mermaid_cache_file_path("display", key)
}

fn mermaid_cache_file_path(kind: &str, key: &str) -> anyhow::Result<PathBuf> {
    let dir = mermaid_cache_dir()?.join(kind);
    fs::create_dir_all(&dir).with_context(|| {
        format!(
            "failed to create Mermaid {kind} cache '{}'",
            dir.display()
        )
    })?;
    let extension = if kind == "display" { "png" } else { "svg" };
    Ok(dir.join(format!("{key}.{extension}")))
}

fn looks_like_supported_mermaid_source(source: &str) -> bool {
    let mut in_frontmatter = false;
    for line in source.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed == "---" {
            in_frontmatter = !in_frontmatter;
            continue;
        }
        if in_frontmatter || trimmed.starts_with("%%") {
            continue;
        }

        let lower = trimmed.to_ascii_lowercase();
        return [
            "sequencediagram",
            "classdiagram",
            "statediagram",
            "erdiagram",
            "pie",
            "mindmap",
            "journey",
            "timeline",
            "gantt",
            "requirementdiagram",
            "gitgraph",
            "c4",
            "sankey",
            "quadrantchart",
            "zenuml",
            "block",
            "packet",
            "kanban",
            "architecture",
            "radar",
            "treemap",
            "xychart",
            "flowchart",
            "graph",
        ]
        .iter()
        .any(|prefix| lower.starts_with(prefix));
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};

    static TEST_RENDERER_CALLS: OnceLock<Mutex<HashMap<String, usize>>> = OnceLock::new();

    fn test_renderer(source: &str) -> anyhow::Result<String> {
        let calls = TEST_RENDERER_CALLS.get_or_init(|| Mutex::new(HashMap::new()));
        let mut calls = calls.lock().expect("renderer calls mutex poisoned");
        *calls.entry(source.to_string()).or_default() += 1;
        drop(calls);
        render_mermaid_raw(source)
    }

    fn reset_renderer_calls(source: &str) {
        let calls = TEST_RENDERER_CALLS.get_or_init(|| Mutex::new(HashMap::new()));
        calls
            .lock()
            .expect("renderer calls mutex poisoned")
            .remove(source);
    }

    fn renderer_calls(source: &str) -> usize {
        let calls = TEST_RENDERER_CALLS.get_or_init(|| Mutex::new(HashMap::new()));
        calls
            .lock()
            .expect("renderer calls mutex poisoned")
            .get(source)
            .copied()
            .unwrap_or(0)
    }

    fn unique_mermaid_source(label: &str) -> MermaidSource {
        MermaidSource {
            raw: format!("```mermaid\nflowchart LR\nA[{}] --> B\n```", label),
            body: format!("flowchart LR\nA[{}] --> B", label),
            info: "mermaid".to_string(),
        }
    }

    fn remove_cache_file(path: &Path) {
        if path.exists() {
            fs::remove_file(path).expect("remove cache file");
        }
    }

    #[test]
    fn detects_mermaid_info_string() {
        assert!(is_mermaid_info_string(Some("mermaid")));
        assert!(is_mermaid_info_string(Some("MMD title")));
        assert!(!is_mermaid_info_string(Some("rust")));
        assert!(!is_mermaid_info_string(None));
    }

    #[test]
    fn parses_backtick_mermaid_fence() {
        let parsed = parse_mermaid_fence_source("```mermaid\nflowchart LR\nA --> B\n```")
            .expect("mermaid fence");
        assert_eq!(parsed.info, "mermaid");
        assert_eq!(parsed.body, "flowchart LR\nA --> B");
    }

    #[test]
    fn parses_tilde_mmd_fence() {
        let parsed = parse_mermaid_fence_source("~~~MMD\nflowchart LR\nA --> B\n~~~")
            .expect("mermaid fence");
        assert_eq!(parsed.info, "MMD");
        assert_eq!(parsed.body, "flowchart LR\nA --> B");
    }

    #[test]
    fn rejects_unclosed_mermaid_fence() {
        assert!(parse_mermaid_fence_source("```mermaid\nflowchart LR").is_none());
    }

    #[test]
    fn cache_key_changes_with_source() {
        assert_ne!(
            mermaid_cache_key("flowchart LR\nA --> B"),
            mermaid_cache_key("flowchart LR\nA --> C")
        );
    }

    #[test]
    fn semantic_line_count_ignores_comments_blank_lines_and_frontmatter() {
        let source = "---\ntitle: Demo\n---\nflowchart LR\n%% comment\n\nA --> B\nB --> C";
        assert_eq!(semantic_mermaid_line_count(source), 3);
    }

    #[test]
    fn display_scale_fills_width_when_height_allows() {
        let source = "flowchart LR\nA --> B\nB --> C";
        let scale = mermaid_display_scale(source, 240.0, 120.0, 720.0, 960.0);

        assert_eq!(scale, 3.0);
        assert_eq!(240.0 * scale, 720.0);
        assert!(120.0 * scale <= 960.0);
    }

    #[test]
    fn display_scale_is_limited_by_available_height() {
        let source = "flowchart TD\nA --> B";
        let scale = mermaid_display_scale(source, 240.0, 900.0, 720.0, 600.0);

        assert_eq!(scale, 600.0 / 900.0);
        assert!(240.0 * scale <= 720.0);
        assert_eq!(900.0 * scale, 600.0);
    }

    #[test]
    fn display_cache_key_changes_with_scale() {
        let source = "flowchart LR\nA --> B";
        assert_ne!(
            mermaid_display_cache_key(source, 1.0, 2.0),
            mermaid_display_cache_key(source, 2.0, 2.0)
        );
    }

    #[test]
    fn display_svg_scaling_rewrites_root_dimensions() {
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="50" viewBox="0 0 100 50"><rect width="100" height="50"/></svg>"#;
        let (scaled, size) = scale_mermaid_svg_for_display(svg, 2.0).expect("scaled svg");

        assert_eq!(
            size,
            MermaidSvgSize {
                width: 200.0,
                height: 100.0
            }
        );
        assert!(scaled.contains(r#"width="200.000""#));
        assert!(scaled.contains(r#"height="100.000""#));
        assert!(scaled.contains(r#"viewBox="0 0 100 50""#));
    }

    #[test]
    fn display_svg_scaling_removes_responsive_root_attrs() {
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="100%" style="max-width: 240px; aspect-ratio: 2;" viewBox="0 0 120 60"><text>x</text></svg>"#;
        let (scaled, size) = scale_mermaid_svg_for_display(svg, 1.5).expect("scaled svg");

        assert_eq!(
            size,
            MermaidSvgSize {
                width: 180.0,
                height: 90.0
            }
        );
        let root = &scaled[..scaled.find('>').unwrap()];
        assert!(root.contains(r#"width="180.000""#));
        assert!(root.contains(r#"height="90.000""#));
        assert!(!root.contains("100%"));
        assert!(!root.contains("max-width"));
        assert!(!root.contains("style="));
    }

    #[test]
    fn renders_basic_flowchart_svg() {
        let svg = render_mermaid_to_svg("flowchart LR\nA --> B").expect("svg");
        assert!(svg.contains("<svg"));
        assert!(svg.contains("</svg>"));
    }

    #[test]
    fn patches_mermaid_svg_font_stack_for_resvg() {
        let input = r#"<svg><style>svg{font-family:Inter,ui-sans-serif,sans-serif;}</style><text font-family="Inter,ui-sans-serif,sans-serif">阶段1</text></svg>"#;
        let patched = inject_mermaid_svg_font_fallbacks(input);
        assert!(patched.contains(mermaid_primary_font_name()));
        assert!(!patched.contains("Inter,ui-sans-serif"));
        assert!(!patched.contains("font-family=\"\""));
    }

    #[test]
    fn mermaid_fontdb_loads_embedded_source_han_sans() {
        let db = configure_mermaid_fontdb();
        let query = usvg::fontdb::Query {
            families: &[usvg::fontdb::Family::Name(mermaid_primary_font_name())],
            weight: usvg::fontdb::Weight::NORMAL,
            stretch: usvg::fontdb::Stretch::Normal,
            style: usvg::fontdb::Style::Normal,
        };
        let id = db.query(&query).expect("Source Han Sans SC should resolve");
        let face = db.face(id).expect("font face");
        assert!(
            face
                .families
                .iter()
                .any(|(name, _)| name.contains("Source Han Sans") || name.contains("思源黑体")),
            "expected embedded Source Han Sans SC, got {:?}",
            face.families
        );
    }

    #[test]
    fn substitutes_unrenderable_emoji_in_svg_text() {
        let patched = substitute_unrenderable_emoji_in_svg("阶段1 ✅");
        assert_eq!(patched, "阶段1  ✓");
    }

    #[test]
    fn renders_flowchart_node_with_checkmark_emoji() {
        let svg = render_mermaid_to_svg("flowchart TD\nA[阶段1: 调研立项 ✅] --> B[市场调研]")
            .expect("svg");
        assert!(
            svg.contains("阶段1"),
            "expected Chinese label in SVG, got snippet: {}",
            &svg[..svg.len().min(500)]
        );
        assert!(
            svg.contains('✓'),
            "expected checkmark substitute in patched SVG output"
        );
        assert!(
            !svg.contains('✅'),
            "raw emoji should be replaced before caching SVG"
        );
        assert!(
            !svg.contains(">?</text>"),
            "unexpected placeholder question mark in node label"
        );
    }

    #[test]
    fn resvg_renders_mermaid_node_with_checkmark_emoji() {
        let svg = render_mermaid_to_svg("flowchart TD\nA[阶段1: 调研立项 ✅] --> B[市场调研]")
            .expect("svg");
        let intrinsic = mermaid_svg_intrinsic_size(&svg).expect("intrinsic size");
        let png = rasterize_mermaid_svg_for_display(&svg, intrinsic, 2.0).expect("png");
        assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"));
        let (width, height) = png_dimensions(&png).expect("png dimensions");
        assert!(width > intrinsic.width);
        assert!(height > intrinsic.height);
    }

    #[test]
    fn display_render_uses_scaled_intrinsic_size() {
        let source =
            parse_mermaid_fence_source("```mermaid\nflowchart LR\nA --> B\n```").expect("source");
        let rendered = render_mermaid_svg_for_display(&source, 720.0, 960.0, 2.0).expect("display png");

        assert!(rendered.display_width > 1.0);
        assert!(rendered.display_height > 1.0);
        assert!(rendered.display_scale >= 1.0);
        assert_eq!(rendered.path.extension().and_then(|ext| ext.to_str()), Some("png"));
        assert!(
            rendered
                .svg
                .contains(&format!("width=\"{:.3}\"", rendered.display_width))
        );
        assert!(
            rendered
                .svg
                .contains(&format!("height=\"{:.3}\"", rendered.display_height))
        );
        let png = fs::read(&rendered.path).expect("read display png");
        assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"));
    }

    #[test]
    fn invalid_mermaid_returns_error() {
        assert!(render_mermaid_to_svg("not a real mermaid diagram ::::").is_err());
    }

    #[test]
    fn display_cache_hit_does_not_call_renderer_again() {
        let source = unique_mermaid_source("display-cache-hit-does-not-call-renderer-again");
        let base_key = mermaid_cache_key(&source.body);
        let base_path = mermaid_base_cache_path(&base_key).expect("base path");
        remove_cache_file(&base_path);

        reset_renderer_calls(&source.body);
        let first = render_mermaid_svg_for_display_with(&source, 720.0, 960.0, 2.0, test_renderer)
            .expect("first render");
        assert_eq!(renderer_calls(&source.body), 1);
        let display_path = first.path.clone();

        let second = render_mermaid_svg_for_display_with(&source, 720.0, 960.0, 2.0, test_renderer)
            .expect("cached render");
        assert_eq!(renderer_calls(&source.body), 1);
        assert_eq!(second.path, display_path);
        assert!(
            (second.display_width - first.display_width).abs() < 0.01,
            "display width mismatch: {} vs {}",
            second.display_width,
            first.display_width
        );
        assert!(
            (second.display_height - first.display_height).abs() < 0.01,
            "display height mismatch: {} vs {}",
            second.display_height,
            first.display_height
        );

        remove_cache_file(&display_path);
        remove_cache_file(&base_path);
    }

    #[test]
    fn display_cache_miss_reuses_base_cache() {
        let source = unique_mermaid_source("display-cache-miss-reuses-base-cache");
        let base_key = mermaid_cache_key(&source.body);
        let base_path = mermaid_base_cache_path(&base_key).expect("base path");
        remove_cache_file(&base_path);

        reset_renderer_calls(&source.body);
        let first = render_mermaid_svg_for_display_with(&source, 720.0, 960.0, 2.0, test_renderer)
            .expect("first render");
        assert_eq!(renderer_calls(&source.body), 1);
        remove_cache_file(&first.path);

        let second = render_mermaid_svg_for_display_with(&source, 720.0, 960.0, 2.0, test_renderer)
            .expect("display rebuild");
        assert_eq!(renderer_calls(&source.body), 1);
        assert!(second.path.exists());
        assert_eq!(second.display_width, first.display_width);
        assert_eq!(second.display_height, first.display_height);

        remove_cache_file(&second.path);
        remove_cache_file(&base_path);
    }

    #[test]
    fn display_scale_change_reuses_base_cache_with_new_display_file() {
        let source = unique_mermaid_source("display-scale-change-reuses-base-cache");
        let base_key = mermaid_cache_key(&source.body);
        let base_path = mermaid_base_cache_path(&base_key).expect("base path");
        remove_cache_file(&base_path);

        reset_renderer_calls(&source.body);
        let narrow = render_mermaid_svg_for_display_with(&source, 240.0, 320.0, 2.0, test_renderer)
            .expect("narrow render");
        assert_eq!(renderer_calls(&source.body), 1);

        let wide = render_mermaid_svg_for_display_with(&source, 900.0, 1200.0, 2.0, test_renderer)
            .expect("wide render");
        assert_eq!(renderer_calls(&source.body), 1);
        assert!(wide.path.exists());

        remove_cache_file(&narrow.path);
        remove_cache_file(&wide.path);
        remove_cache_file(&base_path);
    }
}
