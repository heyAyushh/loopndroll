use ratatui::layout::Rect;

pub(crate) const TAB_BAR_HEIGHT: u16 = 3;
pub(crate) const FOOTER_HEIGHT: u16 = 2;
pub(crate) const BLOCK_BORDER_WIDTH: u16 = 1;

const BLOCK_BORDER_TOTAL_WIDTH: u16 = BLOCK_BORDER_WIDTH * 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PointerState {
    pub(crate) column: u16,
    pub(crate) row: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct UiGeometry {
    pub(crate) tabs: Rect,
    pub(crate) content: Rect,
    pub(crate) footer: Rect,
}

impl Default for UiGeometry {
    fn default() -> Self {
        Self {
            tabs: Rect::new(0, 0, 0, 0),
            content: Rect::new(0, 0, 0, 0),
            footer: Rect::new(0, 0, 0, 0),
        }
    }
}

pub(crate) fn inner_rect(rect: Rect) -> Option<Rect> {
    if rect.width <= BLOCK_BORDER_TOTAL_WIDTH || rect.height <= BLOCK_BORDER_TOTAL_WIDTH {
        return None;
    }
    Some(Rect::new(
        rect.x.saturating_add(BLOCK_BORDER_WIDTH),
        rect.y.saturating_add(BLOCK_BORDER_WIDTH),
        rect.width.saturating_sub(BLOCK_BORDER_TOTAL_WIDTH),
        rect.height.saturating_sub(BLOCK_BORDER_TOTAL_WIDTH),
    ))
}

pub(crate) fn rect_contains(rect: Rect, pointer: PointerState) -> bool {
    pointer.column >= rect.x
        && pointer.column < rect.x.saturating_add(rect.width)
        && pointer.row >= rect.y
        && pointer.row < rect.y.saturating_add(rect.height)
}
