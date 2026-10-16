/// A position in device pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

impl Point {
    pub const fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }
}

/// A non-negative extent in device pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Size {
    pub width: u32,
    pub height: u32,
}

impl Size {
    pub const fn new(width: u32, height: u32) -> Self {
        Self { width, height }
    }

    pub const fn is_empty(self) -> bool {
        self.width == 0 || self.height == 0
    }
}

/// An axis-aligned rectangle in device pixels.
///
/// `x`/`y` may be negative (content scrolled partly off-screen); width and
/// height are always non-negative.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl Rect {
    pub const fn new(x: i32, y: i32, width: u32, height: u32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub const fn from_size(size: Size) -> Self {
        Self::new(0, 0, size.width, size.height)
    }

    pub const fn is_empty(self) -> bool {
        self.width == 0 || self.height == 0
    }

    /// Exclusive right edge, computed in `i64` so it never overflows.
    pub const fn right(self) -> i64 {
        self.x as i64 + self.width as i64
    }

    /// Exclusive bottom edge, computed in `i64` so it never overflows.
    pub const fn bottom(self) -> i64 {
        self.y as i64 + self.height as i64
    }

    pub fn contains(self, point: Point) -> bool {
        let (x, y) = (i64::from(point.x), i64::from(point.y));
        x >= i64::from(self.x) && y >= i64::from(self.y) && x < self.right() && y < self.bottom()
    }

    /// Returns the overlapping region, or `None` if the rectangles do not overlap.
    pub fn intersect(self, other: Rect) -> Option<Rect> {
        let left = self.x.max(other.x);
        let top = self.y.max(other.y);
        let right = self.right().min(other.right());
        let bottom = self.bottom().min(other.bottom());
        let width = u32::try_from(right - i64::from(left)).ok()?;
        let height = u32::try_from(bottom - i64::from(top)).ok()?;
        let rect = Rect::new(left, top, width, height);
        (!rect.is_empty()).then_some(rect)
    }

    /// Shrinks the rectangle by `amount` on every side, saturating at zero size.
    pub fn inset(self, amount: u32) -> Rect {
        let twice = amount.saturating_mul(2);
        Rect::new(
            self.x.saturating_add_unsigned(amount),
            self.y.saturating_add_unsigned(amount),
            self.width.saturating_sub(twice),
            self.height.saturating_sub(twice),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intersect_overlapping() {
        let a = Rect::new(0, 0, 10, 10);
        let b = Rect::new(5, -5, 10, 10);
        assert_eq!(a.intersect(b), Some(Rect::new(5, 0, 5, 5)));
    }

    #[test]
    fn intersect_touching_edges_is_empty() {
        let a = Rect::new(0, 0, 10, 10);
        let b = Rect::new(10, 0, 10, 10);
        assert_eq!(a.intersect(b), None);
    }

    #[test]
    fn intersect_disjoint_is_empty() {
        let a = Rect::new(0, 0, 10, 10);
        let b = Rect::new(-50, 20, 10, 10);
        assert_eq!(a.intersect(b), None);
    }

    #[test]
    fn intersect_extreme_coordinates_does_not_overflow() {
        let a = Rect::new(i32::MIN, i32::MIN, u32::MAX, u32::MAX);
        // `a` spans [i32::MIN, i32::MAX), so it overlaps `b` in exactly one pixel.
        let b = Rect::new(i32::MAX - 1, i32::MAX - 1, u32::MAX, u32::MAX);
        assert_eq!(
            a.intersect(b),
            Some(Rect::new(i32::MAX - 1, i32::MAX - 1, 1, 1))
        );
        let d = Rect::new(i32::MAX, 0, u32::MAX, 1);
        assert_eq!(a.intersect(d), None);
        let c = Rect::new(0, 0, 8, 8);
        assert_eq!(a.intersect(c), Some(Rect::new(0, 0, 8, 8)));
    }

    #[test]
    fn contains_is_half_open() {
        let r = Rect::new(2, 2, 3, 3);
        assert!(r.contains(Point::new(2, 2)));
        assert!(r.contains(Point::new(4, 4)));
        assert!(!r.contains(Point::new(5, 4)));
    }

    #[test]
    fn inset_saturates() {
        assert_eq!(Rect::new(0, 0, 4, 4).inset(3), Rect::new(3, 3, 0, 0));
    }
}
