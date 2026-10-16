//! Flex layout (CSS Flexbox 1 §9, simplified): main and cross axes, line
//! breaking for `flex-wrap`, flexible lengths with grow/shrink and min/max
//! clamping, `justify-content`, `align-items`/`align-self` (with stretch),
//! `align-content`, `gap`, `order` (applied by the box builder) and the
//! `*-reverse` directions.
//!
//! Item sizes: the flex base size is `flex-basis`, else the main size
//! property, else the content size (max-content width for rows; the laid-out
//! height for columns). The automatic minimum size is the min-content width
//! for rows (0 for columns). Known gaps: baseline alignment uses the first
//! line's baseline only for `align-items: baseline` in single-line rows;
//! `auto` margins on the cross axis are treated as 0.

use rustlite_style::ComputedStyle;
use rustlite_style::computed::ComputedFlexBasis;
use rustlite_style::properties::{Align, FlexDirection, FlexWrap};

use crate::block::{Forced, frame, layout_block_inner, length, length_auto, to_content};
use crate::{BlockBox, LayoutContext};

struct Item {
    /// Index into the items slice.
    index: usize,
    /// Flex base size and hypothetical main size (content box).
    base: f32,
    hypothetical: f32,
    min_main: f32,
    max_main: f32,
    /// Margin + border + padding along the main axis.
    main_extra: f32,
    /// Final main size (content box).
    main: f32,
    frozen: bool,
    /// Outer cross size after layout (margin box).
    cross_outer: f32,
}

fn margin_px(style: &ComputedStyle, i: usize, basis: f32) -> f32 {
    length_auto(style.margin[i], Some(basis)).unwrap_or(0.0)
}

fn auto_margin(style: &ComputedStyle, i: usize) -> bool {
    style.margin[i] == rustlite_style::ComputedLengthAuto::Auto
}

/// Lays out the flex `items` of a container with `style`, a content width of
/// `width` and a definite content height if `height` is `Some`. Returns the
/// container's content height.
pub(crate) fn layout_flex(
    items: &mut [BlockBox],
    style: &ComputedStyle,
    width: f32,
    height: Option<f32>,
    _cb_height: Option<f32>,
    ctx: &mut LayoutContext<'_>,
) -> f32 {
    if items.is_empty() {
        return 0.0;
    }
    let row = matches!(
        style.flex_direction,
        FlexDirection::Row | FlexDirection::RowReverse
    );
    let reverse = matches!(
        style.flex_direction,
        FlexDirection::RowReverse | FlexDirection::ColumnReverse
    );
    let wrap = style.flex_wrap != FlexWrap::Nowrap;
    let main_gap =
        if row { style.column_gap } else { style.row_gap }.map_or(0.0, |g| length(g, width));
    let cross_gap =
        if row { style.row_gap } else { style.column_gap }.map_or(0.0, |g| length(g, width));
    // The main size available to items (indefinite for auto-height columns).
    let container_main = if row { Some(width) } else { height };

    // 1. Flex base sizes and hypothetical main sizes (§9.2).
    let mut flex: Vec<Item> = Vec::with_capacity(items.len());
    for (index, item) in items.iter_mut().enumerate() {
        let s = item.style.clone();
        let (hframe, vframe) = frame(&s, width);
        let (main_frame, main_margins) = if row {
            (hframe, margin_px(&s, 1, width) + margin_px(&s, 3, width))
        } else {
            (vframe, margin_px(&s, 0, width) + margin_px(&s, 2, width))
        };
        let main_basis = container_main;
        let size_prop = if row { s.width } else { s.height };
        let basis = match s.flex_basis {
            ComputedFlexBasis::Size(sz) => to_content(&s, length_auto(sz, main_basis), main_frame)
                .or_else(|| to_content(&s, length_auto(size_prop, main_basis), main_frame)),
            ComputedFlexBasis::Content => None,
        };
        let content_size = |item: &mut BlockBox, ctx: &mut LayoutContext<'_>| {
            if row {
                crate::intrinsic::content_widths(item, ctx).1
            } else {
                let w = cross_content_width(item, width, ctx);
                let o = layout_block_inner(
                    item,
                    width,
                    None,
                    ctx,
                    false,
                    (0.0, 0.0),
                    Forced {
                        content_width: Some(w),
                        content_height: None,
                    },
                );
                o.height - vframe
            }
        };
        let base = match basis {
            Some(b) => b,
            None => content_size(item, ctx),
        };
        let (min_prop, max_prop) = if row {
            (s.min_width, s.max_width)
        } else {
            (s.min_height, s.max_height)
        };
        let max_main = to_content(
            &s,
            max_prop.map(|m| length(m, main_basis.unwrap_or(0.0))),
            main_frame,
        )
        .unwrap_or(f32::INFINITY);
        // Automatic minimum size (§4.5): the min-content size for rows, capped
        // by a specified size.
        let min_main = match to_content(&s, length_auto(min_prop, main_basis), main_frame) {
            Some(m) => m,
            None if row => {
                let min_content = crate::intrinsic::content_widths(item, ctx).0;
                let specified = to_content(&s, length_auto(size_prop, main_basis), main_frame);
                specified
                    .map_or(min_content, |sz| sz.min(min_content))
                    .min(max_main)
            }
            None => 0.0,
        };
        let hypothetical = base.min(max_main).max(min_main);
        flex.push(Item {
            index,
            base,
            hypothetical,
            min_main,
            max_main,
            main_extra: main_frame + main_margins,
            main: hypothetical,
            frozen: false,
            cross_outer: 0.0,
        });
    }

    // 2. Line breaking (§9.3).
    let mut lines: Vec<Vec<usize>> = Vec::new();
    {
        let mut line: Vec<usize> = Vec::new();
        let mut used = 0.0;
        for (i, it) in flex.iter().enumerate() {
            let outer = it.hypothetical + it.main_extra;
            let add = if line.is_empty() {
                outer
            } else {
                main_gap + outer
            };
            if wrap && !line.is_empty() && container_main.is_some_and(|m| used + add > m + 0.01) {
                lines.push(std::mem::take(&mut line));
                used = 0.0;
                line.push(i);
                used += outer;
            } else {
                line.push(i);
                used += add;
            }
        }
        lines.push(line);
    }

    // 3. Flexible lengths per line (§9.7).
    for line in &lines {
        let Some(available) = container_main else {
            for &i in line {
                flex[i].main = flex[i].hypothetical;
            }
            continue;
        };
        let gaps = main_gap * line.len().saturating_sub(1) as f32;
        let sum_hypo: f32 = line
            .iter()
            .map(|&i| flex[i].hypothetical + flex[i].main_extra)
            .sum();
        let growing = sum_hypo + gaps < available;
        for &i in line {
            let s = &items[flex[i].index].style;
            let factor = if growing { s.flex_grow } else { s.flex_shrink };
            let inflexible = factor == 0.0
                || (growing && flex[i].base > flex[i].hypothetical)
                || (!growing && flex[i].base < flex[i].hypothetical);
            flex[i].frozen = inflexible;
            flex[i].main = if inflexible {
                flex[i].hypothetical
            } else {
                flex[i].base
            };
        }
        for _ in 0..line.len() + 1 {
            let used: f32 = line
                .iter()
                .map(|&i| {
                    let it = &flex[i];
                    (if it.frozen { it.main } else { it.base }) + it.main_extra
                })
                .sum::<f32>()
                + gaps;
            let free = available - used;
            let unfrozen: Vec<usize> = line.iter().copied().filter(|&i| !flex[i].frozen).collect();
            if unfrozen.is_empty() {
                break;
            }
            let factor_sum: f32 = unfrozen
                .iter()
                .map(|&i| {
                    let s = &items[flex[i].index].style;
                    if growing {
                        s.flex_grow
                    } else {
                        s.flex_shrink * flex[i].base
                    }
                })
                .sum();
            // A factor sum below 1 distributes only that fraction (§9.7 step 4b).
            let free = if growing && factor_sum < 1.0 {
                free * factor_sum
            } else {
                free
            };
            let mut violation = 0.0;
            for &i in &unfrozen {
                let s = &items[flex[i].index].style;
                let target = if factor_sum <= 0.0 {
                    flex[i].base
                } else if growing {
                    flex[i].base + free * s.flex_grow / factor_sum.max(1.0)
                } else {
                    flex[i].base + free * (s.flex_shrink * flex[i].base) / factor_sum
                };
                let clamped = target.min(flex[i].max_main).max(flex[i].min_main);
                violation += clamped - target;
                flex[i].main = clamped;
            }
            if violation.abs() < 0.001 {
                break;
            }
            // Freeze the violators of the dominant direction (§9.7 step 4d).
            for &i in &unfrozen {
                let s = &items[flex[i].index].style;
                let target = if growing {
                    flex[i].base + free * s.flex_grow / factor_sum.max(1.0)
                } else if factor_sum > 0.0 {
                    flex[i].base + free * (s.flex_shrink * flex[i].base) / factor_sum
                } else {
                    flex[i].base
                };
                let min_violation = flex[i].main > target + 0.001;
                let max_violation = flex[i].main < target - 0.001;
                if (violation > 0.0 && min_violation) || (violation < 0.0 && max_violation) {
                    flex[i].frozen = true;
                }
            }
        }
    }

    // 4. Cross sizes: lay out each item with its main size (§9.4).
    for it in flex.iter_mut() {
        let item = &mut items[it.index];
        let s = item.style.clone();
        let (forced_w, forced_h) = if row {
            (Some(it.main), None)
        } else {
            (Some(cross_content_width(item, width, ctx)), Some(it.main))
        };
        let o = layout_block_inner(
            item,
            width,
            if row { height } else { None },
            ctx,
            false,
            (0.0, 0.0),
            Forced {
                content_width: forced_w,
                content_height: forced_h,
            },
        );
        it.cross_outer = if row {
            o.height + margin_px(&s, 0, width) + margin_px(&s, 2, width)
        } else {
            item.rect.width + margin_px(&s, 1, width) + margin_px(&s, 3, width)
        };
    }
    let mut line_cross: Vec<f32> = lines
        .iter()
        .map(|l| l.iter().map(|&i| flex[i].cross_outer).fold(0.0, f32::max))
        .collect();
    let container_cross = if row { height } else { Some(width) };
    // A single-line container's line fills a definite cross size (§9.4 step 15).
    if !wrap && let Some(c) = container_cross {
        line_cross[0] = c;
    }
    // align-content: stretch distributes leftover cross space to lines.
    let lines_total: f32 =
        line_cross.iter().sum::<f32>() + cross_gap * lines.len().saturating_sub(1) as f32;
    let mut line_offsets = vec![0.0; lines.len()];
    if wrap && let Some(c) = container_cross {
        let free = c - lines_total;
        let n = lines.len() as f32;
        let (start, between) = match style.align_content {
            Align::FlexEnd | Align::End => (free, 0.0),
            Align::Center => (free / 2.0, 0.0),
            Align::SpaceBetween if lines.len() > 1 => (0.0, free.max(0.0) / (n - 1.0)),
            Align::SpaceAround => (free.max(0.0) / n / 2.0, free.max(0.0) / n),
            Align::SpaceEvenly => (free.max(0.0) / (n + 1.0), free.max(0.0) / (n + 1.0)),
            Align::Normal | Align::Stretch if free > 0.0 => {
                for lc in &mut line_cross {
                    *lc += free / n;
                }
                (0.0, 0.0)
            }
            _ => (0.0, 0.0),
        };
        let mut pos = start;
        for (k, off) in line_offsets.iter_mut().enumerate() {
            *off = pos;
            pos += line_cross[k] + cross_gap + between;
        }
    } else {
        let mut pos = 0.0;
        for (k, off) in line_offsets.iter_mut().enumerate() {
            *off = pos;
            pos += line_cross[k] + cross_gap;
        }
    }

    // 5. Main-axis placement (justify-content, auto margins) and cross-axis
    //    alignment (align-self, stretch) per line.
    for (k, line) in lines.iter().enumerate() {
        let order: Vec<usize> = if reverse {
            line.iter().rev().copied().collect()
        } else {
            line.clone()
        };
        let main_size = container_main.unwrap_or_else(|| {
            order
                .iter()
                .map(|&i| flex[i].main + flex[i].main_extra)
                .sum::<f32>()
                + main_gap * order.len().saturating_sub(1) as f32
        });
        let used: f32 = order
            .iter()
            .map(|&i| flex[i].main + flex[i].main_extra)
            .sum::<f32>()
            + main_gap * order.len().saturating_sub(1) as f32;
        let mut free = main_size - used;
        let (start_side, end_side) = if row { (3, 1) } else { (0, 2) };
        let auto_count = order
            .iter()
            .map(|&i| {
                let s = &items[flex[i].index].style;
                usize::from(auto_margin(s, start_side)) + usize::from(auto_margin(s, end_side))
            })
            .sum::<usize>();
        let auto_share = if auto_count > 0 && free > 0.0 {
            let share = free / auto_count as f32;
            free = 0.0;
            share
        } else {
            0.0
        };
        let n = order.len() as f32;
        let (mut pos, between) = match style.justify_content {
            Align::FlexEnd | Align::End | Align::Right => (free, 0.0),
            Align::Center => (free / 2.0, 0.0),
            Align::SpaceBetween if order.len() > 1 && free > 0.0 => (0.0, free / (n - 1.0)),
            Align::SpaceAround if free > 0.0 => (free / n / 2.0, free / n),
            Align::SpaceEvenly if free > 0.0 => (free / (n + 1.0), free / (n + 1.0)),
            _ => (0.0, 0.0),
        };
        if reverse
            && !matches!(
                style.justify_content,
                Align::Center | Align::SpaceBetween | Align::SpaceAround | Align::SpaceEvenly
            )
        {
            // flex-start of a reversed axis is the far end.
            pos = main_size - used - pos;
            if matches!(
                style.justify_content,
                Align::FlexEnd | Align::End | Align::Right
            ) {
                pos = 0.0;
            }
        }
        for &i in &order {
            let item = &mut items[flex[i].index];
            let s = item.style.clone();
            let lead = if auto_margin(&s, start_side) {
                auto_share
            } else {
                0.0
            };
            let trail = if auto_margin(&s, end_side) {
                auto_share
            } else {
                0.0
            };
            let align = match s.align_self {
                Align::Auto => style.align_items,
                a => a,
            };
            let cross = line_cross[k];
            let (cross_start_margin, cross_end_margin) = if row {
                (margin_px(&s, 0, width), margin_px(&s, 2, width))
            } else {
                (margin_px(&s, 3, width), margin_px(&s, 1, width))
            };
            let cross_auto =
                if row { s.height } else { s.width } == rustlite_style::ComputedLengthAuto::Auto;
            if matches!(align, Align::Normal | Align::Stretch) && cross_auto {
                // Stretch: the margin box fills the line (§9.4 step 11).
                let (hframe, vframe) = frame(&s, width);
                let target = cross - cross_start_margin - cross_end_margin;
                let (forced_w, forced_h) = if row {
                    (Some(flex[i].main), Some((target - vframe).max(0.0)))
                } else {
                    (Some((target - hframe).max(0.0)), Some(flex[i].main))
                };
                layout_block_inner(
                    item,
                    width,
                    if row { height } else { None },
                    ctx,
                    false,
                    (0.0, 0.0),
                    Forced {
                        content_width: forced_w,
                        content_height: forced_h,
                    },
                );
                flex[i].cross_outer = cross;
            }
            let item_cross_outer = flex[i].cross_outer;
            let cross_offset = match align {
                Align::FlexEnd | Align::End | Align::SelfEnd => cross - item_cross_outer,
                Align::Center => (cross - item_cross_outer) / 2.0,
                _ => 0.0,
            };
            let main_pos = pos + lead;
            let cross_pos = line_offsets[k] + cross_offset;
            if row {
                item.rect.x = main_pos + margin_px(&s, 3, width);
                item.rect.y = cross_pos + cross_start_margin;
            } else {
                item.rect.y = main_pos + margin_px(&s, 0, width);
                item.rect.x = cross_pos + cross_start_margin;
            }
            pos += lead + flex[i].main + flex[i].main_extra + trail + main_gap + between;
        }
    }

    // Container content height.
    if row {
        let total: f32 =
            line_cross.iter().sum::<f32>() + cross_gap * lines.len().saturating_sub(1) as f32;
        height.unwrap_or(total)
    } else {
        height.unwrap_or_else(|| {
            lines
                .iter()
                .map(|l| {
                    l.iter()
                        .map(|&i| flex[i].main + flex[i].main_extra)
                        .sum::<f32>()
                        + main_gap * l.len().saturating_sub(1) as f32
                })
                .fold(0.0, f32::max)
        })
    }
}

/// The content width a column flex item gets before stretching: its
/// specified width, else its shrink-to-fit width in the container.
fn cross_content_width(item: &mut BlockBox, width: f32, ctx: &mut LayoutContext<'_>) -> f32 {
    crate::block::shrink_to_fit_width(item, width, width, ctx)
}
