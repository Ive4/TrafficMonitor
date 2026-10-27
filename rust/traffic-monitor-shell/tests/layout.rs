use traffic_monitor_shell::{
    Alignment, LayoutError, LayoutRequest, Orientation, Rect, Size, layout_panel,
    taskbar_orientation,
};

#[test]
fn infers_horizontal_vertical_and_square_taskbars_without_signed_overflow() {
    assert_eq!(
        taskbar_orientation(Rect {
            left: -1920,
            top: 1050,
            right: 0,
            bottom: 1080
        }),
        Ok(Orientation::Horizontal)
    );
    assert_eq!(
        taskbar_orientation(Rect {
            left: -48,
            top: 0,
            right: 0,
            bottom: 1080
        }),
        Ok(Orientation::Vertical)
    );
    assert_eq!(
        taskbar_orientation(Rect {
            left: 0,
            top: 0,
            right: 48,
            bottom: 48
        }),
        Ok(Orientation::Horizontal)
    );
    assert_eq!(
        taskbar_orientation(Rect {
            left: i32::MIN,
            top: 0,
            right: i32::MAX,
            bottom: 48
        }),
        Ok(Orientation::Horizontal)
    );
    assert_eq!(
        taskbar_orientation(Rect {
            left: 0,
            top: 0,
            right: 0,
            bottom: 48
        }),
        Err(LayoutError::InvalidBounds)
    );
}

fn request() -> LayoutRequest {
    LayoutRequest {
        available: Rect {
            left: 0,
            top: 0,
            right: 1000,
            bottom: 48,
        },
        desired_dip: Size {
            width: 100,
            height: 32,
        },
        dpi: 96,
        orientation: Orientation::Horizontal,
        alignment: Alignment::End,
        margin_dip: 2,
    }
}

#[test]
fn horizontal_end_centers_cross_axis_and_preserves_margins() {
    assert_eq!(
        layout_panel(request()),
        Ok(Rect {
            left: 898,
            top: 8,
            right: 998,
            bottom: 40
        })
    );
}

#[test]
fn horizontal_start_uses_available_region_not_entire_taskbar() {
    let mut input = request();
    input.available.left = 200;
    input.available.right = 500;
    input.alignment = Alignment::Start;
    assert_eq!(
        layout_panel(input),
        Ok(Rect {
            left: 202,
            top: 8,
            right: 302,
            bottom: 40
        })
    );
}

#[test]
fn vertical_end_centers_width_without_rotating_content() {
    let mut input = request();
    input.available = Rect {
        left: 0,
        top: 50,
        right: 64,
        bottom: 1050,
    };
    input.desired_dip = Size {
        width: 32,
        height: 100,
    };
    input.orientation = Orientation::Vertical;
    assert_eq!(
        layout_panel(input),
        Ok(Rect {
            left: 16,
            top: 948,
            right: 48,
            bottom: 1048
        })
    );
    input.alignment = Alignment::Start;
    assert_eq!(
        layout_panel(input),
        Ok(Rect {
            left: 16,
            top: 52,
            right: 48,
            bottom: 152
        })
    );
}

#[test]
fn scales_desired_size_and_margin_at_150_percent() {
    let mut input = request();
    input.dpi = 144;
    input.available.bottom = 72;
    assert_eq!(
        layout_panel(input),
        Ok(Rect {
            left: 847,
            top: 12,
            right: 997,
            bottom: 60
        })
    );
}

#[test]
fn fractional_dip_rounding_uses_nearest_pixel() {
    let mut input = request();
    input.dpi = 120;
    input.desired_dip = Size {
        width: 3,
        height: 1,
    };
    input.margin_dip = 1;
    assert_eq!(
        layout_panel(input),
        Ok(Rect {
            left: 995,
            top: 23,
            right: 999,
            bottom: 24
        })
    );
}

#[test]
fn negative_coordinates_are_valid_in_a_shared_space() {
    let mut input = request();
    input.available = Rect {
        left: -1920,
        top: -48,
        right: 0,
        bottom: 0,
    };
    input.alignment = Alignment::Start;
    assert_eq!(
        layout_panel(input),
        Ok(Rect {
            left: -1918,
            top: -40,
            right: -1818,
            bottom: -8
        })
    );
}

#[test]
fn exact_fit_is_allowed() {
    let mut input = request();
    input.available = Rect {
        left: 0,
        top: 0,
        right: 104,
        bottom: 36,
    };
    assert_eq!(
        layout_panel(input),
        Ok(Rect {
            left: 2,
            top: 2,
            right: 102,
            bottom: 34
        })
    );
}

#[test]
fn refuses_main_axis_overlap_and_cross_axis_overflow() {
    let mut input = request();
    input.available.right = 103;
    assert_eq!(layout_panel(input), Err(LayoutError::NoSpace));
    input.available.right = 1000;
    input.available.bottom = 35;
    assert_eq!(layout_panel(input), Err(LayoutError::NoSpace));
}

#[test]
fn rejects_empty_inverted_bounds_zero_dpi_and_empty_panel() {
    let mut input = request();
    input.available.right = 0;
    assert_eq!(layout_panel(input), Err(LayoutError::InvalidBounds));
    input.available.right = -1;
    assert_eq!(layout_panel(input), Err(LayoutError::InvalidBounds));
    input = request();
    input.dpi = 0;
    assert_eq!(layout_panel(input), Err(LayoutError::InvalidDpi));
    input = request();
    input.desired_dip.height = 0;
    assert_eq!(layout_panel(input), Err(LayoutError::EmptyPanel));
}

#[test]
fn extreme_dpi_and_size_fail_without_overflow() {
    let mut input = request();
    input.dpi = u32::MAX;
    input.desired_dip.width = u32::MAX;
    input.margin_dip = u32::MAX;
    assert_eq!(layout_panel(input), Err(LayoutError::NoSpace));
}

#[test]
fn supports_rectangles_spanning_signed_coordinate_range() {
    let mut input = request();
    input.available = Rect {
        left: i32::MIN,
        top: i32::MIN,
        right: i32::MAX,
        bottom: i32::MAX,
    };
    assert_eq!(
        layout_panel(input),
        Ok(Rect {
            left: 2_147_483_545,
            top: -17,
            right: 2_147_483_645,
            bottom: 15
        })
    );
}
