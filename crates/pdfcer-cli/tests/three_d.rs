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
        stdout.contains(
            "3d index=0 page=1 format=U3D views=1 poster=yes activate=XA source=stream\n"
        ),
        "{stdout}"
    );
    assert!(
        stdout.contains(
            "3d index=1 page=1 format=U3D views=0 poster=no activate=XA source=richmedia name=\"../evil.u3d\"\n"
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
    assert!(
        stdout.contains("note: the model gives no unit; the mesh file stores none"),
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

/// A compressed mesh no fit rebuilds is refused by default, written in part
/// under `--mesh-fit partial` (the triangles decoded before its arrays
/// stopped fitting) and `continue` (the rest too), each with a note.
#[cfg(feature = "3d")]
#[test]
fn a_partial_mesh_is_written_only_when_asked_and_disclosed() {
    let input = with_prc("mesh_partial", "partial.prc");
    let refused = input.with_extension("obj");
    let out = mesh(&input, "2", &refused, &[]);
    assert_eq!(out.status.code(), Some(9));
    assert!(!refused.exists());
    for (fit, faces) in [("partial", 3), ("continue", 4)] {
        let output = input.with_extension(format!("{fit}.obj"));
        let out = mesh(&input, "2", &output, &["--mesh-fit", fit]);
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(out.status.success(), "{fit}: {stdout}");
        let obj = std::fs::read_to_string(&output).unwrap();
        assert_eq!(obj.lines().filter(|l| l.starts_with("f ")).count(), faces);
        assert!(
            stdout.contains(&format!(
                "note: 1 compressed mesh(es) no fit rebuilds are written in part ({faces} of 4 triangles)"
            )) && stdout.contains(
                "note: 1 of those stopped fitting because an edge is shared by more than two triangles"
            ),
            "{fit}: {stdout}"
        );
    }
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

/// A PRC wire is drawn in its colour, counted as `wires=`, and
/// `--line-width` widens it.
#[cfg(feature = "3d")]
#[test]
fn a_prc_wire_is_drawn_and_line_width_widens_it() {
    let input = with_prc("render_wired", "wired.prc");
    let mut red = Vec::new();
    for width in ["1", "3"] {
        let output = input.with_extension(format!("w{width}.png"));
        let out = run(&[
            "3d-render",
            input.to_str().unwrap(),
            "--index",
            "2",
            "--view",
            "top",
            "--ortho",
            "--width",
            "100",
            "--height",
            "100",
            "--line-width",
            width,
            "-o",
            output.to_str().unwrap(),
        ]);
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(stdout.contains(" wires=1 "), "{stdout}");
        let decoder = png::Decoder::new(std::fs::File::open(&output).unwrap());
        let mut reader = decoder.read_info().unwrap();
        let mut buf = vec![0; reader.output_buffer_size()];
        reader.next_frame(&mut buf).unwrap();
        red.push(buf.chunks(4).filter(|p| *p == [255, 0, 0, 255]).count());
    }
    // The diagonal spans about 90 pixels each way.
    assert!((80..=100).contains(&red[0]), "{red:?}");
    assert!(red[1] > 2 * red[0], "{red:?}");
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

/// A square whose material stores a highlight: drawn with it and said so,
/// `--shininess-scale` changes how sharp it is, and an unlit render draws
/// and says nothing of it.
#[cfg(feature = "3d")]
#[test]
fn a_material_highlight_is_drawn_said_and_scaled() {
    let input = with_prc("render_shiny", "shiny.prc");
    let output = input.with_extension("png");
    let render = |extra: &[&str]| {
        let mut args = vec![
            "3d-render",
            input.to_str().unwrap(),
            "--index",
            "2",
            "--view",
            "top",
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
        (stdout, std::fs::read(&output).unwrap())
    };
    let (stdout, auto) = render(&[]);
    assert!(
        stdout.contains("note: finish: 1 mesh(es) drawn with the highlight")
            && stdout.contains("shininess read as `auto`"),
        "{stdout}"
    );
    let (stdout, exponent) = render(&["--shininess-scale", "exponent"]);
    assert!(stdout.contains("shininess read as `exponent`"), "{stdout}");
    assert_ne!(auto, exponent, "the scale changed nothing");
    let (stdout, _) = render(&["--lighting", "none"]);
    assert!(!stdout.contains("note: finish"), "{stdout}");
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
        listed.contains(
            "3d index=0 page=1 format=U3D views=1 poster=yes activate=XA source=stream\n"
        ),
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
    assert!(
        listed.contains(
            "  view index=0 name=\"Front\" default=no
  view index=1 name=\"Top\" default=yes
"
        ),
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
fn a_measurement_is_written_into_every_view_and_listed() {
    let input = with_prc_square("views_measure");
    let output = input.with_extension("measured.pdf");
    let out = views_3d(
        &input,
        &[
            "--view",
            "front",
            "--view",
            "top",
            "--measure",
            "0,0,0:3,4,0",
            "--units",
            "mm",
            "--apply",
            "-o",
            output.to_str().unwrap(),
        ],
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{stderr}");
    assert!(
        stderr.contains("note: 3D measurements are a PDF 2.0"),
        "{stderr}"
    );
    let listed = run(&["3d-list", output.to_str().unwrap()]);
    let listed = String::from_utf8_lossy(&listed.stdout);
    for view in ["Front", "Top"] {
        assert!(
            listed.contains(&format!(
                "    measure view=\"{view}\" index=0 kind=linear name=\"-\" a1=0,0,0 a2=3,4,0 \
                 value=\"5.000 mm\" comment=no"
            )),
            "{listed}"
        );
    }
}

#[cfg(feature = "3d")]
#[test]
fn a_measurement_needs_units_and_two_distinct_points() {
    let input = with_prc_square("views_measure_bad");
    let out = views_3d(&input, &["--view", "front", "--measure", "0,0,0:1,0,0"]);
    assert_eq!(out.status.code(), Some(2), "--units is required");
    let out = views_3d(
        &input,
        &[
            "--view",
            "front",
            "--measure",
            "1,1,1:1,1,1",
            "--units",
            "mm",
        ],
    );
    assert_eq!(out.status.code(), Some(9));
    assert!(String::from_utf8_lossy(&out.stderr).contains("its two points coincide"));
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

/// `--pick` reports the point drawn at an image position through the
/// render's own camera: each placed copy is hit, the gap between is a miss.
#[cfg(feature = "3d")]
#[test]
fn a_pick_reports_the_model_point_under_a_pixel() {
    let input = with_prc("render_pick", "assembly.prc");
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
        "--pick",
        "27.5,50.5",
        "--pick",
        "100.5,50.5",
        "--pick",
        "173.5,50.5",
        "-o",
        output.to_str().unwrap(),
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let picks: Vec<&str> = stdout.lines().filter(|l| l.starts_with("pick ")).collect();
    assert_eq!(picks.len(), 3, "{stdout}");
    assert!(picks[0].starts_with("pick x=27.5 y=50.5 mesh="), "{stdout}");
    assert_eq!(picks[1], "pick x=100.5 y=50.5 miss", "{stdout}");
    assert!(picks[2].contains(" mesh="), "{stdout}");
    let field = |line: &str, key: &str| -> String {
        let rest = &line[line.find(key).unwrap() + key.len()..];
        rest.split(' ').next().unwrap().to_owned()
    };
    assert_ne!(field(picks[0], "mesh="), field(picks[2], "mesh="));
    let x = |line: &str| -> f64 {
        field(line, "point=")
            .split(',')
            .next()
            .unwrap()
            .parse()
            .unwrap()
    };
    assert!((0.0..=1.0).contains(&x(picks[0])), "{stdout}");
    assert!((4.0..=5.0).contains(&x(picks[2])), "{stdout}");
    for p in [picks[0], picks[2]] {
        let n: Vec<f64> = field(p, "normal=")
            .split(',')
            .map(|c| c.parse().unwrap())
            .collect();
        assert!(
            n[0].abs() < 1e-9 && n[1].abs() < 1e-9 && (n[2].abs() - 1.0).abs() < 1e-9,
            "{p}"
        );
    }
}

/// `--lighting` names its scheme and source, and `--threads` never changes
/// a pixel.
#[cfg(feature = "3d")]
#[test]
fn render_lighting_is_reported_and_threads_draw_the_same_image() {
    let input = with_prc("render_lighting", "assembly.prc");
    let render = |tag: &str, extra: &[&str]| {
        let output = input.with_extension(format!("{tag}.png"));
        let mut args = vec!["3d-render", input.to_str().unwrap(), "--index", "2"];
        args.extend_from_slice(extra);
        let out_path = output.to_str().unwrap().to_owned();
        args.extend_from_slice(&["--width", "120", "--height", "80", "-o", &out_path]);
        let out = run(&args);
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        (
            String::from_utf8_lossy(&out.stdout).into_owned(),
            std::fs::read(&output).unwrap(),
        )
    };
    let (default, _) = render("default", &[]);
    assert!(
        default.contains("note: lighting: Artwork (the default"),
        "{default}"
    );
    let (night, one) = render("night1", &["--lighting", "night", "--threads", "1"]);
    assert!(
        night.contains("note: lighting: Night (--lighting)"),
        "{night}"
    );
    let (_, many) = render("night0", &["--lighting", "night", "--threads", "0"]);
    assert_eq!(one, many, "the thread count changes no pixel");
}

/// `3d-render --lens` with `--width 160 --height 100` and `extra`.
#[cfg(feature = "3d")]
fn lens_render(tag: &str, extra: &[&str]) -> (Output, PathBuf) {
    let input = with_prc("render_lens", "assembly.prc");
    let output = input.with_extension(format!("{tag}.png"));
    let mut args = vec![
        "3d-render",
        input.to_str().unwrap(),
        "--index",
        "2",
        "--view",
        "top",
        "--width",
        "160",
        "--height",
        "100",
        "-o",
        output.to_str().unwrap(),
    ];
    args.extend_from_slice(extra);
    let out = run(&args);
    (out, output)
}

/// A fisheye render names its lens, draws the background outside the
/// picture circle and refuses a pick there by name.
#[cfg(feature = "3d")]
#[test]
fn a_fisheye_render_names_its_lens_and_leaves_the_rim_background() {
    let (out, output) = lens_render(
        "equi",
        &["--lens", "equidistant", "--fov", "360", "--pick", "0.5,0.5"],
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("projection=fisheye-equidistant"),
        "{stdout}"
    );
    assert!(
        stdout.contains("pick x=0.5 y=0.5 refused: the pixel lies outside the fisheye lens"),
        "{stdout}"
    );
    let decoder = png::Decoder::new(std::fs::File::open(&output).unwrap());
    let mut reader = decoder.read_info().unwrap();
    let mut buf = vec![0; reader.output_buffer_size()];
    reader.next_frame(&mut buf).unwrap();
    assert_eq!(&buf[..4], &[255, 255, 255, 255], "the corner is background");

    let (out, _) = lens_render("stereo", &["--lens", "stereographic"]);
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("projection=fisheye-stereographic"),
        "{stdout}"
    );
}

/// A field the lens cannot draw is refused with exit 9 and its reason; a
/// fisheye cannot also be orthographic.
#[cfg(feature = "3d")]
#[test]
fn a_fisheye_field_out_of_range_is_refused() {
    let (out, output) = lens_render("refused", &["--lens", "stereographic", "--fov", "360"]);
    assert_eq!(out.status.code(), Some(9));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("a Stereographic fisheye's field of view must be above 0"),
        "{stderr}"
    );
    assert!(!output.exists());

    let (out, _) = lens_render("flat_wide", &["--fov", "200"]);
    assert_eq!(out.status.code(), Some(9), "a flat lens stays below 180");

    let (out, _) = lens_render("ortho", &["--lens", "equidistant", "--ortho"]);
    assert_eq!(out.status.code(), Some(2));
}

/// `3d-render` of the lens fixture's model to `<tag>.<ext>`, sized only by
/// `extra`.
#[cfg(feature = "3d")]
fn panorama_render(tag: &str, ext: &str, extra: &[&str]) -> (Output, PathBuf) {
    let input = with_prc("render_pano", "assembly.prc");
    let output = input.with_extension(format!("{tag}.{ext}"));
    let mut args = vec![
        "3d-render",
        input.to_str().unwrap(),
        "--index",
        "2",
        "--view",
        "top",
        "-o",
        output.to_str().unwrap(),
    ];
    args.extend_from_slice(extra);
    (run(&args), output)
}

/// Whether `needle` occurs in `hay`.
#[cfg(feature = "3d")]
fn holds(hay: &[u8], needle: &str) -> bool {
    hay.windows(needle.len()).any(|w| w == needle.as_bytes())
}

/// `--360` into a `.jpg` draws a 2:1 panorama, names its projection and
/// carries the photo-sphere tag right after the JPEG's opening segments.
#[cfg(feature = "3d")]
#[test]
fn a_360_jpeg_is_a_two_to_one_photo_sphere_tagged_for_photo_viewers() {
    let (out, output) = panorama_render("sphere", "jpg", &["--360", "--height", "40"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("width=80 height=40 projection=equirectangular"),
        "{stdout}"
    );
    assert!(
        stdout.contains("note: photo sphere: the JPEG carries"),
        "{stdout}"
    );
    let bytes = std::fs::read(&output).unwrap();
    assert_eq!(&bytes[..2], &[0xFF, 0xD8]);
    assert!(holds(&bytes, "http://ns.adobe.com/xap/1.0/\0"));
    for field in [
        "<GPano:ProjectionType>equirectangular</GPano:ProjectionType>",
        "<GPano:FullPanoWidthPixels>80</GPano:FullPanoWidthPixels>",
        "<GPano:FullPanoHeightPixels>40</GPano:FullPanoHeightPixels>",
    ] {
        assert!(holds(&bytes, field), "{field}");
    }

    let (out, _) = panorama_render(
        "lens",
        "jpeg",
        &["--lens", "equirectangular", "--width", "60"],
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("width=60 height=30 projection=equirectangular"),
        "{stdout}"
    );
}

/// A flat JPEG carries no photo-sphere tag; a panorama PNG carries none
/// and says so, as it says a picture is not 2:1.
#[cfg(feature = "3d")]
#[test]
fn only_a_panorama_jpeg_is_tagged_and_a_png_panorama_says_it_is_not() {
    let (out, output) = panorama_render("flat", "jpg", &["--width", "40", "--height", "30"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let bytes = std::fs::read(&output).unwrap();
    assert_eq!(&bytes[..2], &[0xFF, 0xD8]);
    assert!(!holds(&bytes, "GPano"));
    assert!(!String::from_utf8_lossy(&out.stdout).contains("photo sphere"));

    let extra = ["--360", "--width", "50", "--height", "40"];
    let (out, output) = panorama_render("sphere_png", "png", &extra);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("the PNG is a 360-degree panorama but carries no"),
        "{stdout}"
    );
    assert!(
        stdout.contains("the picture is 50x40, not twice as wide"),
        "{stdout}"
    );
    assert!(!holds(&std::fs::read(&output).unwrap(), "GPano"));
}

/// A JPEG cannot be transparent, and `--360` takes no other lens or field.
#[cfg(feature = "3d")]
#[test]
fn a_transparent_jpeg_and_a_360_with_another_lens_are_refused() {
    let (out, output) = panorama_render("clear", "jpg", &["--360", "--transparent"]);
    assert_eq!(out.status.code(), Some(9));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("--transparent cannot be honoured for a JPEG"),
        "{stderr}"
    );
    assert!(!output.exists());
    for extra in [["--360", "--fov", "200"], ["--360", "--lens", "flat"]] {
        let (out, _) = panorama_render("conflict", "jpg", &extra);
        assert_eq!(out.status.code(), Some(2), "{extra:?}");
    }
    let (out, _) = panorama_render(
        "field",
        "jpg",
        &["--lens", "equirectangular", "--fov", "200"],
    );
    assert_eq!(out.status.code(), Some(9), "a panorama's field is 360");
}

/// The RGBA pixels of the PNG at `path`.
#[cfg(feature = "3d")]
fn png_pixels(path: &Path) -> Vec<u8> {
    let decoder = png::Decoder::new(std::fs::File::open(path).unwrap());
    let mut reader = decoder.read_info().unwrap();
    let mut buf = vec![0; reader.output_buffer_size()];
    reader.next_frame(&mut buf).unwrap();
    buf
}

/// `lens_render` that must succeed: its stdout and pixels.
#[cfg(feature = "3d")]
fn lit_render(tag: &str, extra: &[&str]) -> (String, Vec<u8>) {
    let (out, output) = lens_render(tag, extra);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    (stdout, png_pixels(&output))
}

/// `--light-gain` brightens the picture and says so; 1 is the default
/// frame; outside 0.25 to 4 is refused before drawing.
#[cfg(feature = "3d")]
#[test]
fn light_gain_brightens_the_render_and_is_noted() {
    let sum = |p: &[u8]| p.iter().map(|&c| u64::from(c)).sum::<u64>();
    let (plain_note, plain) = lit_render("gain_default", &["--lighting", "cad"]);
    let (one_note, one) = lit_render("gain_1", &["--lighting", "cad", "--light-gain", "1"]);
    assert_eq!(plain, one, "a gain of 1 is the default frame");
    assert!(!plain_note.contains("--light-gain") && !one_note.contains("--light-gain"));
    let (note, dim) = lit_render("gain_half", &["--lighting", "cad", "--light-gain", "0.5"]);
    assert!(
        note.contains("note: lighting: every light times 0.5 (--light-gain)"),
        "{note}"
    );
    let (_, bright) = lit_render("gain_2", &["--lighting", "cad", "--light-gain", "2"]);
    assert!(sum(&dim) < sum(&one) && sum(&one) < sum(&bright));
    for bad in ["0.2", "4.5", "nan"] {
        let (out, output) = lens_render("gain_bad", &["--light-gain", bad]);
        assert_eq!(out.status.code(), Some(2), "{bad}");
        assert!(!output.exists());
    }
}

/// The red and blue sums of `rgba`.
#[cfg(feature = "3d")]
fn red_blue(rgba: &[u8]) -> (u64, u64) {
    rgba.chunks(4).fold((0, 0), |(r, b), p| {
        (r + u64::from(p[0]), b + u64::from(p[2]))
    })
}

/// `--sun-elevation` raises the sun; outside 0 to 90 is refused.
#[cfg(feature = "3d")]
#[test]
fn sun_elevation_raises_the_sun() {
    let (note, low) = lit_render("sun_el10", &["--sun-elevation", "10"]);
    assert!(note.contains("elevation 10°"), "{note}");
    let (_, high) = lit_render("sun_el80", &["--sun-elevation", "80"]);
    assert_ne!(low, high);
    for bad in ["-1", "91"] {
        let (out, _) = lens_render("sun_el_bad", &["--sun-elevation", bad]);
        assert_eq!(out.status.code(), Some(2), "{bad}");
    }
}

/// `--sun-kelvin` colours the sun: 2000 K draws redder than 6500 K.
#[cfg(feature = "3d")]
#[test]
fn sun_kelvin_colours_the_sun() {
    let (note, warm) = lit_render("sun_k2000", &["--sun-kelvin", "2000"]);
    assert!(note.contains("2000 K"), "{note}");
    let (_, white) = lit_render("sun_k6500", &["--sun-kelvin", "6500"]);
    let ((wr, wb), (r, b)) = (red_blue(&warm), red_blue(&white));
    assert!(wr * b > r * wb, "{wr}/{wb} {r}/{b}");
    let (out, _) = lens_render("sun_k_bad", &["--sun-kelvin", "999"]);
    assert_eq!(out.status.code(), Some(2));
}

/// `--sun-colour from-elevation` names the temperature it chose, and
/// cannot be given with `--sun-kelvin`.
#[cfg(feature = "3d")]
#[test]
fn sun_colour_from_elevation_says_its_temperature() {
    let args = ["--sun-colour", "from-elevation", "--sun-elevation", "90"];
    let (note, _) = lit_render("sun_from_el", &args);
    assert!(note.contains("5500 K, from its elevation"), "{note}");
    let both = ["--sun-colour", "from-elevation", "--sun-kelvin", "3000"];
    let (out, _) = lens_render("sun_both", &both);
    assert_eq!(out.status.code(), Some(2));
}

/// `--sun-gain` scales the sun alone; with no sun option there is no sun.
#[cfg(feature = "3d")]
#[test]
fn sun_gain_scales_the_sun() {
    let sum = |p: &[u8]| p.iter().map(|&c| u64::from(c)).sum::<u64>();
    let (note, plain) = lit_render("sun_none", &[]);
    assert!(!note.contains("a sun aimed"), "{note}");
    let (_, dim) = lit_render("sun_g025", &["--sun-gain", "0.25"]);
    let (_, bright) = lit_render("sun_g2", &["--sun-gain", "2"]);
    assert!(sum(&plain) < sum(&dim) && sum(&dim) < sum(&bright));
    let (out, _) = lens_render("sun_g_bad", &["--sun-gain", "5"]);
    assert_eq!(out.status.code(), Some(2));
}

/// `--project` goes back from a model point to the image position `--pick`
/// reports, and says when nothing is drawn.
#[cfg(feature = "3d")]
#[test]
fn project_reports_the_image_position_of_a_model_point() {
    let input = with_prc("render_project", "assembly.prc");
    let output = input.with_extension("project.png");
    let out = run(&[
        "3d-render",
        input.to_str().unwrap(),
        "--index",
        "2",
        "--eye",
        "0,0,10",
        "--target",
        "0,0,0",
        "--up",
        "y",
        "--width",
        "100",
        "--height",
        "100",
        "--project",
        "0,0,0",
        "--project",
        "0,0,20",
        "-o",
        output.to_str().unwrap(),
    ]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        stdout.contains("project point=0,0,0 x=50.000000 y=50.000000"),
        "{stdout}"
    );
    assert!(
        stdout.contains("project point=0,0,20 not drawn"),
        "{stdout}"
    );
}

/// A 360° picture is levelled, and says so: a pitched view keeps its eye and
/// looks along the horizon.
#[cfg(feature = "3d")]
#[test]
fn a_360_render_levels_a_pitched_view_and_discloses_it() {
    let (out, _) = panorama_render("level", "png", &["--360", "--height", "20", "--up", "y"]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        stdout.contains("the camera was levelled with +y up (as --up asked)"),
        "{stdout}"
    );
    let flat_render = panorama_render("flat", "png", &["--width", "20", "--height", "20"]);
    assert!(
        !String::from_utf8_lossy(&flat_render.0.stdout).contains("levelled"),
        "a flat picture is not levelled"
    );
}

/// Every measurement kind goes in from the command line and reads back.
#[cfg(feature = "3d")]
#[test]
fn views_write_every_measurement_kind() {
    let input = with_prc_square("views_measure_all");
    let output = input.with_extension("all.pdf");
    let out = views_3d(
        &input,
        &[
            "--view",
            "front",
            "--measure-perpendicular",
            "0,0,0:1,2,3:0,1,0",
            "--measure-angle",
            "0,0,0:1,0,0:1,1,0",
            "--measure-radius",
            "1,0,0:0,1,0:-1,0,0",
            "--note",
            "1,2,3:see: this",
            "--units",
            "mm",
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
    let listed = run(&["3d-list", output.to_str().unwrap()]);
    let listed = String::from_utf8_lossy(&listed.stdout);
    for (kind, value) in [
        ("perpendicular", "2.000 mm"),
        ("angular", "45.000°"),
        ("radial", "R1.000 mm"),
        ("comment", "see: this"),
    ] {
        assert!(
            listed.lines().any(|l| l.contains(&format!("kind={kind}"))
                && l.contains(&format!("value=\"{value}\""))),
            "{kind} {value}: {listed}"
        );
    }
}

/// An angle and a note need no units; a distance and a radius do, and a
/// measurement that cannot be drawn is refused by name.
#[cfg(feature = "3d")]
#[test]
fn measurement_flags_ask_for_units_and_refuse_what_cannot_be_drawn() {
    let input = with_prc_square("views_measure_more_bad");
    for flag in [
        ["--measure-perpendicular", "0,0,0:1,2,3:0,1,0"],
        ["--measure-radius", "1,0,0:0,1,0:-1,0,0"],
    ] {
        let out = views_3d(&input, &["--view", "front", flag[0], flag[1]]);
        assert_eq!(out.status.code(), Some(2), "{flag:?} needs --units");
    }
    let out = views_3d(
        &input,
        &["--view", "front", "--measure-angle", "0,0,0:1,0,0:1,1,0"],
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = views_3d(&input, &["--view", "front", "--units", "mm"]);
    assert_eq!(out.status.code(), Some(2), "--units alone measures nothing");
    for (flag, value, why, units) in [
        (
            "--measure-angle",
            "0,0,0:1,0,0:2,0,0",
            "--measure-angle 1",
            "",
        ),
        (
            "--measure-radius",
            "0,0,0:1,0,0:2,0,0",
            "--measure-radius 1",
            "mm",
        ),
        ("--note", "0,0,0:", "--note 1", ""),
    ] {
        let mut args = vec!["--view", "front", flag, value];
        if !units.is_empty() {
            args.extend(["--units", units]);
        }
        let out = views_3d(&input, &args);
        assert_eq!(out.status.code(), Some(9), "{flag}");
        assert!(String::from_utf8_lossy(&out.stderr).contains(why), "{flag}");
    }
}

#[cfg(feature = "3d")]
fn two_written_views(tag: &str) -> PathBuf {
    let input = with_prc_square(tag);
    let output = input.with_extension("views.pdf");
    let out = views_3d(
        &input,
        &[
            "--view",
            "front",
            "--view",
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
    output
}

#[cfg(feature = "3d")]
fn view_measures(input: &Path, extra: &[&str]) -> Output {
    let mut args = vec!["3d-view-measures", input.to_str().unwrap(), "--index", "2"];
    args.extend_from_slice(extra);
    run(&args)
}

/// The `3d-list` lines for measurements, each as its view's name and index.
#[cfg(feature = "3d")]
fn listed_measures(input: &Path) -> Vec<String> {
    let out = run(&["3d-list", input.to_str().unwrap()]);
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|l| l.trim_start().starts_with("measure view="))
        .map(|l| l.split_whitespace().take(3).collect::<Vec<_>>().join(" "))
        .collect()
}

#[cfg(feature = "3d")]
#[test]
fn one_views_measurements_change_and_the_other_view_keeps_its_own() {
    let input = two_written_views("view_measures");
    let dry = view_measures(
        &input,
        &["--view", "1", "--measure", "0,0,0:1,0,0", "--units", "mm"],
    );
    assert!(
        dry.status.success(),
        "{}",
        String::from_utf8_lossy(&dry.stderr)
    );
    let said = String::from_utf8_lossy(&dry.stdout);
    assert!(said.contains("measurements=0 -> 1"), "{said}");
    assert!(said.contains("applied=0"), "{said}");
    assert!(String::from_utf8_lossy(&dry.stderr).contains("PDF 2.0"));
    assert!(
        listed_measures(&input).is_empty(),
        "a dry run writes nothing"
    );

    let one = input.with_extension("one.pdf");
    let out = view_measures(
        &input,
        &[
            "--view",
            "1",
            "--measure",
            "0,0,0:1,0,0",
            "--note",
            "0,0,0:hello",
            "--units",
            "mm",
            "--apply",
            "-o",
            one.to_str().unwrap(),
        ],
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        listed_measures(&one),
        [
            "measure view=\"Top\" index=0",
            "measure view=\"Top\" index=1"
        ]
    );

    let two = input.with_extension("two.pdf");
    let out = view_measures(
        &one,
        &[
            "--view",
            "1",
            "--remove",
            "0",
            "--apply",
            "-o",
            two.to_str().unwrap(),
        ],
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(listed_measures(&two), ["measure view=\"Top\" index=0"]);
    let list = run(&["3d-list", two.to_str().unwrap()]);
    assert!(String::from_utf8_lossy(&list.stdout).contains("kind=comment"));

    let three = input.with_extension("three.pdf");
    let out = view_measures(
        &two,
        &[
            "--view",
            "1",
            "--clear",
            "--apply",
            "-o",
            three.to_str().unwrap(),
        ],
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(listed_measures(&three).is_empty());
}

#[cfg(feature = "3d")]
#[test]
fn a_view_measures_request_that_makes_no_sense_is_refused() {
    let input = two_written_views("view_measures_refused");
    for (extra, says) in [
        (&["--view", "9", "--clear"][..], "no view 9"),
        (&["--view", "0", "--remove", "0"][..], "--remove 0"),
        (&["--view", "0"][..], "nothing was asked for"),
    ] {
        let out = view_measures(&input, extra);
        assert!(!out.status.success(), "{extra:?}");
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(err.contains(says), "{extra:?}: {err}");
    }
}
