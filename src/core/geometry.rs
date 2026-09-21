use std::ops::Sub;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point<T> {
    pub x: T,
    pub y: T,
}

impl<T> Point<T> {
    pub fn new(x: T, y: T) -> Self {
        Self { x, y }
    }
}

impl<T> Default for Point<T>
where
    T: Default,
{
    fn default() -> Self {
        Self::new(T::default(), T::default())
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Size<T> {
    pub width: T,
    pub height: T,
}

impl<T> Size<T> {
    pub fn new(width: T, height: T) -> Self {
        Self { width, height }
    }
}

impl<T> Default for Size<T>
where
    T: Default,
{
    fn default() -> Self {
        Self::new(T::default(), T::default())
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect<T> {
    pub origin: Point<T>,
    pub size: Size<T>,
}

impl<T> Rect<T> {
    pub fn new(origin: Point<T>, size: Size<T>) -> Self {
        Self { origin, size }
    }

    pub fn from_xywh(x: T, y: T, width: T, height: T) -> Self {
        Self::new(Point::new(x, y), Size::new(width, height))
    }

    pub fn set_pos(mut self, pos: Point<T>) -> Self {
        self.origin = pos;
        self
    }

    pub fn resize(mut self, size: Size<T>) -> Self {
        self.size = size;
        self
    }
}

impl<T> Default for Rect<T>
where
    T: Default,
{
    fn default() -> Self {
        Self::new(Point::default(), Size::default())
    }
}

impl<T> Rect<T>
where
    T: Sub<Output = T> + Clone,
{
    pub fn from_xyxy(x1: T, y1: T, x2: T, y2: T) -> Self {
        Self::new(
            Point::new(x1.clone(), y1.clone()),
            Size::new(x2 - x1, y2 - y1),
        )
    }
}

macro_rules! impl_rect_is_empty_for_numbers {
    ($($t:ty),*) => {
        $(
            impl Rect<$t> {
                pub fn is_empty(&self) -> bool {
                    self.size.width <= 0 as $t || self.size.height <= 0 as $t
                }
            }
        )*
    };
}

impl_rect_is_empty_for_numbers!(
    i8, i16, i32, i64, i128, isize, u8, u16, u32, u64, u128, usize, f32, f64
);

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Spacing<T> {
    pub top: T,
    pub right: T,
    pub bottom: T,
    pub left: T,
}

impl<T> Spacing<T> {
    pub fn new(top: T, right: T, bottom: T, left: T) -> Self {
        Self {
            top,
            right,
            bottom,
            left,
        }
    }
}

impl<T> Spacing<T>
where
    T: Clone,
{
    pub fn axis(vertical: T, horizontal: T) -> Self {
        Self::new(vertical.clone(), horizontal.clone(), vertical, horizontal)
    }

    pub fn uniform(value: T) -> Self {
        Self::new(value.clone(), value.clone(), value.clone(), value)
    }
}

impl<T> Spacing<T>
where
    T: Default,
{
    pub fn default() -> Self {
        Self::new(T::default(), T::default(), T::default(), T::default())
    }
}

macro_rules! impl_spacing_expand_shrink_for_numbers {
    ($($t:ty),*) => {
        $(
            impl Spacing<$t> {
                pub fn expand(mut self, other: Self) -> Self {
                    self.top = self.top + other.top;
                    self.right = self.right + other.right;
                    self.bottom = self.bottom + other.bottom;
                    self.left = self.left + other.left;
                    self
                }

                pub fn shrink(mut self, other: Self) -> Self {
                    self.top = (self.top - other.top).max(0 as $t);
                    self.right = (self.right - other.right).max(0 as $t);
                    self.bottom = (self.bottom - other.bottom).max(0 as $t);
                    self.left = (self.left - other.left).max(0 as $t);
                    self
                }
            }
        )*
    };
}

impl_spacing_expand_shrink_for_numbers!(
    i8, i16, i32, i64, i128, isize, u8, u16, u32, u64, u128, usize, f32, f64
);
