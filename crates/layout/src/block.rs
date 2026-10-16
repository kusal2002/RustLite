//! Block formatting: width resolution (CSS 2.1 §10.3.3), heights (§10.6.3),
//! and vertical margin collapsing (§8.3.1).

use rustlite_style::ComputedStyle;
use rustlite_style::properties::{Display, Overflow};
use rustlite_style::{ComputedLength, ComputedLengthAuto};

use crate::{BlockBox, BlockChildren, Edges, LayoutContext, inline};

/// A set of adjoining margins: the largest positive and most negative.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct Collapse {
    pos: f32,
    neg: f32,
}

impl Collapse {
    pub fn of(margin: f32) -> Self {
        Self {
            pos: margin.max(0.0),
            neg: margin.min(0.0),
        }
    }

    pub fn join(self, o: Collapse) -> Self {
        Self {
            pos: self.pos.max(o.pos),
            neg: self.neg.min(o.neg),
        }
    }

    pub fn resolve(self) -> f32 {
        self.pos + self.neg
    }
}

/// What a laid-out block tells its parent.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Outcome {
    /// Border-box height.
    pub height: f32,
    /// Collapsed top margin (own margin joined with any escaping child margins).
    pub top: Collapse,
    /// For boxes margins collapse through: the top margin alone (without the
    /// bottom), which positions the box (CSS 2.1 §8.3.1).
    pub top_only: Collapse,
    pub bottom: Collapse,
    /// True if top and bottom margins collapse through this box (it is empty).
    pub through: bool,
}

fn length(l: ComputedLength, basis: f32) -> f32 {
    match l {
        ComputedLength::Px(v) => v,
        ComputedLength::Percentage(p) => basis * p / 100.0,
    }
}

/// `None` for `auto` (or a percentage against an indefinite basis).
fn length_auto(l: ComputedLengthAuto, basis: Option<f32>) -> Option<f32> {
    match l {
        ComputedLengthAuto::Auto => None,
        ComputedLengthAuto::Length(ComputedLength::Px(v)) => Some(v),
        ComputedLengthAuto::Length(ComputedLength::Percentage(p)) => basis.map(|b| b * p / 100.0),
    }
}

/// Does this box establish a new block formatting context (CSS 2.1 §9.4.1)?
/// Its margins then never collapse with its children's.
fn establishes_bfc(style: &ComputedStyle, is_root: bool) -> bool {
    is_root
        || !matches!(style.overflow_x, Overflow::Visible | Overflow::Clip)
        || matches!(
            style.display,
            Display::FlowRoot
                | Display::Flex
                | Display::InlineFlex
                | Display::Grid
                | Display::InlineGrid
                | Display::Table
                | Display::InlineTable
                | Display::TableCell
                | Display::TableCaption
                | Display::InlineBlock
        )
}

/// Lays out the root box in the initial containing block (the viewport).
pub(crate) fn layout_root(
    root: &mut BlockBox,
    viewport: (f32, f32),
    ctx: &mut LayoutContext<'_>,
) -> Outcome {
    layout_block_inner(root, viewport.0, Some(viewport.1), ctx, true)
}

/// Lays out `b` in a containing block of `cb_width` (and `cb_height` if
/// definite). On return `b.rect` has its size and an x relative to the
/// containing block; the parent sets y.
fn layout_block_inner(
    b: &mut BlockBox,
    cb_width: f32,
    cb_height: Option<f32>,
    ctx: &mut LayoutContext<'_>,
    is_root: bool,
) -> Outcome {
    let s = b.style.clone();
    let pct = |l: ComputedLength| length(l, cb_width);
    b.padding = Edges {
        top: pct(s.padding[0]),
        right: pct(s.padding[1]),
        bottom: pct(s.padding[2]),
        left: pct(s.padding[3]),
    };
    b.border = Edges {
        top: s.border_width[0],
        right: s.border_width[1],
        bottom: s.border_width[2],
        left: s.border_width[3],
    };
    let margin = |i: usize| length_auto(s.margin[i], Some(cb_width));

    // Horizontal: margin-left + border + padding + width + ... = cb_width.
    let frame = b.border.horizontal() + b.padding.horizontal();
    let (mut ml, mut mr) = (margin(3), margin(1));
    let width = match length_auto(s.width, Some(cb_width)) {
        None => {
            let w = cb_width - ml.unwrap_or(0.0) - mr.unwrap_or(0.0) - frame;
            ml.get_or_insert(0.0);
            mr.get_or_insert(0.0);
            w.max(0.0)
        }
        Some(w) => {
            let remaining = cb_width - w - frame;
            match (ml, mr) {
                (None, None) => {
                    let half = remaining / 2.0;
                    if half < 0.0 {
                        ml = Some(0.0);
                        mr = Some(remaining);
                    } else {
                        ml = Some(half);
                        mr = Some(half);
                    }
                }
                (None, Some(r)) => ml = Some(remaining - r),
                (Some(l), _) => mr = Some(remaining - l),
            }
            w
        }
    };
    b.margin = Edges {
        top: margin(0).unwrap_or(0.0),
        right: mr.unwrap_or(0.0),
        bottom: margin(2).unwrap_or(0.0),
        left: ml.unwrap_or(0.0),
    };
    b.rect.x = b.margin.left;
    b.rect.width = width + frame;

    let specified_height = length_auto(s.height, cb_height);
    let bfc = establishes_bfc(&s, is_root);
    let escape_top = !bfc && b.border.top == 0.0 && b.padding.top == 0.0;
    let escape_bottom =
        !bfc && b.border.bottom == 0.0 && b.padding.bottom == 0.0 && specified_height.is_none();

    let mut escaped_top = Collapse::default();
    let mut escaped_bottom = Collapse::default();
    let mut through = false;

    let content_height = match &mut b.children {
        BlockChildren::Blocks(kids) => {
            let mut cursor = 0.0;
            let mut pending = Collapse::default();
            let mut first = true;
            for kid in kids.iter_mut() {
                let o = layout_block_inner(kid, width, specified_height, ctx, false);
                if o.through {
                    // Its top border edge sits where it would if it had a
                    // bottom border: after the margins adjoining its top only.
                    kid.rect.y = if first && escape_top {
                        0.0
                    } else {
                        cursor + pending.join(o.top_only).resolve()
                    };
                    pending = pending.join(o.top).join(o.bottom);
                    continue;
                }
                let combined = pending.join(o.top);
                let offset = if first && escape_top {
                    escaped_top = combined;
                    0.0
                } else {
                    combined.resolve()
                };
                kid.rect.y = cursor + offset;
                cursor += offset + o.height;
                pending = o.bottom;
                first = false;
            }
            if first {
                // No in-flow content: margins of empty children pass through.
                if escape_top && escape_bottom {
                    through = true;
                    escaped_top = pending;
                    0.0
                } else if escape_top {
                    escaped_top = pending;
                    0.0
                } else {
                    pending.resolve().max(0.0)
                }
            } else if escape_bottom {
                escaped_bottom = pending;
                cursor
            } else {
                cursor + pending.resolve()
            }
        }
        BlockChildren::Inline(content) => {
            let h = inline::layout_inline(content, width, &s, ctx);
            if content.lines.is_empty() {
                through = escape_top && escape_bottom;
            }
            h
        }
        BlockChildren::Empty => {
            through = escape_top && escape_bottom;
            0.0
        }
    };

    let height = specified_height.unwrap_or(content_height).max(0.0);
    // A box with a non-zero height cannot have margins collapsing through it.
    let through = through && height == 0.0;
    b.rect.height = height + b.border.vertical() + b.padding.vertical();

    let own_top = Collapse::of(b.margin.top);
    let own_bottom = Collapse::of(b.margin.bottom);
    let top_only = own_top.join(escaped_top);
    if through {
        Outcome {
            height: b.rect.height,
            top: top_only.join(own_bottom),
            top_only,
            bottom: Collapse::default(),
            through: true,
        }
    } else {
        Outcome {
            height: b.rect.height,
            top: top_only,
            top_only,
            bottom: own_bottom.join(escaped_bottom),
            through: false,
        }
    }
}
