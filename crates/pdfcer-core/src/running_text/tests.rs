use super::*;

#[test]
fn tokens_fill_per_page() {
    let t = RunningText::new(RunningTextKind::Header, "{page}/{pages} draft {page}");
    assert_eq!(t.label(3, 10), "3/10 draft 3");
}

#[test]
fn check_refuses_empty_unencodable_and_bad_geometry() {
    assert_eq!(
        RunningText::new(RunningTextKind::Footer, "  ").check(),
        Err(RunningTextError::Empty)
    );
    assert_eq!(
        RunningText::new(RunningTextKind::Footer, "\u{65e5}").check(),
        Err(RunningTextError::Unencodable('\u{65e5}'))
    );
    let mut t = RunningText::new(RunningTextKind::Footer, "ok");
    t.font_size = 0.0;
    assert!(matches!(t.check(), Err(RunningTextError::Geometry { .. })));
    t.font_size = 9.0;
    assert_eq!(t.check(), Ok(()));
}

#[test]
fn kind_and_align_pick_the_edge_position() {
    let mut t = RunningText::new(RunningTextKind::Header, "x");
    assert_eq!(t.position(), BatesPosition::TopCenter);
    t.kind = RunningTextKind::Footer;
    t.align = RunningTextAlign::Right;
    assert_eq!(t.position(), BatesPosition::BottomRight);
}
