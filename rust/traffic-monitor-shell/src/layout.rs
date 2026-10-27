/// A rectangle in a single coordinate space. Right and bottom are exclusive.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Size {
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Orientation {
    Horizontal,
    Vertical,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Alignment {
    Start,
    End,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LayoutRequest {
    pub available: Rect,
    pub desired_dip: Size,
    pub dpi: u32,
    pub orientation: Orientation,
    pub alignment: Alignment,
    pub margin_dip: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LayoutError {
    InvalidBounds,
    InvalidDpi,
    EmptyPanel,
    NoSpace,
}

/// Uses the same width/height rule as the original classical taskbar adapter.
pub fn taskbar_orientation(bounds: Rect) -> Result<Orientation, LayoutError> {
    let width = i64::from(bounds.right) - i64::from(bounds.left);
    let height = i64::from(bounds.bottom) - i64::from(bounds.top);
    if width <= 0 || height <= 0 {
        return Err(LayoutError::InvalidBounds);
    }
    Ok(if width >= height {
        Orientation::Horizontal
    } else {
        Orientation::Vertical
    })
}

/// Places a DIP-sized panel inside an explicitly available pixel rectangle.
///
/// This function does not discover free space or reserve taskbar space. The caller
/// supplies a region that is already safe to use; a panel that would overlap its
/// bounds is rejected rather than shrinking or moving another window.
pub fn layout_panel(request: LayoutRequest) -> Result<Rect, LayoutError> {
    let bounds = request.available;
    let width = i64::from(bounds.right) - i64::from(bounds.left);
    let height = i64::from(bounds.bottom) - i64::from(bounds.top);
    if width <= 0 || height <= 0 {
        return Err(LayoutError::InvalidBounds);
    }
    if request.dpi == 0 {
        return Err(LayoutError::InvalidDpi);
    }
    if request.desired_dip.width == 0 || request.desired_dip.height == 0 {
        return Err(LayoutError::EmptyPanel);
    }

    // u32 * u32 plus the rounding offset fits u64, even for untrusted inputs.
    // Keep the result unsigned until bounded by the at-most-u32 coordinate span.
    let pixels = |dip: u32| (u64::from(dip) * u64::from(request.dpi) + 48) / 96;
    let panel_width = pixels(request.desired_dip.width).max(1);
    let panel_height = pixels(request.desired_dip.height).max(1);
    let margin = pixels(request.margin_dip);
    let inset_width = width as u64;
    let inset_height = height as u64;
    if margin > inset_width / 2
        || margin > inset_height / 2
        || panel_width > inset_width - 2 * margin
        || panel_height > inset_height - 2 * margin
    {
        return Err(LayoutError::NoSpace);
    }

    // All three values are bounded by the coordinate span after the fit check.
    let panel_width = panel_width as i64;
    let panel_height = panel_height as i64;
    let margin = margin as i64;
    let main_offset = |span, panel| match request.alignment {
        Alignment::Start => margin,
        Alignment::End => span - margin - panel,
    };
    let (x, y) = match request.orientation {
        Orientation::Horizontal => (main_offset(width, panel_width), (height - panel_height) / 2),
        Orientation::Vertical => ((width - panel_width) / 2, main_offset(height, panel_height)),
    };
    let left = i64::from(bounds.left) + x;
    let top = i64::from(bounds.top) + y;
    // The fit check proves each edge remains inside the original i32 rectangle.
    Ok(Rect {
        left: left as i32,
        top: top as i32,
        right: (left + panel_width) as i32,
        bottom: (top + panel_height) as i32,
    })
}
