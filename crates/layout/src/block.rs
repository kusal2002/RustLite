//! Block formatting: width resolution (CSS 2.1 §10.3.3), heights (§10.6.3),
//! vertical margin collapsing (§8.3.1), floats and clearance (§9.5), and
//! `box-sizing` (CSS Box Sizing 3 §4.1).

use rustlite_style::ComputedStyle;
use rustlite_style::properties::{BoxSizing, Clear, Display, Float, Overflow};
use rustlite_style::{ComputedLength, ComputedLengthAuto};

use crate::{BlockBox, BlockChildren, Edges, LayoutContext, RectF, inline};

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

/// Sizes imposed by the caller instead of the box's own width/height
/// resolution: shrink-to-fit widths (floats, inline-blocks, absolutely
/// positioned boxes) and flex-resolved sizes. Content-box px.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Forced {
    pub content_width: Option<f32>,
    pub content_height: Option<f32>,
}

/// Floats of one block formatting context, as margin boxes in the
/// coordinates of the context's root content box.
#[derive(Debug, Default)]
pub(crate) struct Floats {
    left: Vec<RectF>,
    right: Vec<RectF>,
    /// Top of the last placed float: later floats may not go higher (§9.5.1 rule 5).
    last_top: f32,
}

impl Floats {
    pub fn is_empty(&self) -> bool {
        self.left.is_empty() && self.right.is_empty()
    }

    /// The free horizontal span `(left, right)` within `lo..hi` over the band
    /// `y..y+h`.
    pub fn band(&self, y: f32, h: f32, lo: f32, hi: f32) -> (f32, f32) {
        let h = h.max(0.01);
        let overlaps = |r: &&RectF| r.y < y + h && r.bottom() > y && r.height > 0.0;
        let left = self
            .left
            .iter()
            .filter(overlaps)
            .map(RectF::right)
            .fold(lo, f32::max);
        let right = self
            .right
            .iter()
            .filter(overlaps)
            .map(|r| r.x)
            .fold(hi, f32::min);
        (left, right.max(left))
    }

    /// The nearest float bottom below `y`, where the free span may widen.
    pub fn next_bottom(&self, y: f32) -> Option<f32> {
        self.left
            .iter()
            .chain(&self.right)
            .map(RectF::bottom)
            .filter(|&b| b > y + 0.001)
            .min_by(f32::total_cmp)
    }

    /// Where a box with `clear` must start (its border edge), if anywhere.
    pub fn clear_y(&self, clear: Clear) -> Option<f32> {
        let sides: &[&Vec<RectF>] = match clear {
            Clear::None => return None,
            Clear::Left => &[&self.left],
            Clear::Right => &[&self.right],
            Clear::Both => &[&self.left, &self.right],
        };
        sides
            .iter()
            .flat_map(|v| v.iter())
            .map(RectF::bottom)
            .max_by(f32::total_cmp)
    }

    pub fn bottom(&self) -> f32 {
        self.left
            .iter()
            .chain(&self.right)
            .map(RectF::bottom)
            .fold(0.0, f32::max)
    }

    /// Places a float's margin box (`w` × `h`) no higher than `y`, as far
    /// left (or right) as it fits within `lo..hi` (CSS 2.1 §9.5.1). Returns
    /// its top-left corner.
    pub fn place(&mut self, left: bool, w: f32, h: f32, y: f32, lo: f32, hi: f32) -> (f32, f32) {
        let mut y = y.max(self.last_top);
        loop {
            let (l, r) = self.band(y, h, lo, hi);
            let fits = r - l >= w - 0.01;
            let next = self.next_bottom(y);
            if fits || next.is_none() || (l <= lo && r >= hi) {
                let x = if left { l } else { r - w };
                let rect = RectF::new(x, y, w, h);
                if left {
                    self.left.push(rect);
                } else {
                    self.right.push(rect);
                }
                self.last_top = y;
                return (x, y);
            }
            y = next.unwrap_or(y);
        }
    }
}

pub(crate) fn length(l: ComputedLength, basis: f32) -> f32 {
    match l {
        ComputedLength::Px(v) => v,
        ComputedLength::Percentage(p) => basis * p / 100.0,
    }
}

/// `None` for `auto` (or a percentage against an indefinite basis).
pub(crate) fn length_auto(l: ComputedLengthAuto, basis: Option<f32>) -> Option<f32> {
    match l {
        ComputedLengthAuto::Auto => None,
        ComputedLengthAuto::Length(ComputedLength::Px(v)) => Some(v),
        ComputedLengthAuto::Length(ComputedLength::Percentage(p)) => basis.map(|b| b * p / 100.0),
    }
}

/// Converts a specified `width`/`height` to a content-box size.
pub(crate) fn to_content(style: &ComputedStyle, specified: Option<f32>, frame: f32) -> Option<f32> {
    specified.map(|v| match style.box_sizing {
        BoxSizing::ContentBox => v,
        BoxSizing::BorderBox => (v - frame).max(0.0),
    })
}

/// Padding + border widths, horizontal and vertical (percent padding refers
/// to the containing block width).
pub(crate) fn frame(style: &ComputedStyle, cb_width: f32) -> (f32, f32) {
    let p = |i: usize| length(style.padding[i], cb_width);
    (
        p(1) + p(3) + style.border_width[1] + style.border_width[3],
        p(0) + p(2) + style.border_width[0] + style.border_width[2],
    )
}

/// Does this box establish a new block formatting context (CSS 2.1 §9.4.1)?
/// Its margins then never collapse with its children's, it contains its
/// floats, and it does not overlap floats outside it.
pub(crate) fn establishes_bfc(style: &ComputedStyle, is_root: bool) -> bool {
    is_root
        || style.float != Float::None
        || style.is_out_of_flow_positioned()
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

/// The first line baseline inside `children`, relative to their parent's
/// content box (valid during layout, before coordinates become absolute).
pub(crate) fn first_baseline(children: &BlockChildren) -> Option<f32> {
    match children {
        BlockChildren::Inline(content) => inline::first_baseline(content),
        BlockChildren::Blocks(kids) | BlockChildren::Flex(kids) => kids
            .iter()
            .filter(|k| k.style.float == Float::None)
            .find_map(|k| {
                first_baseline(&k.children).map(|b| k.rect.y + k.border.top + k.padding.top + b)
            }),
        BlockChildren::Empty => None,
    }
}

/// The last line baseline inside `children` (an inline-block's baseline).
pub(crate) fn last_baseline(children: &BlockChildren) -> Option<f32> {
    match children {
        BlockChildren::Inline(content) => inline::last_baseline(content),
        BlockChildren::Blocks(kids) | BlockChildren::Flex(kids) => kids
            .iter()
            .rev()
            .filter(|k| k.style.float == Float::None)
            .find_map(|k| {
                last_baseline(&k.children).map(|b| k.rect.y + k.border.top + k.padding.top + b)
            }),
        BlockChildren::Empty => None,
    }
}

/// Lays out the root box in the initial containing block (the viewport).
pub(crate) fn layout_root(
    root: &mut BlockBox,
    viewport: (f32, f32),
    ctx: &mut LayoutContext<'_>,
) -> Outcome {
    layout_block_inner(
        root,
        viewport.0,
        Some(viewport.1),
        ctx,
        true,
        (0.0, 0.0),
        Forced::default(),
    )
}

/// Lays out `b` in a containing block of `cb_width` (and `cb_height` if
/// definite). `pos` is where the containing block's content box starts
/// horizontally and where `b`'s border box (probably) starts vertically, in
/// the coordinates of the current block formatting context; floats are
/// avoided relative to it. On return `b.rect` has its size and an x relative
/// to the containing block; the parent sets y.
pub(crate) fn layout_block_inner(
    b: &mut BlockBox,
    cb_width: f32,
    cb_height: Option<f32>,
    ctx: &mut LayoutContext<'_>,
    is_root: bool,
    pos: (f32, f32),
    forced: Forced,
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
    let vframe = b.border.vertical() + b.padding.vertical();
    // Replaced elements size from their content, not from the containing block.
    let replaced_size = b
        .replaced
        .as_ref()
        .map(|r| crate::replaced::content_size(&s, r.image.as_deref(), cb_width));
    // CSS 2.1 §10.3.3 for one candidate width (`None` = auto).
    let resolve_horizontal = |width: Option<f32>| -> (f32, Option<f32>, Option<f32>) {
        let (mut ml, mut mr) = (margin(3), margin(1));
        let w = match width {
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
        (w, ml, mr)
    };
    let (width, ml, mr) = if let Some(w) = forced.content_width {
        // Shrink-to-fit and flex sizes: `auto` margins are 0 here; the caller
        // positions the box.
        (
            w.max(0.0),
            Some(margin(3).unwrap_or(0.0)),
            Some(margin(1).unwrap_or(0.0)),
        )
    } else {
        let specified = to_content(&s, length_auto(s.width, Some(cb_width)), frame);
        let mut horizontal = resolve_horizontal(specified.or(replaced_size.map(|r| r.0)));
        // CSS 2.1 §10.4: clamp to max-width, then min-width, re-solving margins.
        if let Some(max) = to_content(&s, s.max_width.map(|m| length(m, cb_width)), frame)
            && horizontal.0 > max
        {
            horizontal = resolve_horizontal(Some(max));
        }
        let min_width =
            to_content(&s, length_auto(s.min_width, Some(cb_width)), frame).unwrap_or(0.0);
        if horizontal.0 < min_width {
            horizontal = resolve_horizontal(Some(min_width));
        }
        horizontal
    };
    b.margin = Edges {
        top: margin(0).unwrap_or(0.0),
        right: mr.unwrap_or(0.0),
        bottom: margin(2).unwrap_or(0.0),
        left: ml.unwrap_or(0.0),
    };
    b.rect.x = b.margin.left;
    b.rect.width = width + frame;

    let specified_height = forced.content_height.or_else(|| {
        to_content(&s, length_auto(s.height, cb_height), vframe).or(replaced_size.map(|r| r.1))
    });
    let bfc = establishes_bfc(&s, is_root);
    let escape_top = !bfc && b.border.top == 0.0 && b.padding.top == 0.0;
    let escape_bottom =
        !bfc && b.border.bottom == 0.0 && b.padding.bottom == 0.0 && specified_height.is_none();

    // Content box origin in the coordinates of the floats it must avoid: a
    // new formatting context starts its own coordinates at its content box.
    let content_origin = if bfc {
        ctx.floats.push(Floats::default());
        (0.0, 0.0)
    } else {
        (
            pos.0 + b.margin.left + b.border.left + b.padding.left,
            pos.1 + b.border.top + b.padding.top,
        )
    };

    let mut escaped_top = Collapse::default();
    let mut escaped_bottom = Collapse::default();
    let mut through = false;
    // Static positions for out-of-flow boxes: the cursor before each in-flow
    // child (only tracked when there are such boxes).
    let track_statics = !b.out_of_flow.is_empty();
    let mut statics: Vec<f32> = Vec::new();

    let content_height = match &mut b.children {
        BlockChildren::Blocks(kids) => {
            let (cx, cy) = content_origin;
            let mut cursor = 0.0;
            let mut pending = Collapse::default();
            let mut first = true;
            for kid in kids.iter_mut() {
                if track_statics {
                    statics.push(cursor + if first { 0.0 } else { pending.resolve() });
                }
                if kid.style.float != Float::None {
                    place_float(
                        kid,
                        width,
                        specified_height,
                        ctx,
                        (cx, cy),
                        cursor + if first { 0.0 } else { pending.resolve() },
                    );
                    continue;
                }
                let kid_mt = length_auto(kid.style.margin[0], Some(width)).unwrap_or(0.0);
                let estimate = if first && escape_top {
                    0.0
                } else {
                    pending.join(Collapse::of(kid_mt)).resolve()
                };
                // Clearance (CSS 2.1 §9.5.2): the border edge goes below the floats.
                let clear_to = ctx
                    .floats
                    .last()
                    .and_then(|f| f.clear_y(kid.style.clear))
                    .map(|c| c - cy)
                    .filter(|&c| c > cursor + estimate - kid_mt.min(0.0));
                let kid_top = match clear_to {
                    Some(c) => c,
                    None => cursor + estimate,
                };
                // A new formatting context next to floats is narrowed to fit
                // beside them (CSS 2.1 §9.5).
                let (mut kid_cb_x, mut kid_cb_width) = (cx, width);
                if establishes_bfc(&kid.style, false)
                    && let Some(f) = ctx.floats.last()
                    && !f.is_empty()
                {
                    let (l, r) = f.band(cy + kid_top, 1.0, cx, cx + width);
                    kid_cb_x = l;
                    kid_cb_width = (r - l).max(0.0);
                }
                let o = layout_block_inner(
                    kid,
                    kid_cb_width,
                    specified_height,
                    ctx,
                    false,
                    (kid_cb_x, cy + kid_top),
                    Forced::default(),
                );
                kid.rect.x += kid_cb_x - cx;
                if let Some(c) = clear_to {
                    // Clearance separates the box from what precedes it: no
                    // margin collapsing across it.
                    kid.rect.y = c;
                    cursor = c + o.height;
                    pending = o.bottom;
                    first = false;
                    continue;
                }
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
            if track_statics {
                statics.push(cursor + if first { 0.0 } else { pending.resolve() });
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
            let h = inline::layout_inline(content, width, &s, ctx, content_origin);
            if content.lines.is_empty() {
                through = escape_top && escape_bottom;
            }
            h
        }
        BlockChildren::Flex(items) => {
            crate::flex::layout_flex(items, &s, width, specified_height, cb_height, ctx)
        }
        BlockChildren::Empty => {
            through = escape_top && escape_bottom;
            0.0
        }
    };
    for abs in &mut b.out_of_flow {
        let y = statics.get(abs.static_index).copied().unwrap_or(0.0);
        abs.static_position = (0.0, y);
    }

    // A formatting context root grows to contain its floats (CSS 2.1 §10.6.7).
    let content_height = if bfc {
        let floats = ctx.floats.pop().unwrap_or_default();
        content_height.max(floats.bottom())
    } else {
        content_height
    };
    if bfc {
        // Its margins never collapse through it.
        through = false;
    }

    if let Some(marker) = &mut b.marker {
        let (runs, marker_width, ascent) = inline::shape_text(&marker.text, &s, ctx.fonts);
        // Outside marker: ends where the content box starts, on the first
        // line's baseline (or the font's ascent for an item without lines).
        marker.x = -marker_width;
        marker.baseline = first_baseline(&b.children).unwrap_or(ascent);
        marker.runs = runs;
    }

    // CSS 2.1 §10.7: max-height, then min-height (percentages need a definite
    // containing block height, else they do not apply).
    let mut height = specified_height.unwrap_or(content_height).max(0.0);
    if forced.content_height.is_none() {
        if let Some(max) = to_content(
            &s,
            s.max_height.and_then(|m| match m {
                ComputedLength::Px(v) => Some(v),
                ComputedLength::Percentage(p) => cb_height.map(|h| h * p / 100.0),
            }),
            vframe,
        ) {
            height = height.min(max);
        }
        if let Some(min) = to_content(&s, length_auto(s.min_height, cb_height), vframe) {
            height = height.max(min);
        }
    }
    // A box with a non-zero height cannot have margins collapsing through it.
    let through = through && height == 0.0;
    b.rect.height = height + vframe;

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

/// The content width a shrink-to-fit box takes in `available` px of margin
/// box space (CSS 2.1 §10.3.5): its specified width, else
/// `min(max(min-content, available), max-content)`, clamped by min/max-width.
pub(crate) fn shrink_to_fit_width(
    kid: &mut BlockBox,
    available: f32,
    cb_width: f32,
    ctx: &mut LayoutContext<'_>,
) -> f32 {
    let s = kid.style.clone();
    let (hframe, _) = frame(&s, cb_width);
    let margins = length_auto(s.margin[1], Some(cb_width)).unwrap_or(0.0)
        + length_auto(s.margin[3], Some(cb_width)).unwrap_or(0.0);
    let w = match to_content(&s, length_auto(s.width, Some(cb_width)), hframe) {
        Some(w) => w,
        None => {
            let (min, max) = crate::intrinsic::content_widths(kid, ctx);
            let avail = (available - margins - hframe).max(0.0);
            min.max(avail).min(max)
        }
    };
    let mut w = w;
    if let Some(max) = to_content(&s, s.max_width.map(|m| length(m, cb_width)), hframe) {
        w = w.min(max);
    }
    if let Some(min) = to_content(&s, length_auto(s.min_width, Some(cb_width)), hframe) {
        w = w.max(min);
    }
    w.max(0.0)
}

/// Lays out and places a float child of a block container whose content box
/// is at `content` (formatting-context coordinates) and `width` wide;
/// `y` is the current position in the container.
fn place_float(
    kid: &mut BlockBox,
    width: f32,
    cb_height: Option<f32>,
    ctx: &mut LayoutContext<'_>,
    content: (f32, f32),
    y: f32,
) {
    let content_width = shrink_to_fit_width(kid, width, width, ctx);
    let o = layout_block_inner(
        kid,
        width,
        cb_height,
        ctx,
        false,
        (0.0, 0.0),
        Forced {
            content_width: Some(content_width),
            content_height: None,
        },
    );
    let w = kid.rect.width + kid.margin.left + kid.margin.right;
    let h = (o.height + kid.margin.top + kid.margin.bottom).max(0.0);
    let Some(floats) = ctx.floats.last_mut() else {
        return;
    };
    let mut top = content.1 + y;
    if let Some(c) = floats.clear_y(kid.style.clear) {
        top = top.max(c);
    }
    let (x, fy) = floats.place(
        kid.style.float == Float::Left,
        w,
        h,
        top,
        content.0,
        content.0 + width,
    );
    kid.rect.x = x - content.0 + kid.margin.left;
    kid.rect.y = fy - content.1 + kid.margin.top;
}
