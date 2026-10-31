//! Mermaid diagrams in docs, drawn in the app: the diagram's text laid out and drawn to SVG by
//! `mermaid-rs-renderer` (pure Rust, no browser), in a dark theme of the app's own, shown as an
//! image at its own size (narrower when the doc is). Each diagram is drawn once and kept, by its
//! text; one that will not draw shows its text and why.

use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex};

use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use mermaid_rs_renderer::{RenderOptions, Theme, render_with_options};

use super::theme::*;

/// A diagram drawn: its image and its size; or why it could not be.
type Drawn = Result<(Arc<Image>, f32, f32), String>;

static DRAWN: LazyLock<Mutex<HashMap<String, Drawn>>> = LazyLock::new(Mutex::default);

/// The app's dark, for diagrams: its surfaces, hairlines and text.
fn theme() -> Theme {
    let mut t = Theme::dark();
    t.font_family = "-apple-system, 'SF Pro Text', 'Helvetica Neue', sans-serif".into();
    t.background = "transparent".into();
    t.primary_color = "#18191a".into();
    t.primary_text_color = "#d0d6e0".into();
    t.primary_border_color = "#34343a".into();
    t.line_color = "#8a8f98".into();
    t.secondary_color = "#141516".into();
    t.tertiary_color = "#0f1011".into();
    t.edge_label_background = "#0f1011".into();
    t.cluster_background = "#0f1011".into();
    t.cluster_border = "#23252a".into();
    t.text_color = "#d0d6e0".into();
    t
}

/// `source` drawn, once.
fn drawn(source: &str) -> Drawn {
    if let Ok(cache) = DRAWN.lock()
        && let Some(d) = cache.get(source)
    {
        return d.clone();
    }
    let options = RenderOptions {
        theme: theme(),
        ..RenderOptions::default()
    };
    let d = render_with_options(source, options)
        .map_err(|e| e.to_string())
        .and_then(|svg| {
            let (w, h) = size_of(&svg).ok_or_else(|| "the diagram has no size".to_owned())?;
            Ok((
                Arc::new(Image::from_bytes(ImageFormat::Svg, svg.into_bytes())),
                w,
                h,
            ))
        });
    if let Ok(mut cache) = DRAWN.lock() {
        cache.insert(source.to_owned(), d.clone());
    }
    d
}

/// The size `source` draws at, or why it will not draw.
pub fn size(source: &str) -> Result<(f32, f32), String> {
    drawn(source).map(|(_, w, h)| (w, h))
}

/// An SVG's size, from its `viewBox`, else its `width` and `height`.
pub fn size_of(svg: &str) -> Option<(f32, f32)> {
    let attr = |name: &str| -> Option<&str> {
        let at = svg.find(&format!("{name}=\""))? + name.len() + 2;
        Some(&svg[at..at + svg[at..].find('"')?])
    };
    if let Some(vb) = attr("viewBox") {
        let n: Vec<f32> = vb
            .split(|c: char| c.is_whitespace() || c == ',')
            .filter_map(|s| s.parse().ok())
            .collect();
        if let [_, _, w, h] = n[..] {
            return Some((w, h));
        }
    }
    let num = |s: &str| s.trim_end_matches("px").parse::<f32>().ok();
    Some((num(attr("width")?)?, num(attr("height")?)?))
}

/// A diagram, at most `max_w` wide.
pub fn mermaid(source: &str, max_w: f32) -> AnyElement {
    match drawn(source) {
        Ok((image, w, h)) => {
            let scale = (max_w / w).min(1.);
            div()
                .w_full()
                .py_4()
                .flex()
                .justify_center()
                .rounded_lg()
                .bg(color(SURFACE))
                .border_1()
                .border_color(color(BORDER))
                .child(img(image).w(px(w * scale)).h(px(h * scale)))
                .into_any_element()
        }
        Err(why) => div()
            .w_full()
            .p_3()
            .flex()
            .flex_col()
            .gap_2()
            .rounded_lg()
            .bg(color(SURFACE))
            .border_1()
            .border_color(color(BORDER))
            .text_size(px(13.))
            .child(
                div()
                    .text_color(color(RED))
                    .child(format!("This diagram will not draw: {why}")),
            )
            .when(!source.is_empty(), |d| {
                d.child(
                    div()
                        .text_color(color(TEXT_3))
                        .whitespace_normal()
                        .child(source.to_owned()),
                )
            })
            .into_any_element(),
    }
}
