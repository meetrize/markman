//! Mermaid flowchart display styles (theme colors and edge routing appearance).

use mermaid_rs_renderer::{RenderOptions, Theme};

use crate::config::MermaidDisplayStyle;

pub(crate) fn mermaid_render_options(style: MermaidDisplayStyle, font_family: &str) -> RenderOptions {
    let mut options = RenderOptions::modern();
    options.theme = theme_for_style(style);
    options.theme.font_family = font_family.to_string();
    options.theme.font_size = 16.0;
    match style {
        MermaidDisplayStyle::Default | MermaidDisplayStyle::Beautified => {
            options.layout.flowchart.routing.enable_grid_router = true;
            options.layout.flowchart.routing.snap_ports_to_grid = true;
        }
    }
    options
}

fn theme_for_style(style: MermaidDisplayStyle) -> Theme {
    let mut theme = Theme::modern();
    match style {
        MermaidDisplayStyle::Default => {
            theme.primary_color = "#EFEFFF".into();
            theme.primary_border_color = "#A090E0".into();
            theme.primary_text_color = "#000000".into();
            theme.line_color = "#000000".into();
            theme.secondary_color = "#E8E4FF".into();
            theme.tertiary_color = "#F5F3FF".into();
            theme.edge_label_background = "#FFFFFF".into();
            theme.cluster_background = "#F5F3FF".into();
            theme.cluster_border = "#C4B5E8".into();
            theme.background = "#FFFFFF".into();
        }
        MermaidDisplayStyle::Beautified => {
            theme.primary_color = "#E8F0FA".into();
            theme.primary_border_color = "#7BA3C9".into();
            theme.primary_text_color = "#000000".into();
            theme.line_color = "#000000".into();
            theme.secondary_color = "#DCE8F5".into();
            theme.tertiary_color = "#F0F5FA".into();
            theme.edge_label_background = "#FFFFFF".into();
            theme.cluster_background = "#F0F5FA".into();
            theme.cluster_border = "#B8D0E8".into();
            theme.background = "#FFFFFF".into();
        }
    }
    theme
}

/// Applies post-render SVG styling for nodes and edge paths.
pub(crate) fn apply_mermaid_display_style(svg: &str, style: MermaidDisplayStyle) -> String {
    let mut out = restyle_flowchart_nodes(svg, style);
    out = restyle_edge_paths(&out, style);
    out
}

fn restyle_flowchart_nodes(svg: &str, style: MermaidDisplayStyle) -> String {
    match style {
        MermaidDisplayStyle::Default => svg.to_string(),
        MermaidDisplayStyle::Beautified => svg
            .replace(" rx=\"3\" ry=\"3\"", " rx=\"8\" ry=\"8\"")
            .replace(" rx=\"2\" ry=\"2\"", " rx=\"8\" ry=\"8\""),
    }
}

fn restyle_edge_paths(svg: &str, style: MermaidDisplayStyle) -> String {
    const PATH_OPEN: &str = "<path";
    const EDGE_MARKER: &str = "class=\"edgePath\"";
    let mut out = String::with_capacity(svg.len());
    let mut index = 0;
    while let Some(rel) = svg[index..].find(PATH_OPEN) {
        let path_start = index + rel;
        out.push_str(&svg[index..path_start]);
        let Some(close_rel) = svg[path_start..].find("/>") else {
            out.push_str(&svg[path_start..]);
            break;
        };
        let path_end = path_start + close_rel + 2;
        let tag = &svg[path_start..path_end];
        index = path_end;

        if !tag.contains(EDGE_MARKER) {
            out.push_str(tag);
            continue;
        }

        if let Some((d_value, d_start, d_end)) = extract_d_attribute(tag) {
            let transformed = transform_edge_path_d(&d_value, style);
            out.push_str(&tag[..d_start]);
            out.push_str(" d=\"");
            out.push_str(&transformed);
            out.push('"');
            out.push_str(&tag[d_end..]);
        } else {
            out.push_str(tag);
        }
    }
    out.push_str(&svg[index..]);
    out
}

fn extract_d_attribute(tag: &str) -> Option<(String, usize, usize)> {
    let marker = " d=\"";
    let attr_start = tag.find(marker)?;
    let value_start = attr_start + marker.len();
    let value_end = value_start + tag[value_start..].find('"')?;
    Some((
        tag[value_start..value_end].to_string(),
        attr_start,
        value_end + 1,
    ))
}

fn transform_edge_path_d(d: &str, style: MermaidDisplayStyle) -> String {
    let points = parse_ml_path(d);
    if points.len() < 2 {
        return d.to_string();
    }
    match style {
        MermaidDisplayStyle::Default => orthogonal_edge_path(&points),
        MermaidDisplayStyle::Beautified => rounded_orthogonal_path(&points, 8.0),
    }
}

fn parse_ml_path(d: &str) -> Vec<(f32, f32)> {
    let mut points = Vec::new();
    for token in d.split_whitespace() {
        if token == "M" || token == "L" {
            continue;
        }
        if let Some((x, y)) = token.split_once(',') {
            if let (Ok(x), Ok(y)) = (x.parse::<f32>(), y.parse::<f32>()) {
                points.push((x, y));
            }
        }
    }
    points
}

fn segment_is_axis_aligned(from: (f32, f32), to: (f32, f32)) -> bool {
    (from.0 - to.0).abs() < 0.5 || (from.1 - to.1).abs() < 0.5
}

fn orthogonal_edge_path(points: &[(f32, f32)]) -> String {
    if points.len() < 2 {
        return String::new();
    }
    let mut d = format!("M {:.3},{:.3}", points[0].0, points[0].1);
    for window in points.windows(2) {
        let from = window[0];
        let to = window[1];
        if segment_is_axis_aligned(from, to) {
            d.push_str(&format!(" L {:.3},{:.3}", to.0, to.1));
        } else {
            let dx = to.0 - from.0;
            let dy = to.1 - from.1;
            let corner = if dx.abs() >= dy.abs() {
                (to.0, from.1)
            } else {
                (from.0, to.1)
            };
            d.push_str(&format!(" L {:.3},{:.3}", corner.0, corner.1));
            d.push_str(&format!(" L {:.3},{:.3}", to.0, to.1));
        }
    }
    d
}

fn points_to_line_path(points: &[(f32, f32)]) -> String {
    let mut d = format!("M {:.3},{:.3}", points[0].0, points[0].1);
    for point in points.iter().skip(1) {
        d.push_str(&format!(" L {:.3},{:.3}", point.0, point.1));
    }
    d
}

fn rounded_orthogonal_path(points: &[(f32, f32)], radius: f32) -> String {
    if points.len() < 2 {
        return String::new();
    }
    if points.len() == 2 {
        return points_to_line_path(points);
    }

    let mut d = format!("M {:.3},{:.3}", points[0].0, points[0].1);
    for index in 1..points.len() - 1 {
        let prev = points[index - 1];
        let curr = points[index];
        let next = points[index + 1];

        let v1 = (curr.0 - prev.0, curr.1 - prev.1);
        let v2 = (next.0 - curr.0, next.1 - curr.1);
        let len1 = (v1.0 * v1.0 + v1.1 * v1.1).sqrt();
        let len2 = (v2.0 * v2.0 + v2.1 * v2.1).sqrt();
        if len1 < 1e-3 || len2 < 1e-3 {
            continue;
        }

        let corner_radius = radius.min(len1 * 0.5).min(len2 * 0.5);
        let n1 = (v1.0 / len1, v1.1 / len1);
        let n2 = (v2.0 / len2, v2.1 / len2);
        let before = (curr.0 - n1.0 * corner_radius, curr.1 - n1.1 * corner_radius);
        let after = (curr.0 + n2.0 * corner_radius, curr.1 + n2.1 * corner_radius);

        d.push_str(&format!(" L {:.3},{:.3}", before.0, before.1));
        d.push_str(&format!(
            " Q {:.3},{:.3} {:.3},{:.3}",
            curr.0, curr.1, after.0, after.1
        ));
    }

    let last = points[points.len() - 1];
    d.push_str(&format!(" L {:.3},{:.3}", last.0, last.1));
    d
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_style_uses_sharp_orthogonal_lines() {
        let path = orthogonal_edge_path(&[(0.0, 0.0), (100.0, 80.0)]);
        assert!(path.contains('L'));
        assert!(!path.contains('Q'));
        assert!(!path.contains('C'));
        assert_eq!(path.matches('L').count(), 2, "expected L-shaped corner, got {path}");
    }

    #[test]
    fn default_style_keeps_orthogonal_segments_straight() {
        let path = orthogonal_edge_path(&[(0.0, 0.0), (100.0, 0.0), (100.0, 80.0)]);
        assert!(path.contains('L'));
        assert!(!path.contains('Q'));
        assert!(!path.contains('C'));
    }

    #[test]
    fn beautified_style_rounds_orthogonal_corners() {
        let path = rounded_orthogonal_path(&[(0.0, 0.0), (100.0, 0.0), (100.0, 80.0)], 8.0);
        assert!(path.contains('Q'), "expected rounded corner, got {path}");
    }

    #[test]
    fn theme_colors_match_default_style() {
        let theme = theme_for_style(MermaidDisplayStyle::Default);
        assert_eq!(theme.primary_color, "#EFEFFF");
        assert_eq!(theme.primary_border_color, "#A090E0");
        assert_eq!(theme.primary_text_color, "#000000");
    }

    #[test]
    fn theme_colors_match_beautified_style() {
        let theme = theme_for_style(MermaidDisplayStyle::Beautified);
        assert_eq!(theme.primary_color, "#E8F0FA");
        assert_eq!(theme.primary_border_color, "#7BA3C9");
        assert_eq!(theme.primary_text_color, "#000000");
    }
}
