use super::geometry::*;

#[test]
fn anchor_top_left_keeps_top_edge_invariant_across_compact_and_full_transitions() {
    let primary_height = 982.0;
    // Top-left at logical (120.5, 91.5) with size 400x700 -> bottom = 791.5 -> appkit_y = 190.5
    let full_frame = NSRect {
        origin: NSPoint { x: 120.5, y: 190.5 },
        size: NSSize {
            width: 400.0,
            height: 700.0,
        },
    };
    assert_eq!(
        appkit_frame_to_top_left(full_frame, primary_height),
        LogicalPoint { x: 121, y: 92 }
    );

    let collapse_req = validate_request(&AnimateWindowFrameRequest {
        width: Some(400.0),
        height: 86.0,
        min_width: 400.0,
        min_height: 82.0,
        placement: WindowFramePlacement::AnchorTopLeft,
        target_x: None,
        target_y: None,
        duration_ms: 185,
        pinned: Some(true),
    })
    .unwrap();

    let compact_frame =
        compute_target_frame(full_frame, full_frame.size, primary_height, collapse_req);
    // Exact floating-point top-left anchor: origin.x unchanged, origin.y + height unchanged!
    assert_eq!(compact_frame.origin.x, 120.5);
    assert_eq!(
        compact_frame.origin.y + compact_frame.size.height,
        full_frame.origin.y + full_frame.size.height
    );
    assert_eq!(compact_frame.size.height, 86.0);

    // Drawer height expansion with width: None preserves exact width (even custom width):
    let wide_compact = NSRect {
        origin: compact_frame.origin,
        size: NSSize {
            width: 460.0,
            height: 86.0,
        },
    };
    let drawer_req = validate_request(&AnimateWindowFrameRequest {
        width: None,
        height: 240.0,
        min_width: 400.0,
        min_height: 82.0,
        placement: WindowFramePlacement::AnchorTopLeft,
        target_x: None,
        target_y: None,
        duration_ms: 145,
        pinned: None,
    })
    .unwrap();
    let drawer_frame =
        compute_target_frame(wide_compact, wide_compact.size, primary_height, drawer_req);
    assert_eq!(drawer_frame.size.width, 460.0);
    assert_eq!(drawer_frame.size.height, 240.0);
    assert_eq!(
        drawer_frame.origin.y + drawer_frame.size.height,
        wide_compact.origin.y + wide_compact.size.height
    );
}

#[test]
fn restore_position_round_trips_top_left_coordinates_including_secondary_displays() {
    let primary_height = 982.0;
    // Secondary display positioned to the right (x = 1800) and slightly above primary (y = -120):
    let req = validate_request(&AnimateWindowFrameRequest {
        width: Some(460.0),
        height: 90.0,
        min_width: 400.0,
        min_height: 82.0,
        placement: WindowFramePlacement::RestorePosition,
        target_x: Some(1800.0),
        target_y: Some(-120.0),
        duration_ms: 185,
        pinned: Some(false),
    })
    .unwrap();
    let current = NSRect {
        origin: NSPoint { x: 100.0, y: 100.0 },
        size: NSSize {
            width: 400.0,
            height: 700.0,
        },
    };
    let target = compute_target_frame(current, current.size, primary_height, req);
    assert_eq!(target.origin.x, 1800.0);
    assert_eq!(target.origin.y, primary_height - (-120.0) - 90.0);
    assert_eq!(
        appkit_frame_to_top_left(target, primary_height),
        LogicalPoint { x: 1800, y: -120 }
    );
}

#[test]
fn request_validation_clamps_duration_and_rejects_unknown_or_invalid_fields() {
    let valid = validate_request(&AnimateWindowFrameRequest {
        width: Some(400.0),
        height: 700.0,
        min_width: 400.0,
        min_height: 480.0,
        placement: WindowFramePlacement::AnchorTopLeft,
        target_x: None,
        target_y: None,
        duration_ms: 5000,
        pinned: None,
    })
    .unwrap();
    assert_eq!(valid.duration_ms, MAX_DURATION_MS);

    assert!(
        validate_request(&AnimateWindowFrameRequest {
            width: Some(100.0),
            height: 700.0,
            min_width: 400.0,
            min_height: 480.0,
            placement: WindowFramePlacement::AnchorTopLeft,
            target_x: None,
            target_y: None,
            duration_ms: 185,
            pinned: None,
        })
        .is_err()
    );
    assert!(
        validate_request(&AnimateWindowFrameRequest {
            width: Some(400.0),
            height: f64::NAN,
            min_width: 400.0,
            min_height: 480.0,
            placement: WindowFramePlacement::AnchorTopLeft,
            target_x: None,
            target_y: None,
            duration_ms: 185,
            pinned: None,
        })
        .is_err()
    );

    // `deny_unknown_fields` rejects unexpected IPC keys:
    let bad_wire = serde_json::from_str::<AnimateWindowFrameRequest>(
        r#"{"width":400,"height":700,"minWidth":400,"minHeight":480,"placement":"anchorTopLeft","durationMs":185,"extra":"nope"}"#,
    );
    assert!(bad_wire.is_err());
}

#[test]
fn collection_behavior_toggles_all_spaces_without_losing_unrelated_flags() {
    const MANAGED: usize = 1 << 2;
    const FULL_SCREEN_PRIMARY: usize = 1 << 7;
    let baseline = MANAGED | FULL_SCREEN_PRIMARY;

    // Pinning adds CanJoinAllSpaces while keeping existing managed/full-screen flags:
    let pinned = next_collection_behavior(baseline, true);
    assert_eq!(
        pinned,
        baseline | NS_WINDOW_COLLECTION_BEHAVIOR_CAN_JOIN_ALL_SPACES
    );
    assert_eq!(next_collection_behavior(pinned, true), pinned);

    // Enabling CanJoinAllSpaces clears mutually exclusive MoveToActiveSpace:
    let with_move = baseline | NS_WINDOW_COLLECTION_BEHAVIOR_MOVE_TO_ACTIVE_SPACE;
    let pinned_from_move = next_collection_behavior(with_move, true);
    assert_eq!(
        pinned_from_move,
        baseline | NS_WINDOW_COLLECTION_BEHAVIOR_CAN_JOIN_ALL_SPACES
    );
    assert_eq!(
        pinned_from_move & NS_WINDOW_COLLECTION_BEHAVIOR_MOVE_TO_ACTIVE_SPACE,
        0
    );

    // Unpinning removes only CanJoinAllSpaces and preserves all other flags:
    let unpinned = next_collection_behavior(pinned, false);
    assert_eq!(unpinned, baseline);
    assert_eq!(next_collection_behavior(unpinned, false), baseline);
}
