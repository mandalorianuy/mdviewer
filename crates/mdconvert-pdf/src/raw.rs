use mdconvert_core::DocumentMetadata;

#[derive(Debug, Clone, PartialEq)]
pub struct RawDocument {
    pub metadata: DocumentMetadata,
    pub pages: Vec<RawPage>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RawRect {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

impl RawRect {
    pub(crate) fn try_new(left: f32, top: f32, right: f32, bottom: f32) -> Option<Self> {
        if [left, top, right, bottom]
            .iter()
            .all(|value| value.is_finite())
        {
            Some(Self {
                left: left.min(right),
                top: top.min(bottom),
                right: left.max(right),
                bottom: top.max(bottom),
            })
        } else {
            None
        }
    }

    pub fn width(&self) -> f32 {
        self.right - self.left
    }

    pub fn height(&self) -> f32 {
        self.bottom - self.top
    }

    pub(crate) fn union(self, other: Self) -> Self {
        Self {
            left: self.left.min(other.left),
            top: self.top.min(other.top),
            right: self.right.max(other.right),
            bottom: self.bottom.max(other.bottom),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RawPage {
    pub number: u32,
    pub width: f32,
    pub height: f32,
    pub rotation_degrees: i16,
    pub glyphs: Vec<RawGlyph>,
    pub words: Vec<RawWord>,
    pub images: Vec<RawImage>,
    pub links: Vec<RawLink>,
    pub rules: Vec<RawRule>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RawGlyph {
    pub text: String,
    pub bounds: RawRect,
    pub font_size: f32,
    pub font_name: Option<String>,
    pub font_weight: Option<u16>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RawWord {
    pub text: String,
    pub bounds: RawRect,
    pub glyph_start: usize,
    pub glyph_end: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RawImage {
    pub index: u32,
    pub bounds: RawRect,
    pub pixel_width: u32,
    pub pixel_height: u32,
    pub rgba: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RawLink {
    pub bounds: RawRect,
    pub target: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleKind {
    Line,
    Rectangle,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RawRule {
    pub kind: RuleKind,
    pub bounds: RawRect,
    pub stroke_width: f32,
}
