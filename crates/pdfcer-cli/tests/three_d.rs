//! `pdfcer 3d-list` / `3d-extract` / `3d-embed` / `3d-mesh` on a synthetic
//! page with one U3D `/3D` annotation and one RichMedia PRC asset.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

fn three_d_pdf(tag: &str) -> PathBuf {
    let bodies = [
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Annots [4 0 R 6 0 R] >>",
        "<< /Type /Annot /Subtype /3D /Rect [0 0 100 100] /3DD 5 0 R /AP << /N 5 0 R >> >>",
        "<< /Type /3D /Subtype /U3D /Filter /ASCIIHexDecode /VA [<< >>] /Length 13 >>\n\
         stream\n55334400C0FFEE>\nendstream",
        "<< /Type /Annot /Subtype /RichMedia /Rect [0 0 1 1] /RichMediaContent \
         << /Configurations [<< /Subtype /3D /Instances [<< /Subtype /3D /Asset 7 0 R >>] >>] >> >>",
        "<< /Type /Filespec /UF (../evil.u3d) /EF << /F 8 0 R >> >>",
        "<< /Type /EmbeddedFile /Length 4 >>\nstream\nPRC!\nendstream",
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
    let path = std::env::temp_dir().join(format!("pdfcer_3d_{tag}_{}.pdf", std::process::id()));
    std::fs::write(&path, buf).unwrap();
    path
}

fn run(args: &[&str]) -> Output {
    Command::new(BIN).args(args).output().unwrap()
}

#[test]
fn both_models_are_listed() {
    let input = three_d_pdf("list");
    let out = run(&["3d-list", input.to_str().unwrap()]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("3d index=0 page=1 format=U3D views=1 poster=yes source=stream\n"),
        "{stdout}"
    );
    assert!(
        stdout.contains(
            "3d index=1 page=1 format=U3D views=0 poster=no source=richmedia name=\"../evil.u3d\"\n"
        ),
        "{stdout}"
    );
    assert!(stdout.contains("count=2\n"), "{stdout}");
}

#[test]
fn extraction_writes_the_decoded_bytes() {
    let input = three_d_pdf("ext");
    let output = input.with_extension("u3d");
    let out = run(&[
        "3d-extract",
        input.to_str().unwrap(),
        "--index",
        "0",
        "-o",
        output.to_str().unwrap(),
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(std::fs::read(&output).unwrap(), b"U3D\0\xC0\xFF\xEE");
    assert!(!String::from_utf8_lossy(&out.stdout).contains("note:"));
}

/// The asset's name says `.u3d`, its bytes say PRC: written anyway, and
/// said out loud.
#[test]
fn a_mislabelled_model_is_disclosed() {
    let input = three_d_pdf("lie");
    let output = input.with_extension("bin");
    let out = run(&[
        "3d-extract",
        input.to_str().unwrap(),
        "--index",
        "1",
        "-o",
        output.to_str().unwrap(),
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(std::fs::read(&output).unwrap(), b"PRC!");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("note: the document declares U3D but the bytes are PRC"),
        "{stdout}"
    );
}

#[test]
fn an_out_of_range_index_is_refused_and_writes_nothing() {
    let input = three_d_pdf("oob");
    let output = input.with_extension("none");
    let out = run(&[
        "3d-extract",
        input.to_str().unwrap(),
        "--index",
        "5",
        "-o",
        output.to_str().unwrap(),
    ]);
    assert_eq!(out.status.code(), Some(9));
    assert!(!output.exists());
    assert!(String::from_utf8_lossy(&out.stderr).contains("has 2"));
}

fn model(tag: &str, bytes: &[u8]) -> PathBuf {
    let path = std::env::temp_dir().join(format!("pdfcer_3d_{tag}_{}.bin", std::process::id()));
    std::fs::write(&path, bytes).unwrap();
    path
}

/// Dry run: the summary, the inferred format and the version note are
/// printed, and nothing is written.
#[test]
fn an_embed_dry_run_discloses_the_inferred_format_and_the_version() {
    let input = three_d_pdf("emb_dry");
    let prc = model("emb_dry", b"PRC\x08\x00\x01");
    let output = input.with_extension("out.pdf");
    let out = run(&[
        "3d-embed",
        input.to_str().unwrap(),
        "--model",
        prc.to_str().unwrap(),
        "--page",
        "1",
        "--rect",
        "10,10,190,190",
        "-o",
        output.to_str().unwrap(),
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("format=PRC bytes=6 poster=placeholder activate=XA"),
        "{stdout}"
    );
    assert!(stdout.contains("applied=0"), "{stdout}");
    assert!(
        stdout.contains("inferred: format PRC from the file's signature\n"),
        "{stdout}"
    );
    assert!(
        stdout.contains("note: the document is PDF 1.7 and PRC needs PDF 2.0"),
        "{stdout}"
    );
    assert!(!output.exists(), "a dry run wrote a file");
}

/// Applied with a stated format: the model lists and extracts back from
/// the written file, nothing is inferred, and the only note says why a U3D
/// model gets the placeholder poster.
#[test]
fn an_applied_embed_round_trips_through_list_and_extract() {
    let input = three_d_pdf("emb_apply");
    let u3d = model("emb_apply", b"U3D\0\x10\x20\x30");
    let output = input.with_extension("out.pdf");
    let out = run(&[
        "3d-embed",
        input.to_str().unwrap(),
        "--model",
        u3d.to_str().unwrap(),
        "--page",
        "1",
        "--rect",
        "10,10,190,190",
        "--format",
        "u3d",
        "--activate",
        "page-visible",
        "--apply",
        "-o",
        output.to_str().unwrap(),
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("activate=PV"), "{stdout}");
    assert!(stdout.contains("poster=placeholder"), "{stdout}");
    assert!(!stdout.contains("inferred:"), "{stdout}");
    let notes: Vec<&str> = stdout.lines().filter(|l| l.starts_with("note:")).collect();
    assert_eq!(
        notes,
        [
            "note: the poster is pdfcer's placeholder drawing: pdfcer decodes only PRC models; this one is U3D"
        ],
        "{stdout}"
    );

    let listed = run(&["3d-list", output.to_str().unwrap()]);
    assert!(
        String::from_utf8_lossy(&listed.stdout).contains("count=3\n"),
        "{}",
        String::from_utf8_lossy(&listed.stdout)
    );
    let extracted = input.with_extension("back.u3d");
    let ext = run(&[
        "3d-extract",
        output.to_str().unwrap(),
        "--index",
        "2",
        "-o",
        extracted.to_str().unwrap(),
    ]);
    assert!(
        ext.status.success(),
        "{}",
        String::from_utf8_lossy(&ext.stderr)
    );
    assert_eq!(std::fs::read(&extracted).unwrap(), b"U3D\0\x10\x20\x30");
}

#[test]
fn a_step_model_is_refused_and_writes_nothing() {
    let input = three_d_pdf("emb_step");
    let step = model("emb_step", b"ISO-10303-21;\nHEADER;");
    let output = input.with_extension("out.pdf");
    let out = run(&[
        "3d-embed",
        input.to_str().unwrap(),
        "--model",
        step.to_str().unwrap(),
        "--page",
        "1",
        "--rect",
        "10,10,190,190",
        "--apply",
        "-o",
        output.to_str().unwrap(),
    ]);
    assert_eq!(out.status.code(), Some(9));
    assert!(!output.exists());
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("STEP"),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// `three_d_pdf` with the synthetic PRC square embedded as model 2.
#[cfg(feature = "3d")]
fn with_prc_square(tag: &str) -> PathBuf {
    with_prc(tag, "square.prc")
}

/// `three_d_pdf` with `fixtures/synthetic/prc/<name>` embedded as model 2.
#[cfg(feature = "3d")]
fn with_prc(tag: &str, name: &str) -> PathBuf {
    let input = three_d_pdf(tag);
    let prc = format!(
        "{}/../../fixtures/synthetic/prc/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    let output = input.with_extension("prc.pdf");
    let out = run(&[
        "3d-embed",
        input.to_str().unwrap(),
        "--model",
        &prc,
        "--page",
        "1",
        "--rect",
        "10,10,190,190",
        "--apply",
        "-o",
        output.to_str().unwrap(),
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    output
}

fn mesh(input: &Path, index: &str, output: &Path, format: &[&str]) -> Output {
    let mut args = vec![
        "3d-mesh",
        input.to_str().unwrap(),
        "--index",
        index,
        "-o",
        output.to_str().unwrap(),
    ];
    args.extend_from_slice(format);
    run(&args)
}

#[cfg(feature = "3d")]
#[test]
fn a_prc_model_meshes_to_stl_by_default() {
    let input = with_prc_square("mesh_stl");
    let output = input.with_extension("stl");
    let out = mesh(&input, "2", &output, &[]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stl = std::fs::read(&output).unwrap();
    assert_eq!(stl.len(), 84 + 2 * 50);
    assert_eq!(u32::from_le_bytes(stl[80..84].try_into().unwrap()), 2);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains(
            "meshed index=2 meshes=1 triangles=2 wires_skipped=0 markup_skipped=0 \
             compressed_rebuilt=0 compressed_skipped=0"
        ),
        "{stdout}"
    );
    assert!(
        stdout.contains("note: part placements are not applied"),
        "{stdout}"
    );
    assert!(!stdout.contains("normals"), "{stdout}");
}

/// The format follows the output's extension unless `--format` says otherwise.
#[cfg(feature = "3d")]
#[test]
fn a_mesh_format_follows_the_output_extension() {
    let input = with_prc_square("mesh_ext");
    let obj = input.with_extension("OBJ");
    assert!(mesh(&input, "2", &obj, &[]).status.success());
    assert!(
        std::fs::read_to_string(&obj)
            .unwrap()
            .contains("f 1//1 2//1 3//1\n")
    );
    let forced = input.with_extension("obj");
    assert!(
        mesh(&input, "2", &forced, &["--format", "stl"])
            .status
            .success()
    );
    assert_eq!(std::fs::read(&forced).unwrap().len(), 84 + 2 * 50);
}

#[cfg(feature = "3d")]
#[test]
fn a_prc_model_meshes_to_obj() {
    let input = with_prc_square("mesh_obj");
    let output = input.with_extension("obj");
    let out = mesh(&input, "2", &output, &["--format", "obj"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let obj = std::fs::read_to_string(&output).unwrap();
    assert!(obj.contains("v 1 1 0\n"), "{obj}");
    assert!(obj.contains("vn 0 0 1\n"), "{obj}");
    assert!(
        obj.contains("f 1//1 2//1 3//1\nf 1//1 3//1 4//1\n"),
        "{obj}"
    );
}

/// The product tree places the square twice, the second copy mirrored in x
/// and moved 5 along it, so its winding is flipped to keep facing out.
#[cfg(feature = "3d")]
#[test]
fn a_prc_assembly_is_written_at_its_placements() {
    let input = with_prc("mesh_assembly", "assembly.prc");
    let output = input.with_extension("obj");
    let out = mesh(&input, "2", &output, &["--format", "obj"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("meshes=2 triangles=4 "), "{stdout}");
    assert!(!stdout.contains("placements are not applied"), "{stdout}");
    let obj = std::fs::read_to_string(&output).unwrap();
    assert!(obj.contains("v 1 1 0\n"), "{obj}");
    assert!(obj.contains("v 4 1 0\n"), "{obj}");
    assert!(
        obj.contains("f 1//1 2//1 3//1\nf 1//1 3//1 4//1\n"),
        "{obj}"
    );
    assert!(
        obj.contains("f 5//3 7//3 6//3\nf 5//3 8//3 7//3\n"),
        "{obj}"
    );
    assert!(!obj.contains("-0"), "{obj}");
}

#[test]
fn meshing_a_u3d_model_is_refused_and_writes_nothing() {
    let input = three_d_pdf("mesh_u3d");
    let output = input.with_extension("stl");
    let out = mesh(&input, "0", &output, &[]);
    assert_eq!(out.status.code(), Some(9));
    assert!(!output.exists());
    let stderr = String::from_utf8_lossy(&out.stderr);
    #[cfg(feature = "3d")]
    assert!(stderr.contains("not a PRC model"), "{stderr}");
    #[cfg(not(feature = "3d"))]
    assert!(stderr.contains("without the `3d` feature"), "{stderr}");
}

/// Bytes that start `PRC` but are not a container are refused with the
/// decoder's reason.
#[cfg(feature = "3d")]
#[test]
fn a_damaged_prc_model_is_refused() {
    let input = three_d_pdf("mesh_bad");
    let output = input.with_extension("stl");
    let out = mesh(&input, "1", &output, &[]);
    assert_eq!(out.status.code(), Some(9));
    assert!(!output.exists());
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("3D artwork 1:"),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// A model holding only compressed tessellation is refused by name, not as
/// an empty model.
#[cfg(feature = "3d")]
#[test]
fn a_compressed_only_prc_model_is_refused_by_name() {
    let input = with_prc("mesh_compressed", "compressed.prc");
    let output = input.with_extension("stl");
    let out = mesh(&input, "2", &output, &[]);
    assert_eq!(out.status.code(), Some(9));
    assert!(!output.exists());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("1 mesh(es) use compressed tessellation in a form")
            && stderr.contains("(a triangle refers to a vertex not yet decoded)"),
        "{stderr}"
    );
}

/// A compressed mesh that rebuilds is exported, and the reconstruction is
/// disclosed.
#[cfg(feature = "3d")]
#[test]
fn a_rebuilt_compressed_mesh_is_exported_and_disclosed() {
    let input = with_prc("mesh_compressed_rebuilt", "compressed_triangle.prc");
    let output = input.with_extension("obj");
    let out = mesh(&input, "2", &output, &["--format", "obj"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let obj = std::fs::read_to_string(&output).unwrap();
    assert!(obj.contains("f 1 2 3\n"), "{obj}");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("meshes=1 triangles=1")
            && stdout.contains("compressed_rebuilt=1 compressed_skipped=0"),
        "{stdout}"
    );
    assert!(
        stdout.contains("note: 1 compressed mesh(es) were rebuilt"),
        "{stdout}"
    );
}

/// A compressed mesh only the best-fit search rebuilds is exported with the
/// note saying so, and `--mesh-fit unique` leaves it out with its reason.
#[cfg(feature = "3d")]
#[test]
fn a_best_fit_mesh_is_disclosed_and_the_strict_setting_leaves_it_out() {
    let input = with_prc("mesh_best_fit", "best_fit.prc");
    let output = input.with_extension("obj");
    let out = mesh(&input, "2", &output, &["--format", "obj"]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    assert!(
        stdout.contains("triangles=28")
            && stdout.contains("note: 1 of those were rebuilt by a best-fit search"),
        "{stdout}"
    );
    let strict = input.with_extension("strict.obj");
    let out = mesh(&input, "2", &strict, &["--mesh-fit", "unique"]);
    assert_eq!(out.status.code(), Some(9));
    assert!(!strict.exists());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("the strict mesh-fit setting draws unique fits only"),
        "{stderr}"
    );
}

#[cfg(feature = "3d")]
#[test]
fn a_prc_assembly_renders_both_placed_copies() {
    let input = with_prc("render_assembly", "assembly.prc");
    let output = input.with_extension("png");
    let out = run(&[
        "3d-render",
        input.to_str().unwrap(),
        "--index",
        "2",
        "--view",
        "top",
        "--ortho",
        "--width",
        "200",
        "--height",
        "100",
        "-o",
        output.to_str().unwrap(),
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("meshes=2 triangles=4 "), "{stdout}");
    assert!(stdout.contains("projection=orthographic"), "{stdout}");
    assert!(
        stdout.contains(
            "note: each part, and each face styled on its own, is drawn in the colour its model \
             tree gives it (0 translucent)"
        ) && stdout.contains("2 mesh(es) had none and are drawn grey"),
        "{stdout}"
    );
    let decoder = png::Decoder::new(std::fs::File::open(&output).unwrap());
    let mut reader = decoder.read_info().unwrap();
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).unwrap();
    assert_eq!((info.width, info.height), (200, 100));
    let px = |x: usize, y: usize| buf[(y * 200 + x) * 4];
    // The squares sit at x 0..1 and 4..5 of a 0..5 model framed 5.5 wide:
    // pixels ~9-45 and ~155-191; the gap between them stays white.
    assert!(px(27, 50) < 255, "the first copy is drawn");
    assert!(px(173, 50) < 255, "the mirrored copy is drawn");
    assert_eq!(px(4, 50), 255, "a margin on the left");
    assert_eq!(px(100, 50), 255, "nothing between the copies");
}

/// A square textured with a 2x2 red/green/blue/white picture draws each
/// texel, says so, and `--texture-origin top` flips it.
#[cfg(feature = "3d")]
#[test]
fn a_textured_prc_model_renders_its_picture() {
    let input = with_prc("render_textured", "textured.prc");
    let output = input.with_extension("png");
    let render = |extra: &[&str]| {
        let mut args = vec![
            "3d-render",
            input.to_str().unwrap(),
            "--index",
            "2",
            "--view",
            "top",
            "--ortho",
            "--width",
            "64",
            "--height",
            "64",
            "-o",
            output.to_str().unwrap(),
        ];
        args.extend_from_slice(extra);
        let out = run(&args);
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
        assert!(
            stdout.contains("note: 1 mesh(es) drawn with their texture picture"),
            "{stdout}"
        );
        assert!(!stdout.contains("note: texture on"), "{stdout}");
        let decoder = png::Decoder::new(std::fs::File::open(&output).unwrap());
        let mut reader = decoder.read_info().unwrap();
        let mut buf = vec![0; reader.output_buffer_size()];
        reader.next_frame(&mut buf).unwrap();
        buf
    };
    let bottom = render(&[]);
    // Lit and filtered, a texel shows as a pixel its own channel dominates.
    let has = |buf: &[u8], c: usize| {
        buf.chunks_exact(4)
            .any(|p| (0..3).all(|k| if k == c { p[k] > 100 } else { p[k] < p[c] / 4 }))
    };
    let top = render(&["--texture-origin", "top"]);
    assert_ne!(bottom, top, "the picture flips");
    for (buf, origin) in [(&bottom, "bottom"), (&top, "top")] {
        for c in 0..3 {
            assert!(has(buf, c), "{origin}: the texel of channel {c} is drawn");
        }
    }
}

/// The fixture's two styled copies sit on alpha-0 materials with style
/// transparencies 255 and 128: by default the style's value wins, and
/// `--style-alpha multiply` makes both fully see-through.
#[cfg(feature = "3d")]
#[test]
fn style_alpha_picks_how_a_style_transparency_meets_its_material() {
    let input = with_prc("render_coloured", "coloured.prc");
    let output = input.with_extension("png");
    for (extra, translucent) in [(&[][..], 1), (&["--style-alpha", "multiply"][..], 2)] {
        let mut args = vec![
            "3d-render",
            input.to_str().unwrap(),
            "--index",
            "2",
            "--view",
            "top",
            "--ortho",
            "--width",
            "200",
            "--height",
            "100",
            "-o",
            output.to_str().unwrap(),
        ];
        args.extend_from_slice(extra);
        let out = run(&args);
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(stdout.contains("meshes=3 "), "{stdout}");
        if translucent == 1 {
            // Squares at x 0..1, 2..3 and 4..5 seen from above, 5.5 wide.
            let decoder = png::Decoder::new(std::fs::File::open(&output).unwrap());
            let mut reader = decoder.read_info().unwrap();
            let mut buf = vec![0; reader.output_buffer_size()];
            reader.next_frame(&mut buf).unwrap();
            let px = |x: usize| &buf[(50 * 200 + x) * 4..][..3];
            let (red, blue, grey) = (px(27), px(100), px(173));
            assert!(red[0] > 100 && red[1] < 30 && red[2] < 30, "{red:?}");
            assert!(blue[2] > blue[0] + 50 && blue[0] > 30, "blended: {blue:?}");
            assert!(
                grey[0] < 250 && grey.iter().all(|&c| c.abs_diff(grey[0]) < 16),
                "{grey:?}"
            );
        }
        assert!(
            stdout.contains(&format!("({translucent} translucent)"))
                && stdout.contains("1 mesh(es) had none"),
            "{extra:?}: {stdout}"
        );
    }
}

/// A grey square whose material alpha is 0 under a style with no
/// transparency: drawn opaque by default, and said so; invisible under
/// `--style-alpha style`.
#[cfg(feature = "3d")]
#[test]
fn a_zero_material_alpha_is_drawn_opaque_by_default_and_disclosed() {
    let input = with_prc("render_alpha_unset", "alpha-unset.prc");
    let output = input.with_extension("png");
    for (extra, drawn) in [(&[][..], true), (&["--style-alpha", "style"][..], false)] {
        let mut args = vec![
            "3d-render",
            input.to_str().unwrap(),
            "--index",
            "2",
            "--view",
            "top",
            "--width",
            "100",
            "--height",
            "100",
            "-o",
            output.to_str().unwrap(),
        ];
        args.extend_from_slice(extra);
        let out = run(&args);
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let stdout = String::from_utf8_lossy(&out.stdout);
        let decoder = png::Decoder::new(std::fs::File::open(&output).unwrap());
        let mut reader = decoder.read_info().unwrap();
        let mut buf = vec![0; reader.output_buffer_size()];
        reader.next_frame(&mut buf).unwrap();
        let inked = buf.chunks_exact(4).filter(|p| p[..3] != [255; 3]).count();
        assert_eq!(inked > 0, drawn, "{extra:?}: {inked} inked pixels");
        let note = "inferred: 1 mesh(es) drawn opaque: each material's alpha is 0";
        assert_eq!(stdout.contains(note), drawn, "{extra:?}: {stdout}");
    }
}

/// The fixture's first copy is recoloured translucent blue and its third
/// hidden by the assembly's entity references: drawn so and said so by
/// default, and each copy in its own colour under `--entity-overrides
/// ignore`.
#[cfg(feature = "3d")]
#[test]
fn entity_overrides_recolour_and_hide_the_copies_an_assembly_names() {
    let input = with_prc("render_overridden", "overridden.prc");
    let output = input.with_extension("png");
    for (extra, applied) in [
        (&[][..], true),
        (&["--entity-overrides", "ignore"][..], false),
    ] {
        let mut args = vec![
            "3d-render",
            input.to_str().unwrap(),
            "--index",
            "2",
            "--view",
            "top",
            "--ortho",
            "--width",
            "200",
            "--height",
            "100",
            "-o",
            output.to_str().unwrap(),
        ];
        args.extend_from_slice(extra);
        let out = run(&args);
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let stdout = String::from_utf8_lossy(&out.stdout);
        let note = "2 placement(s) drawn in a colour an enclosing assembly overrides";
        assert_eq!(stdout.contains(note), applied, "{extra:?}: {stdout}");
        let decoder = png::Decoder::new(std::fs::File::open(&output).unwrap());
        let mut reader = decoder.read_info().unwrap();
        let mut buf = vec![0; reader.output_buffer_size()];
        reader.next_frame(&mut buf).unwrap();
        let count = |f: &dyn Fn(&[u8]) -> bool| buf.chunks_exact(4).filter(|p| f(&p[..3])).count();
        let blue = count(&|p| p[2] > p[0].saturating_add(50));
        let red = count(&|p| p[0] > 100 && p[2] < 30);
        let grey = count(&|p| p != [255; 3] && p.iter().all(|&c| c.abs_diff(p[0]) < 16));
        assert_eq!(blue > 0, applied, "{extra:?}: {blue} blue pixels");
        assert!(applied || red > 0, "{extra:?}: each copy in its own red");
        assert_eq!(
            grey > 0,
            !applied,
            "{extra:?}: {grey} grey pixels, the hidden copy"
        );
    }
}

#[test]
fn rendering_a_u3d_model_is_refused_and_writes_nothing() {
    let input = three_d_pdf("render_u3d");
    let output = input.with_extension("png");
    let out = run(&[
        "3d-render",
        input.to_str().unwrap(),
        "--index",
        "0",
        "-o",
        output.to_str().unwrap(),
    ]);
    assert_eq!(out.status.code(), Some(9));
    assert!(!output.exists());
}

/// A page whose `/3D` annotation holds `fixtures/synthetic/prc/assembly.prc`
/// raw, with one view dictionary `view` as the stream's `/VA`.
#[cfg(feature = "3d")]
fn assembly_with_view(tag: &str, view: &str) -> PathBuf {
    let prc = std::fs::read(format!(
        "{}/../../fixtures/synthetic/prc/assembly.prc",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap();
    let mut stream = format!(
        "<< /Type /3D /Subtype /PRC /VA [{view}] /Length {} >>\nstream\n",
        prc.len()
    )
    .into_bytes();
    stream.extend_from_slice(&prc);
    stream.extend_from_slice(b"\nendstream");
    let bodies: [Vec<u8>; 5] = [
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Annots [4 0 R] >>".to_vec(),
        b"<< /Type /Annot /Subtype /3D /Rect [0 0 100 100] /3DD 5 0 R >>".to_vec(),
        stream,
    ];
    let mut buf = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in bodies.iter().enumerate() {
        offsets.push(buf.len());
        buf.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
        buf.extend_from_slice(body);
        buf.extend_from_slice(b"\nendobj\n");
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
    let path = std::env::temp_dir().join(format!("pdfcer_3d_{tag}_{}.pdf", std::process::id()));
    std::fs::write(&path, buf).unwrap();
    path
}

#[cfg(feature = "3d")]
#[test]
fn a_render_with_no_camera_option_opens_on_the_files_saved_view() {
    // Looking down -z with +x up the image: the two copies, at x 0..1 and
    // 4..5, are stacked vertically. Every default named view puts them side
    // by side.
    let input = assembly_with_view(
        "render_saved_view",
        "<< /Type /3DView /XN (Plan) /MS /M /P << /Subtype /O /OS 0.2 /OB /Min >> \
         /C2W [0 1 0 1 0 0 0 0 -1 2.5 0.5 50] >>",
    );
    let output = input.with_extension("png");
    let out = run(&[
        "3d-render",
        input.to_str().unwrap(),
        "--index",
        "0",
        "--width",
        "100",
        "--height",
        "200",
        "-o",
        output.to_str().unwrap(),
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("projection=orthographic"), "{stdout}");
    assert!(
        stdout.contains("note: camera: the file's opening view \"Plan\""),
        "{stdout}"
    );
    let decoder = png::Decoder::new(std::fs::File::open(&output).unwrap());
    let mut reader = decoder.read_info().unwrap();
    let mut buf = vec![0; reader.output_buffer_size()];
    reader.next_frame(&mut buf).unwrap();
    // /OS 0.2 /OB /Min: the 100-pixel width spans 5 units, so 20 pixels a
    // unit, centred on the camera axis at x 2.5.
    let px = |x: usize, y: usize| buf[(y * 100 + x) * 4];
    assert!(px(50, 60) < 255, "the x 4..5 copy is drawn at y 50..70");
    assert!(px(50, 140) < 255, "the x 0..1 copy is drawn at y 130..150");
    assert_eq!(px(50, 100), 255, "nothing between the copies");
    assert_eq!(px(50, 27), 255, "the saved scale, not a fit to the model");
    assert!(stdout.contains("10.000000 model units high"), "{stdout}");
}

#[cfg(feature = "3d")]
#[test]
fn a_named_view_overrides_the_files_saved_view() {
    let input = assembly_with_view(
        "render_saved_view_overridden",
        "<< /Type /3DView /XN (Plan) /MS /M /C2W [0 1 0 1 0 0 0 0 -1 0 0 50] >>",
    );
    let output = input.with_extension("png");
    let out = run(&[
        "3d-render",
        input.to_str().unwrap(),
        "--index",
        "0",
        "--view",
        "top",
        "-o",
        output.to_str().unwrap(),
    ]);
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("note: camera: the named view asked for")
            && stdout.contains("projection=perspective"),
        "{stdout}"
    );
}

fn embed_square(tag: &str, extra: &[&str]) -> Output {
    let input = three_d_pdf(tag);
    let prc = format!(
        "{}/../../fixtures/synthetic/prc/square.prc",
        env!("CARGO_MANIFEST_DIR")
    );
    let mut args = vec![
        "3d-embed",
        input.to_str().unwrap(),
        "--model",
        &prc,
        "--page",
        "1",
        "--rect",
        "10,10,190,100",
    ];
    args.extend_from_slice(extra);
    let out = run(&args);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    out
}

/// A PRC model with no `--poster` gets a poster rendered from it, and the
/// summary and an `inferred:` line say so.
#[cfg(feature = "3d")]
#[test]
fn an_embedded_prc_model_gets_a_rendered_poster_disclosed() {
    let out = embed_square("emb_rendered", &[]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains(" poster=rendered:360x180 "), "{stdout}");
    assert!(
        stdout.contains("inferred: poster rendered by pdfcer from the model"),
        "{stdout}"
    );
    assert!(!stdout.contains("placeholder"), "{stdout}");
}

/// `--placeholder-poster` draws the placeholder and infers nothing about it.
#[test]
fn placeholder_poster_skips_the_rendering() {
    let out = embed_square("emb_placeholder", &["--placeholder-poster"]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains(" poster=placeholder "), "{stdout}");
    assert!(!stdout.contains("poster rendered"), "{stdout}");
    assert!(!stdout.contains("placeholder drawing"), "{stdout}");
}

fn poster_png() -> String {
    format!(
        "{}/../../fixtures/synthetic/images/rgb8.png",
        env!("CARGO_MANIFEST_DIR")
    )
}

/// `3d-poster` replaces the poster: the written file lists the model with a
/// poster and extracts the same model bytes.
#[test]
fn a_replaced_poster_keeps_the_model() {
    let input = three_d_pdf("poster_apply");
    let output = input.with_extension("out.pdf");
    let png = poster_png();
    let out = run(&[
        "3d-poster",
        input.to_str().unwrap(),
        "--index",
        "0",
        "--image",
        &png,
        "--apply",
        "-o",
        output.to_str().unwrap(),
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.starts_with("3d-poster ") && stdout.contains(" index=0 page=1 annot=4 "),
        "{stdout}"
    );
    assert!(stdout.contains("applied=1"), "{stdout}");

    let listed = run(&["3d-list", output.to_str().unwrap()]);
    let listed = String::from_utf8_lossy(&listed.stdout);
    assert!(
        listed.contains("3d index=0 page=1 format=U3D views=1 poster=yes source=stream\n"),
        "{listed}"
    );
    let extracted = input.with_extension("poster.u3d");
    let ext = run(&[
        "3d-extract",
        output.to_str().unwrap(),
        "--index",
        "0",
        "-o",
        extracted.to_str().unwrap(),
    ]);
    assert!(ext.status.success());
    assert_eq!(
        std::fs::read(&extracted).unwrap(),
        b"U3D\0\xC0\xFF\xEE",
        "the model bytes are unchanged"
    );
}

/// A RichMedia model has no 3D poster: refused, nothing written.
#[test]
fn a_richmedia_poster_is_refused_and_writes_nothing() {
    let input = three_d_pdf("poster_rich");
    let output = input.with_extension("out.pdf");
    let png = poster_png();
    let out = run(&[
        "3d-poster",
        input.to_str().unwrap(),
        "--index",
        "1",
        "--image",
        &png,
        "--apply",
        "-o",
        output.to_str().unwrap(),
    ]);
    assert_eq!(out.status.code(), Some(9));
    assert!(!output.exists());
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("is not a 3D annotation"),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// The assembly fixture's tree: a root over two placements of one part.
#[cfg(feature = "3d")]
#[test]
fn a_prc_assembly_tree_is_listed() {
    let input = with_prc("tree_assembly", "assembly.prc");
    let path = input.to_str().unwrap();
    let out = run(&["3d-tree", path, "--index", "2"]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    assert_eq!(
        stdout.lines().collect::<Vec<_>>(),
        [
            "occurrence 0:0  [2 placement(s)]",
            "  occurrence 0:1  [1 placement(s)]",
            "  occurrence 0:2  [1 placement(s)]",
            "nodes=3 drawn=3 not_drawn=0",
        ]
    );
    let out = run(&["3d-tree", path, "--index", "2", "--json"]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains(
            "{\"name\":null,\"name_from\":\"none\",\"parent\":0,\"depth\":1,\
             \"file_structure\":0,\"occurrence\":2,\"hidden\":false,\"suppressed\":false,\
             \"drawn\":true,\"has_part\":true,\"placements\":[1,2]}"
        ),
        "{stdout}"
    );
    let out = run(&["3d-tree", path, "--index", "0"]);
    assert_eq!(out.status.code(), Some(9));
    assert!(String::from_utf8_lossy(&out.stderr).contains("not a PRC model"));
}

#[cfg(feature = "3d")]
#[test]
fn draw_hidden_also_writes_the_parts_the_file_stores_hidden() {
    let input = with_prc("draw_hidden", "named-tree.prc");
    let output = input.with_extension("stl");
    let triangles = |extra: &[&str]| {
        let out = mesh(&input, "2", &output, extra);
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let stl = std::fs::read(&output).unwrap();
        u32::from_le_bytes(stl[80..84].try_into().unwrap())
    };
    assert_eq!(triangles(&[]), 6, "three visible copies of the square");
    assert_eq!(
        triangles(&["--draw-hidden"]),
        10,
        "plus the hidden and suppressed copies"
    );
}

#[cfg(feature = "3d")]
fn views_3d(input: &Path, extra: &[&str]) -> Output {
    let mut args = vec!["3d-views", input.to_str().unwrap(), "--index", "2"];
    args.extend_from_slice(extra);
    run(&args)
}

#[cfg(feature = "3d")]
#[test]
fn named_views_are_written_and_listed() {
    let input = with_prc_square("views_write");
    let output = input.with_extension("views.pdf");
    let out = views_3d(
        &input,
        &[
            "--view",
            "front",
            "--view",
            "top",
            "--default",
            "top",
            "--apply",
            "-o",
            output.to_str().unwrap(),
        ],
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("views=0 -> 2 [Front,Top] default=Top shared=0"),
        "{stdout}"
    );
    let listed = run(&["3d-list", output.to_str().unwrap()]);
    let listed = String::from_utf8_lossy(&listed.stdout);
    assert!(
        listed.contains("3d index=2 page=1 format=PRC views=2 "),
        "{listed}"
    );

    let cleared = output.with_extension("cleared.pdf");
    let out = views_3d(
        &output,
        &["--clear", "--apply", "-o", cleared.to_str().unwrap()],
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(String::from_utf8_lossy(&out.stdout).contains("views=2 -> 0 [] default=-"));
    let listed = run(&["3d-list", cleared.to_str().unwrap()]);
    assert!(String::from_utf8_lossy(&listed.stdout).contains("format=PRC views=0 "));
}

#[cfg(feature = "3d")]
#[test]
fn an_orthographic_views_dry_run_discloses_and_writes_nothing() {
    let input = with_prc_square("views_dry");
    let output = input.with_extension("dry.pdf");
    let out = views_3d(
        &input,
        &["--view", "iso", "--ortho", "-o", output.to_str().unwrap()],
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("note: an orthographic view's scale"),
        "{stderr}"
    );
    assert!(String::from_utf8_lossy(&out.stdout).contains("applied=0"));
    assert!(!output.exists());
}

#[cfg(feature = "3d")]
#[test]
fn views_on_a_u3d_model_or_a_richmedia_index_are_refused() {
    let input = three_d_pdf("views_u3d");
    for index in ["0", "1"] {
        let out = run(&[
            "3d-views",
            input.to_str().unwrap(),
            "--index",
            index,
            "--view",
            "front",
        ]);
        assert_eq!(out.status.code(), Some(9), "index {index}");
    }
}
