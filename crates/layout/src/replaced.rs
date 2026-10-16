//! Sizing of replaced elements (CSS 2.1 §10.3.2, §10.6.2; CSS Images 3 §5.2,
//! the "default sizing algorithm" with intrinsic dimensions and ratio).

use rustlite_paint::ImageData;
use rustlite_style::{ComputedLength, ComputedLengthAuto, ComputedStyle};

fn resolve(l: ComputedLengthAuto, basis: Option<f32>) -> Option<f32> {
    match l {
        ComputedLengthAuto::Auto => None,
        ComputedLengthAuto::Length(ComputedLength::Px(v)) => Some(v),
        ComputedLengthAuto::Length(ComputedLength::Percentage(p)) => basis.map(|b| b * p / 100.0),
    }
}

/// Content-box size of a replaced element. Percentage heights are treated as
/// `auto` (the containing block height is not definite in normal flow).
pub(crate) fn content_size(
    style: &ComputedStyle,
    image: Option<&ImageData>,
    cb_width: f32,
) -> (f32, f32) {
    let width = resolve(style.width, Some(cb_width));
    let height = resolve(style.height, None);
    let intrinsic = image.map(|i| (i.width as f32, i.height as f32));
    let (mut w, mut h) = match (width, height, intrinsic) {
        (Some(w), Some(h), _) => (w, h),
        (Some(w), None, Some((iw, ih))) if iw > 0.0 => (w, w * ih / iw),
        (None, Some(h), Some((iw, ih))) if ih > 0.0 => (h * iw / ih, h),
        (None, None, Some(size)) => size,
        // No intrinsic size (missing or broken image): unspecified sides are 0.
        (w, h, _) => (w.unwrap_or(0.0), h.unwrap_or(0.0)),
    };

    // Min/max constraints (CSS 2.1 §10.4 and its table for replaced
    // elements, simplified): when a dimension is not specified, the
    // intrinsic ratio is kept while clamping the other.
    let ratio = intrinsic
        .filter(|(iw, ih)| *iw > 0.0 && *ih > 0.0)
        .map(|(iw, ih)| iw / ih);
    let keep_ratio = ratio.filter(|_| width.is_none() || height.is_none());
    let max_w = style.max_width.map(|m| match m {
        ComputedLength::Px(v) => v,
        ComputedLength::Percentage(p) => cb_width * p / 100.0,
    });
    let max_h = style.max_height.and_then(|m| match m {
        ComputedLength::Px(v) => Some(v),
        ComputedLength::Percentage(_) => None,
    });
    let min_w = resolve(style.min_width, Some(cb_width)).unwrap_or(0.0);
    let min_h = resolve(style.min_height, None).unwrap_or(0.0);
    let clamp_w = |w: f32| max_w.map_or(w, |m| w.min(m)).max(min_w);
    let clamp_h = |h: f32| max_h.map_or(h, |m| h.min(m)).max(min_h);
    let new_w = clamp_w(w);
    if new_w != w {
        if let Some(r) = keep_ratio {
            h = new_w / r;
        }
        w = new_w;
    }
    let new_h = clamp_h(h);
    if new_h != h {
        if let Some(r) = keep_ratio {
            w = new_h * r;
        }
        h = new_h;
    }
    (w, h)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustlite_style::ComputedLengthAuto as A;

    fn style(width: A, height: A) -> ComputedStyle {
        ComputedStyle {
            width,
            height,
            ..ComputedStyle::initial()
        }
    }

    fn px(v: f32) -> A {
        A::Length(ComputedLength::Px(v))
    }

    fn img(w: u32, h: u32) -> ImageData {
        ImageData::new(w, h, vec![0; (w * h * 4) as usize]).unwrap()
    }

    #[test]
    fn intrinsic_size_and_ratio() {
        let i = img(40, 20);
        assert_eq!(
            content_size(&style(A::Auto, A::Auto), Some(&i), 500.0),
            (40.0, 20.0)
        );
        assert_eq!(
            content_size(&style(px(100.0), A::Auto), Some(&i), 500.0),
            (100.0, 50.0)
        );
        assert_eq!(
            content_size(&style(A::Auto, px(10.0)), Some(&i), 500.0),
            (20.0, 10.0)
        );
        assert_eq!(
            content_size(&style(px(7.0), px(9.0)), Some(&i), 500.0),
            (7.0, 9.0)
        );
        let half = A::Length(ComputedLength::Percentage(50.0));
        assert_eq!(
            content_size(&style(half, A::Auto), Some(&i), 500.0),
            (250.0, 125.0)
        );
    }

    #[test]
    fn missing_image_uses_specified_sides_only() {
        assert_eq!(
            content_size(&style(A::Auto, A::Auto), None, 500.0),
            (0.0, 0.0)
        );
        assert_eq!(
            content_size(&style(px(30.0), A::Auto), None, 500.0),
            (30.0, 0.0)
        );
    }

    #[test]
    fn max_width_keeps_ratio_when_height_is_auto() {
        let i = img(400, 200);
        let s = ComputedStyle {
            max_width: Some(ComputedLength::Percentage(100.0)),
            ..ComputedStyle::initial()
        };
        assert_eq!(content_size(&s, Some(&i), 100.0), (100.0, 50.0));
        // Explicit sizes clamp independently.
        let s = ComputedStyle {
            width: px(300.0),
            height: px(30.0),
            max_width: Some(ComputedLength::Px(200.0)),
            ..ComputedStyle::initial()
        };
        assert_eq!(content_size(&s, Some(&i), 1000.0), (200.0, 30.0));
    }
}
