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
/// the written file, and nothing is inferred or noted.
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
    assert!(!stdout.contains("inferred:"), "{stdout}");
    assert!(!stdout.contains("note:"), "{stdout}");

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
        stderr.contains("1 mesh(es) use compressed tessellation in a form"),
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
