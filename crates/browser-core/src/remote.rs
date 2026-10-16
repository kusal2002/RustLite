//! Out-of-process pages (ADR-0005): the browser keeps the window, network,
//! navigation policy and raster; a renderer process owns the [`Page`] —
//! HTML, CSS, layout, paint, JavaScript and image decoding — and talks over
//! its stdin/stdout with the frames of [`crate::ipc`].
//!
//! The protocol is request/response, driven by the browser. Every reply
//! carries a [`State`] snapshot so most getters need no round trip.
//! Display lists name fonts by renderer-local ids; the first time an id
//! appears the renderer announces its font file and face index, and the
//! browser maps it to a face *it already knows* (it never opens a path a
//! renderer names). Images travel once per id.
//!
//! Everything a renderer sends is untrusted: decoding is bounds-checked,
//! URLs it returns go through the browser's navigation policy, and a
//! renderer that crashes, sends garbage or stops answering within
//! [`RemotePage::timeout`] is killed and replaced by an error page.

use std::collections::{HashMap, HashSet};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin};
use std::rc::Rc;
use std::sync::mpsc::{Receiver, RecvTimeoutError, channel};
use std::time::{Duration, Instant};

use rustlite_dom::NodeId;
use rustlite_fonts::FontContext;
use rustlite_layout::{FindMatch, RectF, TreeSize};
use rustlite_net::{Destination, Url};
use rustlite_paint::{
    Color, DisplayItem, DisplayList, FontId, GlyphRun, ImageData, PositionedGlyph, Rect, Size,
};

use crate::ipc::{DecodeError, Reader, Writer, read_frame, write_frame};
use crate::{InputOutcome, MemoryReport, Page, PageKey};

/// How long the browser waits for any renderer reply before declaring the
/// renderer hung and killing it. This is also the effective limit on how
/// long one script turn may run.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);

// ----- messages ---------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
enum Request {
    Init {
        width: u32,
        height: u32,
        scale: f32,
        javascript: bool,
    },
    LoadHtml {
        url: String,
        html: String,
        base_dir: Option<String>,
    },
    SetViewport {
        width: u32,
        height: u32,
    },
    SetDeviceScale(f32),
    ScrollBy(f32),
    ScrollTo(f32),
    Paint,
    SubresourceRequests,
    SetStylesheet {
        node: u32,
        css: String,
    },
    SetImage {
        node: u32,
        bytes: Vec<u8>,
    },
    SetScript {
        node: u32,
        source: Option<String>,
    },
    LinkAt {
        x: f32,
        y: f32,
    },
    Click {
        x: f32,
        y: f32,
    },
    KeyDown(PageKey),
    KeyUp(PageKey),
    RunTimers,
    RunAnimationFrame,
    Find(String),
    MemoryReport,
    Console,
}

/// Page state the browser mirrors after every reply.
#[derive(Debug, Clone, Default, PartialEq)]
struct State {
    title: Option<String>,
    scroll_y: f32,
    max_scroll: f32,
    mutation_count: u64,
    layout_generation: u64,
    focused: Option<u32>,
    /// Milliseconds until the next page timer, from the renderer's clock.
    next_timer_ms: Option<f64>,
    wants_frame: bool,
    needs_paint: bool,
}

#[derive(Debug, Clone, PartialEq)]
struct Frame {
    list: DisplayList,
    /// Fonts used for the first time: (renderer id, file, face index).
    faces: Vec<(u32, String, u32)>,
    /// Images sent for the first time: (id, image).
    images: Vec<(u32, Rc<ImageData>)>,
}

#[derive(Debug, Clone, PartialEq)]
enum Payload {
    None,
    Bool(bool),
    Frame(Frame),
    Requests(Vec<(Destination, String, Vec<u32>)>),
    Url(Option<String>),
    Input {
        consumed: bool,
        navigate: Option<String>,
    },
    Matches(Vec<FindMatch>),
    Memory(MemoryReport),
    Console(Vec<String>),
}

// ----- encoding ----------------------------------------------------------------

fn put_key(w: &mut Writer, key: &PageKey) {
    let (tag, text) = match key {
        PageKey::Text(t) => (0, Some(t.as_str())),
        PageKey::Backspace => (1, None),
        PageKey::Delete => (2, None),
        PageKey::Left => (3, None),
        PageKey::Right => (4, None),
        PageKey::Home => (5, None),
        PageKey::End => (6, None),
        PageKey::Enter => (7, None),
        PageKey::Tab { shift: false } => (8, None),
        PageKey::Tab { shift: true } => (9, None),
        PageKey::Escape => (10, None),
        PageKey::Other(k) => (11, Some(k.as_str())),
    };
    w.u8(tag);
    if let Some(t) = text {
        w.str(t);
    }
}

fn get_key(r: &mut Reader<'_>) -> Result<PageKey, DecodeError> {
    Ok(match r.u8()? {
        0 => PageKey::Text(r.str()?),
        1 => PageKey::Backspace,
        2 => PageKey::Delete,
        3 => PageKey::Left,
        4 => PageKey::Right,
        5 => PageKey::Home,
        6 => PageKey::End,
        7 => PageKey::Enter,
        8 => PageKey::Tab { shift: false },
        9 => PageKey::Tab { shift: true },
        10 => PageKey::Escape,
        11 => PageKey::Other(r.str()?),
        _ => return Err(DecodeError::Invalid("key")),
    })
}

fn put_destination(w: &mut Writer, d: Destination) {
    w.u8(match d {
        Destination::Document => 0,
        Destination::Style => 1,
        Destination::Image => 2,
        Destination::Script => 3,
    });
}

fn get_destination(r: &mut Reader<'_>) -> Result<Destination, DecodeError> {
    Ok(match r.u8()? {
        0 => Destination::Document,
        1 => Destination::Style,
        2 => Destination::Image,
        3 => Destination::Script,
        _ => return Err(DecodeError::Invalid("destination")),
    })
}

impl Request {
    fn encode(&self) -> Vec<u8> {
        let mut w = Writer::default();
        match self {
            Request::Init {
                width,
                height,
                scale,
                javascript,
            } => {
                w.u8(0);
                w.u32(*width);
                w.u32(*height);
                w.f32(*scale);
                w.bool(*javascript);
            }
            Request::LoadHtml {
                url,
                html,
                base_dir,
            } => {
                w.u8(1);
                w.str(url);
                w.str(html);
                w.opt_str(base_dir.as_deref());
            }
            Request::SetViewport { width, height } => {
                w.u8(2);
                w.u32(*width);
                w.u32(*height);
            }
            Request::SetDeviceScale(s) => {
                w.u8(3);
                w.f32(*s);
            }
            Request::ScrollBy(d) => {
                w.u8(4);
                w.f32(*d);
            }
            Request::ScrollTo(y) => {
                w.u8(5);
                w.f32(*y);
            }
            Request::Paint => w.u8(6),
            Request::SubresourceRequests => w.u8(7),
            Request::SetStylesheet { node, css } => {
                w.u8(8);
                w.u32(*node);
                w.str(css);
            }
            Request::SetImage { node, bytes } => {
                w.u8(9);
                w.u32(*node);
                w.bytes(bytes);
            }
            Request::SetScript { node, source } => {
                w.u8(10);
                w.u32(*node);
                w.opt_str(source.as_deref());
            }
            Request::LinkAt { x, y } => {
                w.u8(11);
                w.f32(*x);
                w.f32(*y);
            }
            Request::Click { x, y } => {
                w.u8(12);
                w.f32(*x);
                w.f32(*y);
            }
            Request::KeyDown(k) => {
                w.u8(13);
                put_key(&mut w, k);
            }
            Request::KeyUp(k) => {
                w.u8(14);
                put_key(&mut w, k);
            }
            Request::RunTimers => w.u8(15),
            Request::RunAnimationFrame => w.u8(16),
            Request::Find(q) => {
                w.u8(17);
                w.str(q);
            }
            Request::MemoryReport => w.u8(18),
            Request::Console => w.u8(19),
        }
        w.buf
    }

    fn decode(data: &[u8]) -> Result<Self, DecodeError> {
        let mut r = Reader::new(data);
        let req = match r.u8()? {
            0 => Request::Init {
                width: r.u32()?,
                height: r.u32()?,
                scale: r.f32()?,
                javascript: r.bool()?,
            },
            1 => Request::LoadHtml {
                url: r.str()?,
                html: r.str()?,
                base_dir: r.opt_str()?,
            },
            2 => Request::SetViewport {
                width: r.u32()?,
                height: r.u32()?,
            },
            3 => Request::SetDeviceScale(r.f32()?),
            4 => Request::ScrollBy(r.f32()?),
            5 => Request::ScrollTo(r.f32()?),
            6 => Request::Paint,
            7 => Request::SubresourceRequests,
            8 => Request::SetStylesheet {
                node: r.u32()?,
                css: r.str()?,
            },
            9 => Request::SetImage {
                node: r.u32()?,
                bytes: r.bytes()?.to_vec(),
            },
            10 => Request::SetScript {
                node: r.u32()?,
                source: r.opt_str()?,
            },
            11 => Request::LinkAt {
                x: r.f32()?,
                y: r.f32()?,
            },
            12 => Request::Click {
                x: r.f32()?,
                y: r.f32()?,
            },
            13 => Request::KeyDown(get_key(&mut r)?),
            14 => Request::KeyUp(get_key(&mut r)?),
            15 => Request::RunTimers,
            16 => Request::RunAnimationFrame,
            17 => Request::Find(r.str()?),
            18 => Request::MemoryReport,
            19 => Request::Console,
            _ => return Err(DecodeError::Invalid("request")),
        };
        if !r.finished() {
            return Err(DecodeError::Invalid("trailing bytes"));
        }
        Ok(req)
    }
}

fn put_rect(w: &mut Writer, r: Rect) {
    w.i32(r.x);
    w.i32(r.y);
    w.u32(r.width);
    w.u32(r.height);
}

fn get_rect(r: &mut Reader<'_>) -> Result<Rect, DecodeError> {
    Ok(Rect::new(r.i32()?, r.i32()?, r.u32()?, r.u32()?))
}

fn put_color(w: &mut Writer, c: Color) {
    w.u8(c.r);
    w.u8(c.g);
    w.u8(c.b);
    w.u8(c.a);
}

fn get_color(r: &mut Reader<'_>) -> Result<Color, DecodeError> {
    Ok(Color::rgba(r.u8()?, r.u8()?, r.u8()?, r.u8()?))
}

fn put_rectf(w: &mut Writer, r: RectF) {
    w.f32(r.x);
    w.f32(r.y);
    w.f32(r.width);
    w.f32(r.height);
}

fn get_rectf(r: &mut Reader<'_>) -> Result<RectF, DecodeError> {
    Ok(RectF::new(r.f32()?, r.f32()?, r.f32()?, r.f32()?))
}

/// Largest image dimension accepted from a renderer (matches the decoder's).
const MAX_IMAGE_SIDE: u32 = 16_384;

impl State {
    fn encode(&self, w: &mut Writer) {
        w.opt_str(self.title.as_deref());
        w.f32(self.scroll_y);
        w.f32(self.max_scroll);
        w.u64(self.mutation_count);
        w.u64(self.layout_generation);
        w.opt_u32(self.focused);
        w.opt_f64(self.next_timer_ms);
        w.bool(self.wants_frame);
        w.bool(self.needs_paint);
    }

    fn decode(r: &mut Reader<'_>) -> Result<Self, DecodeError> {
        Ok(Self {
            title: r.opt_str()?,
            scroll_y: r.f32()?,
            max_scroll: r.f32()?,
            mutation_count: r.u64()?,
            layout_generation: r.u64()?,
            focused: r.opt_u32()?,
            next_timer_ms: r.opt_f64()?,
            wants_frame: r.bool()?,
            needs_paint: r.bool()?,
        })
    }
}

impl Payload {
    fn encode(&self, w: &mut Writer, images: &HashMap<usize, (u32, Rc<ImageData>)>) {
        match self {
            Payload::None => w.u8(0),
            Payload::Bool(b) => {
                w.u8(1);
                w.bool(*b);
            }
            Payload::Frame(f) => {
                w.u8(2);
                w.len(f.faces.len());
                for (id, path, index) in &f.faces {
                    w.u32(*id);
                    w.str(path);
                    w.u32(*index);
                }
                w.len(f.images.len());
                for (id, img) in &f.images {
                    w.u32(*id);
                    w.u32(img.width);
                    w.u32(img.height);
                    w.bytes(&img.rgba);
                }
                w.len(f.list.items().len());
                for item in f.list.items() {
                    match item {
                        DisplayItem::Text {
                            x,
                            y,
                            scale,
                            color,
                            run,
                        } => {
                            w.u8(0);
                            w.f32(*x);
                            w.f32(*y);
                            w.f32(*scale);
                            put_color(w, *color);
                            w.u32(run.font.0);
                            w.f32(run.size);
                            w.len(run.glyphs.len());
                            for g in run.glyphs.iter() {
                                w.u32(u32::from(g.id));
                                w.f32(g.x);
                                w.f32(g.y);
                            }
                        }
                        DisplayItem::Image { rect, image } => {
                            w.u8(1);
                            put_rect(w, *rect);
                            let key = Rc::as_ptr(image) as usize;
                            w.u32(images.get(&key).map_or(0, |(id, _)| *id));
                        }
                        DisplayItem::SolidRect { rect, color } => {
                            w.u8(2);
                            put_rect(w, *rect);
                            put_color(w, *color);
                        }
                        DisplayItem::Border { rect, width, color } => {
                            w.u8(3);
                            put_rect(w, *rect);
                            w.u32(*width);
                            put_color(w, *color);
                        }
                        DisplayItem::PushClip { rect } => {
                            w.u8(4);
                            put_rect(w, *rect);
                        }
                        DisplayItem::PopClip => w.u8(5),
                    }
                }
            }
            Payload::Requests(list) => {
                w.u8(3);
                w.len(list.len());
                for (d, url, nodes) in list {
                    put_destination(w, *d);
                    w.str(url);
                    w.len(nodes.len());
                    for n in nodes {
                        w.u32(*n);
                    }
                }
            }
            Payload::Url(u) => {
                w.u8(4);
                w.opt_str(u.as_deref());
            }
            Payload::Input { consumed, navigate } => {
                w.u8(5);
                w.bool(*consumed);
                w.opt_str(navigate.as_deref());
            }
            Payload::Matches(matches) => {
                w.u8(6);
                w.len(matches.len());
                for m in matches {
                    w.len(m.len());
                    for r in m {
                        put_rectf(w, *r);
                    }
                }
            }
            Payload::Memory(m) => {
                w.u8(7);
                for v in [
                    m.dom_nodes,
                    m.dom_bytes,
                    m.styles,
                    m.style_bytes,
                    m.layout.boxes,
                    m.layout.lines,
                    m.layout.fragments,
                    m.layout.glyphs,
                    m.layout.bytes,
                    m.images,
                    m.image_bytes,
                    m.display_items,
                    m.display_list_bytes,
                    m.glyph_cache_bytes,
                    m.shape_cache_bytes,
                ] {
                    w.u64(v as u64);
                }
            }
            Payload::Console(lines) => {
                w.u8(8);
                w.len(lines.len());
                for l in lines {
                    w.str(l);
                }
            }
        }
    }

    /// Decodes a payload; image references resolve against `known` images
    /// plus the ones the frame itself carries.
    fn decode(
        r: &mut Reader<'_>,
        known: &HashMap<u32, Rc<ImageData>>,
    ) -> Result<Self, DecodeError> {
        Ok(match r.u8()? {
            0 => Payload::None,
            1 => Payload::Bool(r.bool()?),
            2 => {
                let mut faces = Vec::new();
                for _ in 0..r.len(12)? {
                    faces.push((r.u32()?, r.str()?, r.u32()?));
                }
                let mut images: Vec<(u32, Rc<ImageData>)> = Vec::new();
                for _ in 0..r.len(16)? {
                    let id = r.u32()?;
                    let (width, height) = (r.u32()?, r.u32()?);
                    if width > MAX_IMAGE_SIDE || height > MAX_IMAGE_SIDE {
                        return Err(DecodeError::Invalid("image size"));
                    }
                    let rgba = r.bytes()?.to_vec();
                    let img = ImageData::new(width, height, rgba)
                        .ok_or(DecodeError::Invalid("image data"))?;
                    images.push((id, Rc::new(img)));
                }
                let lookup = |id: u32| {
                    images
                        .iter()
                        .find(|(i, _)| *i == id)
                        .map(|(_, img)| img.clone())
                        .or_else(|| known.get(&id).cloned())
                };
                let mut list = DisplayList::new();
                for _ in 0..r.len(1)? {
                    match r.u8()? {
                        0 => {
                            let (x, y, scale) = (r.f32()?, r.f32()?, r.f32()?);
                            let color = get_color(r)?;
                            let font = FontId(r.u32()?);
                            let size = r.f32()?;
                            let mut glyphs = Vec::new();
                            for _ in 0..r.len(12)? {
                                let id = u16::try_from(r.u32()?)
                                    .map_err(|_| DecodeError::Invalid("glyph id"))?;
                                glyphs.push(PositionedGlyph {
                                    id,
                                    x: r.f32()?,
                                    y: r.f32()?,
                                });
                            }
                            list.push(DisplayItem::Text {
                                x,
                                y,
                                scale,
                                color,
                                run: GlyphRun {
                                    font,
                                    size,
                                    glyphs: glyphs.into(),
                                },
                            });
                        }
                        1 => {
                            let rect = get_rect(r)?;
                            if let Some(image) = lookup(r.u32()?) {
                                list.push(DisplayItem::Image { rect, image });
                            }
                        }
                        2 => {
                            let rect = get_rect(r)?;
                            list.fill_rect(rect, get_color(r)?);
                        }
                        3 => {
                            let rect = get_rect(r)?;
                            let width = r.u32()?;
                            list.stroke_rect(rect, width, get_color(r)?);
                        }
                        4 => list.push_clip(get_rect(r)?),
                        5 => list.pop_clip(),
                        _ => return Err(DecodeError::Invalid("display item")),
                    }
                }
                Payload::Frame(Frame {
                    list,
                    faces,
                    images,
                })
            }
            3 => {
                let mut list = Vec::new();
                for _ in 0..r.len(9)? {
                    let d = get_destination(r)?;
                    let url = r.str()?;
                    let mut nodes = Vec::new();
                    for _ in 0..r.len(4)? {
                        nodes.push(r.u32()?);
                    }
                    list.push((d, url, nodes));
                }
                Payload::Requests(list)
            }
            4 => Payload::Url(r.opt_str()?),
            5 => Payload::Input {
                consumed: r.bool()?,
                navigate: r.opt_str()?,
            },
            6 => {
                let mut matches = Vec::new();
                for _ in 0..r.len(4)? {
                    let mut m = Vec::new();
                    for _ in 0..r.len(16)? {
                        m.push(get_rectf(r)?);
                    }
                    matches.push(m);
                }
                Payload::Matches(matches)
            }
            7 => {
                let mut v = [0usize; 15];
                for x in &mut v {
                    *x = usize::try_from(r.u64()?).unwrap_or(usize::MAX);
                }
                Payload::Memory(MemoryReport {
                    dom_nodes: v[0],
                    dom_bytes: v[1],
                    styles: v[2],
                    style_bytes: v[3],
                    layout: TreeSize {
                        boxes: v[4],
                        lines: v[5],
                        fragments: v[6],
                        glyphs: v[7],
                        bytes: v[8],
                    },
                    images: v[9],
                    image_bytes: v[10],
                    display_items: v[11],
                    display_list_bytes: v[12],
                    glyph_cache_bytes: v[13],
                    shape_cache_bytes: v[14],
                })
            }
            8 => {
                let mut lines = Vec::new();
                for _ in 0..r.len(4)? {
                    lines.push(r.str()?);
                }
                Payload::Console(lines)
            }
            _ => return Err(DecodeError::Invalid("payload")),
        })
    }
}

// ----- renderer side --------------------------------------------------------------

/// Per-process bookkeeping of what the browser already knows.
#[derive(Default)]
struct Session {
    announced_faces: HashSet<u32>,
    /// Image identity (pointer) → (id, the image kept alive so the pointer
    /// cannot be reused while the id is live).
    images: HashMap<usize, (u32, Rc<ImageData>)>,
    next_image: u32,
}

fn snapshot(page: &mut Page) -> State {
    let now = Instant::now();
    let max_scroll = page.max_scroll();
    State {
        title: page.title(),
        scroll_y: page.scroll_y(),
        max_scroll,
        mutation_count: page.mutation_count(),
        layout_generation: page.layout_count(),
        focused: page.focused().map(NodeId::to_raw),
        next_timer_ms: page
            .next_timer()
            .map(|t| t.saturating_duration_since(now).as_secs_f64() * 1000.0),
        wants_frame: page.wants_animation_frame(),
        needs_paint: page.needs_paint(),
    }
}

fn node(raw: u32) -> Option<NodeId> {
    NodeId::from_raw(raw)
}

fn handle(page: &mut Page, session: &mut Session, req: Request) -> Payload {
    match req {
        Request::Init {
            width,
            height,
            scale,
            javascript,
        } => {
            page.set_viewport(Size::new(width, height));
            page.set_device_scale(scale);
            page.set_javascript_enabled(javascript);
            Payload::None
        }
        Request::LoadHtml {
            url,
            html,
            base_dir,
        } => {
            *session = Session::default();
            match base_dir {
                Some(dir) => page.load_html_with_base(url, &html, Some(Path::new(&dir))),
                None => page.load_html(url, &html),
            }
            Payload::None
        }
        Request::SetViewport { width, height } => {
            page.set_viewport(Size::new(width, height));
            Payload::None
        }
        Request::SetDeviceScale(s) => {
            page.set_device_scale(s);
            Payload::None
        }
        Request::ScrollBy(d) => Payload::Bool(page.scroll_by(d)),
        Request::ScrollTo(y) => Payload::Bool(page.scroll_to(y)),
        Request::Paint => {
            let list = page.display_list().clone();
            let mut faces = Vec::new();
            let mut images = Vec::new();
            for item in list.items() {
                match item {
                    DisplayItem::Text { run, .. } => {
                        if session.announced_faces.insert(run.font.0)
                            && let Some((path, index)) = page.fonts().face_source(run.font)
                        {
                            faces.push((run.font.0, path.to_string_lossy().into_owned(), index));
                        }
                    }
                    DisplayItem::Image { image, .. } => {
                        let key = Rc::as_ptr(image) as usize;
                        if !session.images.contains_key(&key) {
                            session.next_image += 1;
                            let id = session.next_image;
                            session.images.insert(key, (id, image.clone()));
                            images.push((id, image.clone()));
                        }
                    }
                    _ => {}
                }
            }
            Payload::Frame(Frame {
                list,
                faces,
                images,
            })
        }
        Request::SubresourceRequests => Payload::Requests(
            page.subresource_requests()
                .into_iter()
                .map(|(d, u, nodes)| {
                    (
                        d,
                        u.to_string(),
                        nodes.into_iter().map(NodeId::to_raw).collect(),
                    )
                })
                .collect(),
        ),
        Request::SetStylesheet { node: n, css } => {
            if let Some(n) = node(n) {
                page.set_external_stylesheet(n, &css);
            }
            Payload::None
        }
        Request::SetImage { node: n, bytes } => {
            Payload::Bool(node(n).is_some_and(|n| page.set_image_bytes(n, &bytes)))
        }
        Request::SetScript { node: n, source } => {
            if let Some(n) = node(n) {
                page.set_script_source(n, source);
            }
            Payload::None
        }
        Request::LinkAt { x, y } => Payload::Url(page.link_at(x, y).map(|u| u.to_string())),
        Request::Click { x, y } => input_payload(page.click(x, y)),
        Request::KeyDown(k) => input_payload(page.key_down(&k)),
        Request::KeyUp(k) => {
            page.key_up(&k);
            Payload::None
        }
        Request::RunTimers => Payload::Bool(page.run_timers(Instant::now())),
        Request::RunAnimationFrame => Payload::Bool(page.run_animation_frame(Instant::now())),
        Request::Find(q) => Payload::Matches(page.find(&q)),
        Request::MemoryReport => Payload::Memory(page.memory_report()),
        Request::Console => Payload::Console(page.console()),
    }
}

fn input_payload(out: InputOutcome) -> Payload {
    Payload::Input {
        consumed: out.consumed,
        navigate: out.navigate.map(|u| u.to_string()),
    }
}

/// The renderer process main loop: serves requests from `input` until it
/// closes. Each request gets exactly one reply on `output`.
pub fn serve(input: &mut impl Read, output: &mut impl Write) -> io::Result<()> {
    let mut page = Page::new(Size::new(800, 600));
    let mut session = Session::default();
    while let Some(frame) = read_frame(input)? {
        let req = Request::decode(&frame)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
        let payload = handle(&mut page, &mut session, req);
        let mut w = Writer::default();
        snapshot(&mut page).encode(&mut w);
        payload.encode(&mut w, &session.images);
        write_frame(output, &w.buf)?;
    }
    Ok(())
}

// ----- browser side -----------------------------------------------------------------

/// A started renderer: the process (stdin and stdout piped) and whatever
/// must live as long as it (its sandbox job; dropping that kills it).
pub struct Spawned {
    pub child: Child,
    pub guard: Option<Box<dyn std::any::Any>>,
}

impl std::fmt::Debug for Spawned {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Spawned")
            .field("pid", &self.child.id())
            .field("guarded", &self.guard.is_some())
            .finish()
    }
}

/// Starts a renderer process.
pub type Spawner = Rc<dyn Fn() -> io::Result<Spawned>>;

struct Process {
    child: Child,
    stdin: ChildStdin,
    replies: Receiver<Option<Vec<u8>>>,
    _guard: Option<Box<dyn std::any::Any>>,
}

impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// A page living in a renderer process (see the module docs).
pub struct RemotePage {
    spawn: Spawner,
    process: Option<Process>,
    /// After a crash or hang: a local page showing an error, until the next
    /// navigation starts a fresh renderer.
    fallback: Option<Box<Page>>,
    state: State,
    next_timer: Option<Instant>,
    viewport: Size,
    device_scale: f32,
    javascript: bool,
    /// Site (scheme + host) of the current document: navigating to another
    /// site gets a fresh process.
    site: Option<String>,
    fonts: Option<FontContext>,
    face_sources: HashMap<u32, (PathBuf, u32)>,
    font_map: HashMap<u32, Option<FontId>>,
    images: HashMap<u32, Rc<ImageData>>,
    raw_list: DisplayList,
    list: DisplayList,
    list_valid: bool,
    /// Longest wait for a reply before the renderer is killed.
    pub timeout: Duration,
    /// Renderer processes started so far (for tests and diagnostics).
    pub spawn_count: u32,
}

impl std::fmt::Debug for RemotePage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RemotePage")
            .field("site", &self.site)
            .field("crashed", &self.fallback.is_some())
            .finish_non_exhaustive()
    }
}

fn site_of(url: &str) -> Option<String> {
    let u = Url::parse(url).ok()?;
    Some(match u.host_str() {
        Some(h) => format!("{}://{}", u.scheme(), h),
        None => u.scheme().to_owned(),
    })
}

impl RemotePage {
    pub fn new(spawn: Spawner, viewport: Size) -> Self {
        Self {
            spawn,
            process: None,
            fallback: None,
            state: State::default(),
            next_timer: None,
            viewport,
            device_scale: 1.0,
            javascript: true,
            site: None,
            fonts: None,
            face_sources: HashMap::new(),
            font_map: HashMap::new(),
            images: HashMap::new(),
            raw_list: DisplayList::new(),
            list: DisplayList::new(),
            list_valid: false,
            timeout: DEFAULT_TIMEOUT,
            spawn_count: 0,
        }
    }

    /// Whether the renderer crashed or hung (an error page is shown).
    pub fn crashed(&self) -> bool {
        self.fallback.is_some()
    }

    /// The renderer's process id, if one is running.
    pub fn process_id(&self) -> Option<u32> {
        self.process.as_ref().map(|p| p.child.id())
    }

    fn start(&mut self) -> io::Result<()> {
        self.process = None;
        let Spawned { mut child, guard } = (self.spawn)()?;
        self.spawn_count += 1;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| io::Error::other("renderer stdin not piped"))?;
        let mut stdout = child
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("renderer stdout not piped"))?;
        let (tx, rx) = channel();
        std::thread::Builder::new()
            .name("rustlite-renderer-reader".into())
            .spawn(move || {
                loop {
                    match read_frame(&mut stdout) {
                        Ok(Some(f)) => {
                            if tx.send(Some(f)).is_err() {
                                return;
                            }
                        }
                        _ => {
                            let _ = tx.send(None);
                            return;
                        }
                    }
                }
            })?;
        self.process = Some(Process {
            child,
            stdin,
            replies: rx,
            _guard: guard,
        });
        // Renderer-local ids mean nothing to a new process.
        self.face_sources.clear();
        self.font_map.clear();
        self.images.clear();
        self.list_valid = false;
        let init = Request::Init {
            width: self.viewport.width,
            height: self.viewport.height,
            scale: self.device_scale,
            javascript: self.javascript,
        };
        self.call(init);
        Ok(())
    }

    /// Replaces the renderer with an error page (crash, hang or protocol
    /// violation). The window's fonts move to the fallback page.
    fn fail(&mut self, reason: &str) {
        tracing::error!(reason, "renderer failed");
        self.process = None;
        let mut page = Page::new(self.viewport);
        page.set_device_scale(self.device_scale);
        if let Some(f) = self.fonts.take() {
            page.set_fonts(f);
        }
        let html = crate::error_page_html(
            self.site.as_deref().unwrap_or(""),
            "RENDERER_FAILED",
            &format!("The page's renderer process {reason}. Reload to try again."),
        );
        page.load_html("about:blank", &html);
        self.fallback = Some(Box::new(page));
        self.state = State::default();
        self.next_timer = None;
    }

    /// Sends `req` and waits for the reply. `None` if the renderer is gone.
    fn call(&mut self, req: Request) -> Option<Payload> {
        if self.fallback.is_some() {
            return None;
        }
        if self.process.is_none()
            && let Err(e) = self.start()
        {
            self.fail(&format!("could not start ({e})"));
            return None;
        }
        let process = self.process.as_mut()?;
        if write_frame(&mut process.stdin, &req.encode()).is_err() {
            self.fail("exited");
            return None;
        }
        let reply = match process.replies.recv_timeout(self.timeout) {
            Ok(Some(frame)) => frame,
            Ok(None) | Err(RecvTimeoutError::Disconnected) => {
                self.fail("crashed");
                return None;
            }
            Err(RecvTimeoutError::Timeout) => {
                self.fail(&format!(
                    "stopped responding for {} s and was stopped",
                    self.timeout.as_secs()
                ));
                return None;
            }
        };
        let mut r = Reader::new(&reply);
        let decoded = State::decode(&mut r)
            .and_then(|state| Ok((state, Payload::decode(&mut r, &self.images)?)));
        match decoded {
            Ok((state, payload)) if r.finished() => {
                self.next_timer = state.next_timer_ms.map(|ms| {
                    Instant::now() + Duration::from_secs_f64(ms.clamp(0.0, 1.0e9) / 1000.0)
                });
                if state.needs_paint {
                    self.list_valid = false;
                }
                self.state = state;
                Some(payload)
            }
            _ => {
                self.fail("sent an invalid message");
                None
            }
        }
    }

    fn fallback_mut(&mut self) -> Option<&mut Page> {
        self.fallback.as_deref_mut()
    }

    // ----- the page API (mirrors `Page`) -----

    pub fn load_html_with_base(
        &mut self,
        url: impl Into<String>,
        html: &str,
        base_dir: Option<&Path>,
    ) {
        let url = url.into();
        if url == "about:blank" && html.is_empty() && self.fallback.is_none() {
            // A blank page needs no renderer.
            self.process = None;
            self.site = None;
            self.state = State::default();
            self.next_timer = None;
            self.raw_list = DisplayList::new();
            self.list = DisplayList::new();
            self.list_valid = true;
            return;
        }
        let site = site_of(&url);
        // A new navigation recovers from a crash; another site gets a
        // separate process (site isolation).
        if let Some(mut fb) = self.fallback.take()
            && let Some(f) = fb.take_fonts()
        {
            self.fonts = Some(f);
        }
        if self.process.is_some() && site != self.site {
            self.process = None;
        }
        self.site = site;
        self.call(Request::LoadHtml {
            url,
            html: html.to_owned(),
            base_dir: base_dir.map(|d| d.to_string_lossy().into_owned()),
        });
        self.list_valid = false;
    }

    pub fn load_html(&mut self, url: impl Into<String>, html: &str) {
        self.load_html_with_base(url, html, None);
    }

    pub fn set_javascript_enabled(&mut self, enabled: bool) {
        self.javascript = enabled;
    }

    pub fn title(&self) -> Option<String> {
        match &self.fallback {
            Some(p) => p.title(),
            None => self.state.title.clone(),
        }
    }

    pub fn mutation_count(&self) -> u64 {
        self.state.mutation_count
    }

    pub fn layout_generation(&mut self) -> u64 {
        if let Some(p) = self.fallback_mut() {
            p.layout();
            return p.layout_count();
        }
        self.state.layout_generation
    }

    pub fn fonts(&mut self) -> &mut FontContext {
        if let Some(p) = self.fallback.as_deref_mut() {
            return p.fonts();
        }
        self.fonts
            .get_or_insert_with(FontContext::with_system_fonts)
    }

    pub fn take_fonts(&mut self) -> Option<FontContext> {
        if let Some(p) = self.fallback.as_deref_mut() {
            return p.take_fonts();
        }
        self.fonts.take()
    }

    pub fn set_fonts(&mut self, fonts: FontContext) {
        if let Some(p) = self.fallback.as_deref_mut() {
            return p.set_fonts(fonts);
        }
        self.fonts = Some(fonts);
        // Translations are per context; this is normally the same one.
        self.font_map.clear();
        self.list_valid = false;
    }

    pub fn viewport(&self) -> Size {
        self.viewport
    }

    pub fn set_viewport(&mut self, viewport: Size) {
        if let Some(p) = self.fallback_mut() {
            p.set_viewport(viewport);
        }
        if viewport != self.viewport {
            self.viewport = viewport;
            if self.process.is_none() {
                return;
            }
            self.call(Request::SetViewport {
                width: viewport.width,
                height: viewport.height,
            });
            self.list_valid = false;
        }
    }

    pub fn device_scale(&self) -> f32 {
        self.device_scale
    }

    pub fn set_device_scale(&mut self, scale: f32) {
        if let Some(p) = self.fallback_mut() {
            p.set_device_scale(scale);
        }
        if scale != self.device_scale {
            self.device_scale = scale;
            if self.process.is_none() {
                return;
            }
            self.call(Request::SetDeviceScale(scale));
            self.list_valid = false;
        }
    }

    pub fn css_viewport(&self) -> (f32, f32) {
        (
            self.viewport.width as f32 / self.device_scale,
            self.viewport.height as f32 / self.device_scale,
        )
    }

    pub fn scroll_y(&self) -> f32 {
        match &self.fallback {
            Some(p) => p.scroll_y(),
            None => self.state.scroll_y,
        }
    }

    pub fn max_scroll(&mut self) -> f32 {
        if let Some(p) = self.fallback_mut() {
            return p.max_scroll();
        }
        self.state.max_scroll
    }

    pub fn scroll_by(&mut self, dy: f32) -> bool {
        if let Some(p) = self.fallback_mut() {
            return p.scroll_by(dy);
        }
        matches!(self.call(Request::ScrollBy(dy)), Some(Payload::Bool(true)))
    }

    pub fn scroll_to(&mut self, y: f32) -> bool {
        if let Some(p) = self.fallback_mut() {
            return p.scroll_to(y);
        }
        matches!(self.call(Request::ScrollTo(y)), Some(Payload::Bool(true)))
    }

    pub fn needs_paint(&self) -> bool {
        match &self.fallback {
            Some(p) => p.needs_paint(),
            None => !self.list_valid,
        }
    }

    /// The renderer's display list with its fonts mapped to this context's.
    pub fn display_list(&mut self) -> &DisplayList {
        if self.fallback.is_some() {
            return self
                .fallback
                .as_deref_mut()
                .expect("checked")
                .display_list();
        }
        if !self.list_valid && self.process.is_some() {
            if let Some(Payload::Frame(frame)) = self.call(Request::Paint) {
                for (id, path, index) in frame.faces {
                    self.face_sources.insert(id, (PathBuf::from(path), index));
                }
                for (id, image) in frame.images {
                    self.images.insert(id, image);
                }
                self.raw_list = frame.list;
            }
            self.translate();
            self.list_valid = true;
        }
        &self.list
    }

    /// Maps renderer font ids to faces of this context; text in faces the
    /// browser does not know is dropped.
    fn translate(&mut self) {
        let fonts = self
            .fonts
            .get_or_insert_with(FontContext::with_system_fonts);
        let mut out = DisplayList::new();
        for item in self.raw_list.items() {
            match item {
                DisplayItem::Text {
                    x,
                    y,
                    scale,
                    color,
                    run,
                } => {
                    let mapped = *self.font_map.entry(run.font.0).or_insert_with(|| {
                        self.face_sources
                            .get(&run.font.0)
                            .and_then(|(path, index)| fonts.font_for_source(path, *index))
                    });
                    if let Some(font) = mapped {
                        out.push(DisplayItem::Text {
                            x: *x,
                            y: *y,
                            scale: *scale,
                            color: *color,
                            run: GlyphRun {
                                font,
                                size: run.size,
                                glyphs: run.glyphs.clone(),
                            },
                        });
                    }
                }
                other => out.push(other.clone()),
            }
        }
        self.list = out;
    }

    pub fn glyph_rasterizer(&mut self) -> &mut dyn rustlite_paint::GlyphRasterizer {
        self.fonts()
    }

    pub fn subresource_requests(&mut self) -> Vec<(Destination, Url, Vec<NodeId>)> {
        let Some(Payload::Requests(list)) = self.call(Request::SubresourceRequests) else {
            return Vec::new();
        };
        list.into_iter()
            .filter_map(|(d, url, nodes)| {
                let url = Url::parse(&url).ok()?;
                // The browser's fetch policy applies again: only http(s).
                matches!(url.scheme(), "http" | "https").then(|| {
                    (
                        d,
                        url,
                        nodes.into_iter().filter_map(NodeId::from_raw).collect(),
                    )
                })
            })
            .collect()
    }

    pub fn set_external_stylesheet(&mut self, node: NodeId, css: &str) {
        self.call(Request::SetStylesheet {
            node: node.to_raw(),
            css: css.to_owned(),
        });
    }

    pub fn set_image_bytes(&mut self, node: NodeId, bytes: &[u8]) -> bool {
        matches!(
            self.call(Request::SetImage {
                node: node.to_raw(),
                bytes: bytes.to_vec(),
            }),
            Some(Payload::Bool(true))
        )
    }

    pub fn set_script_source(&mut self, node: NodeId, source: Option<String>) {
        self.call(Request::SetScript {
            node: node.to_raw(),
            source,
        });
    }

    pub fn link_at(&mut self, x: f32, y: f32) -> Option<Url> {
        if let Some(p) = self.fallback_mut() {
            return p.link_at(x, y);
        }
        match self.call(Request::LinkAt { x, y }) {
            Some(Payload::Url(Some(u))) => Url::parse(&u).ok(),
            _ => None,
        }
    }

    fn input(&mut self, req: Request) -> InputOutcome {
        match self.call(req) {
            Some(Payload::Input { consumed, navigate }) => InputOutcome {
                consumed,
                navigate: navigate.and_then(|u| Url::parse(&u).ok()),
            },
            _ => InputOutcome::default(),
        }
    }

    pub fn click(&mut self, x: f32, y: f32) -> InputOutcome {
        if let Some(p) = self.fallback_mut() {
            return p.click(x, y);
        }
        self.input(Request::Click { x, y })
    }

    pub fn key_down(&mut self, key: &PageKey) -> InputOutcome {
        if self.fallback.is_some() {
            return InputOutcome::default();
        }
        self.input(Request::KeyDown(key.clone()))
    }

    pub fn key_up(&mut self, key: &PageKey) {
        if self.fallback.is_none() {
            self.call(Request::KeyUp(key.clone()));
        }
    }

    pub fn focused(&self) -> Option<NodeId> {
        if self.fallback.is_some() {
            return None;
        }
        self.state.focused.and_then(NodeId::from_raw)
    }

    pub fn next_timer(&self) -> Option<Instant> {
        self.next_timer
    }

    /// Runs the renderer's due timers if the deadline has passed.
    pub fn run_timers(&mut self, now: Instant) -> bool {
        if self.next_timer.is_none_or(|t| t > now) {
            return false;
        }
        let before = self.state.mutation_count;
        self.call(Request::RunTimers);
        self.state.mutation_count != before
    }

    pub fn wants_animation_frame(&self) -> bool {
        self.state.wants_frame
    }

    pub fn run_animation_frame(&mut self, _now: Instant) -> bool {
        let before = self.state.mutation_count;
        self.call(Request::RunAnimationFrame);
        self.state.mutation_count != before
    }

    pub fn find(&mut self, query: &str) -> Vec<FindMatch> {
        if let Some(p) = self.fallback_mut() {
            return p.find(query);
        }
        match self.call(Request::Find(query.to_owned())) {
            Some(Payload::Matches(m)) => m,
            _ => Vec::new(),
        }
    }

    pub fn memory_report(&mut self) -> MemoryReport {
        if let Some(p) = self.fallback_mut() {
            return p.memory_report();
        }
        match self.call(Request::MemoryReport) {
            Some(Payload::Memory(m)) => m,
            _ => MemoryReport::default(),
        }
    }

    pub fn console(&mut self) -> Vec<String> {
        match self.call(Request::Console) {
            Some(Payload::Console(c)) => c,
            _ => Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_round_trip() {
        let reqs = [
            Request::Init {
                width: 10,
                height: 20,
                scale: 1.5,
                javascript: false,
            },
            Request::LoadHtml {
                url: "https://a.test/".into(),
                html: "<p>x".into(),
                base_dir: Some("C:/x".into()),
            },
            Request::SetImage {
                node: 7,
                bytes: vec![1, 2, 3],
            },
            Request::KeyDown(PageKey::Text("é".into())),
            Request::KeyUp(PageKey::Tab { shift: true }),
            Request::Find("needle".into()),
            Request::Console,
        ];
        for r in reqs {
            assert_eq!(Request::decode(&r.encode()), Ok(r));
        }
        assert!(Request::decode(&[99]).is_err());
        assert!(Request::decode(&[6, 0]).is_err(), "trailing bytes");
    }

    #[test]
    fn frames_round_trip_with_images_and_fonts() {
        let image = Rc::new(ImageData::new(1, 1, vec![1, 2, 3, 4]).unwrap());
        let mut list = DisplayList::new();
        list.fill_rect(Rect::new(1, 2, 3, 4), Color::rgb(5, 6, 7));
        list.push(DisplayItem::Image {
            rect: Rect::new(0, 0, 1, 1),
            image: image.clone(),
        });
        list.push(DisplayItem::Text {
            x: 1.0,
            y: 2.0,
            scale: 1.0,
            color: Color::WHITE,
            run: GlyphRun {
                font: FontId(3),
                size: 16.0,
                glyphs: vec![PositionedGlyph {
                    id: 42,
                    x: 0.5,
                    y: 0.0,
                }]
                .into(),
            },
        });
        let mut images = HashMap::new();
        images.insert(Rc::as_ptr(&image) as usize, (1, image.clone()));
        let payload = Payload::Frame(Frame {
            list,
            faces: vec![(3, "C:/f.ttf".into(), 0)],
            images: vec![(1, image)],
        });
        let mut w = Writer::default();
        payload.encode(&mut w, &images);
        let decoded = Payload::decode(&mut Reader::new(&w.buf), &HashMap::new()).unwrap();
        assert_eq!(decoded, payload);
    }

    #[test]
    fn hostile_frames_are_rejected() {
        // An image claiming 100000×100000 pixels.
        let mut w = Writer::default();
        w.u8(2);
        w.len(0);
        w.len(1);
        w.u32(1);
        w.u32(100_000);
        w.u32(100_000);
        w.bytes(&[]);
        assert!(Payload::decode(&mut Reader::new(&w.buf), &HashMap::new()).is_err());
        // A display list claiming 4 billion items.
        let mut w = Writer::default();
        w.u8(2);
        w.len(0);
        w.len(0);
        w.u32(u32::MAX);
        assert!(Payload::decode(&mut Reader::new(&w.buf), &HashMap::new()).is_err());
    }
}
