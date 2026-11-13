use super::*;

fn paths(n: usize) -> Vec<PathBuf> {
    (0..n).map(|i| PathBuf::from(format!("f{i}.pdf"))).collect()
}

#[test]
fn every_file_prints_and_the_first_failure_sets_the_code() {
    let mut seen = Vec::new();
    let code = print_each(&paths(3), None, |p| {
        seen.push(p.to_path_buf());
        match p.to_str() {
            Some("f1.pdf") => 5,
            Some("f2.pdf") => 7,
            _ => exit::SUCCESS,
        }
    });
    assert_eq!(seen, paths(3));
    assert_eq!(code, 5);
}

#[test]
fn all_succeeding_exits_zero() {
    assert_eq!(
        print_each(&paths(2), None, |_| exit::SUCCESS),
        exit::SUCCESS
    );
}

#[test]
fn to_file_refuses_several_inputs_before_printing_any() {
    let mut called = false;
    let code = print_each(&paths(2), Some(Path::new("out.pdf")), |_| {
        called = true;
        exit::SUCCESS
    });
    assert_eq!(code, USAGE);
    assert!(!called);
    assert_eq!(
        print_each(&paths(1), Some(Path::new("out.pdf")), |_| exit::SUCCESS),
        exit::SUCCESS
    );
}

#[test]
fn colour_keeps_the_bitmap_and_says_nothing() {
    let out = PrintOutput::default();
    assert!(out.recolor(&mut []).unwrap().is_empty());
}

#[test]
fn auto_edges_are_seamless_below_300_dpi_and_hard_at_and_above() {
    use pdfcer_render::EdgeAntialiasing as E;
    assert_eq!(EdgeAaArg::default(), EdgeAaArg::Auto);
    assert_eq!(EdgeAaArg::Auto.at_dpi(108), E::Supersampled);
    assert_eq!(EdgeAaArg::Auto.at_dpi(299), E::Supersampled);
    assert_eq!(EdgeAaArg::Auto.at_dpi(300), E::Off);
    assert_eq!(EdgeAaArg::On.at_dpi(600), E::On);
}

/// A 300 x 200 page: a blue band across its top as page content, a printable
/// red text widget and a printable green square markup, each with an `/AP`.
fn band_and_widget() -> EditSession {
    let form = |rgb: &str, w: u32, h: u32| {
        let body = format!("{rgb} rg 0 0 {w} {h} re f");
        format!(
            "<< /Type /XObject /Subtype /Form /BBox [0 0 {w} {h}] /Length {} >>\nstream\n{body}\nendstream",
            body.len()
        )
    };
    let band = "0 0 1 rg 0 150 300 50 re f";
    let bodies = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 200] /Resources << >> \
         /Contents 4 0 R /Annots [5 0 R 7 0 R] >>"
            .to_owned(),
        format!("<< /Length {} >>\nstream\n{band}\nendstream", band.len()),
        "<< /Type /Annot /Subtype /Widget /FT /Tx /T (a) /Rect [100 60 200 110] /F 4 \
         /AP << /N 6 0 R >> >>"
            .to_owned(),
        form("1 0 0", 100, 50),
        "<< /Type /Annot /Subtype /Square /Rect [10 60 60 110] /F 4 /AP << /N 8 0 R >> >>"
            .to_owned(),
        form("0 1 0", 50, 50),
    ];
    let mut buf = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in bodies.iter().enumerate() {
        offsets.push(buf.len());
        buf.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref_at = buf.len();
    let size = bodies.len() + 1;
    buf.extend_from_slice(format!("xref\n0 {size}\n0000000000 65535 f \n").as_bytes());
    for off in &offsets {
        buf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    buf.extend_from_slice(
        format!("trailer\n<< /Size {size} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n")
            .as_bytes(),
    );
    EditSession::new(pdfcer_core::document::Document::from_bytes(buf).unwrap())
}

/// Blue, red, green and near-black pixel counts of page 0, printed
/// fields-only through `out`.
fn fields_only_print(out: &PrintOutput) -> [usize; 4] {
    let mut session = band_and_widget();
    let scope = out
        .fields_only_scope(&mut session, CommentsArg::FieldsOnly)
        .unwrap();
    out.stamp(&mut session, "job").unwrap();
    let page = session.pages().unwrap().remove(0);
    let options = pdfcer_render::RenderOptions::default()
        .with_annotation_scope(scope.to_scope())
        .with_print_intent(true);
    let rendered =
        pdfcer_render::render_page_with_view(&session.view(), &page, 2.0, &options).unwrap();
    let mut counts = [0; 4];
    for px in rendered.pixmap.data().as_chunks::<4>().0 {
        let (r, g, b) = (px[0], px[1], px[2]);
        let slot = match (r > 200, g > 200, b > 200) {
            (false, false, true) => 0,
            (true, false, false) => 1,
            (false, true, false) => 2,
            _ if u16::from(r) + u16::from(g) + u16::from(b) < 200 => 3,
            _ => continue,
        };
        counts[slot] += 1;
    }
    counts
}

#[test]
fn fields_only_with_marks_prints_the_widget_and_the_marks_not_the_page() {
    // Every mark but the colour bars, whose patches include the blue the
    // band is detected by.
    let out = PrintOutput {
        marks: vec![
            MarkArg::Trim,
            MarkArg::Bleed,
            MarkArg::Registration,
            MarkArg::Info,
        ],
        ..PrintOutput::default()
    };
    let [blue, red, green, dark] = fields_only_print(&out);
    assert_eq!((blue, green), (0, 0), "page content and markup stay off");
    assert!(red > 1000, "the widget prints: {red}");
    assert!(dark > 0, "the marks print");
}

#[test]
fn fields_only_with_a_header_and_footer_prints_both_and_not_the_page() {
    for (header, footer) in [(Some("Top line"), None), (None, Some("Bottom line"))] {
        let line = |t: Option<&str>| t.map(|t| (t.to_owned(), AlignArg::Center));
        let out = PrintOutput {
            header: line(header),
            footer: line(footer),
            ..PrintOutput::default()
        };
        let [blue, red, green, dark] = fields_only_print(&out);
        assert_eq!((blue, green), (0, 0), "page content and markup stay off");
        assert!(red > 1000, "the widget prints: {red}");
        assert!(dark > 0, "the running text prints: {header:?} {footer:?}");
    }
}

#[test]
fn fields_only_without_decoration_keeps_the_scope_and_the_file() {
    let mut session = band_and_widget();
    let out = PrintOutput::default();
    let scope = out
        .fields_only_scope(&mut session, CommentsArg::FieldsOnly)
        .unwrap();
    assert_eq!(scope, CommentsArg::FieldsOnly);
    assert_eq!(session.undo_kind(), None);
    let marked = PrintOutput {
        marks: vec![MarkArg::All],
        ..PrintOutput::default()
    };
    let scope = marked
        .fields_only_scope(&mut session, CommentsArg::Markups)
        .unwrap();
    assert_eq!(scope, CommentsArg::Markups, "only fields-only is rewritten");
    assert_eq!(session.undo_kind(), None);
}
