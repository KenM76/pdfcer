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
    assert_eq!(out.recolor(&mut []), None);
}
