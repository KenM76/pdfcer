//! glTF to PRC, for `3d-convert` and `3d-embed --model x.gltf|x.glb`.

use super::*;

/// The glTF reading settings both commands take.
// Without `3d` the settings are parsed but there is nothing to apply them to.
#[cfg_attr(not(feature = "3d"), allow(dead_code))]
#[derive(Clone, Copy, Debug)]
pub(crate) struct GltfHow {
    pub(crate) colours: GltfColoursArg,
    pub(crate) up: GltfUpArg,
}

/// Whether `data` from `path` is glTF: a GLB signature, or a `.gltf` /
/// `.glb` extension (any case).
pub(crate) fn is_gltf(path: &Path, data: &[u8]) -> bool {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase);
    data.starts_with(b"glTF") || matches!(ext.as_deref(), Some("gltf" | "glb"))
}

/// A glTF converted to PRC, with the `note:` / `inferred:` lines to print.
pub(crate) struct Converted {
    pub(crate) prc: Vec<u8>,
    pub(crate) triangles: usize,
    pub(crate) lines: Vec<String>,
}

/// Convert the glTF `data` read from `path`, or the exit code after the
/// reason. External buffers are read beside `path`; a URI that is absolute
/// or climbs out of that folder is refused.
#[cfg(feature = "3d")]
pub(crate) fn to_prc(path: &Path, data: &[u8], how: GltfHow) -> Result<Converted, u8> {
    use pdfcer_3d::{GltfColours, GltfOptions, GltfUpAxis, read_gltf, write_prc};
    let mut options = GltfOptions::default();
    options.colours = match how.colours {
        GltfColoursArg::Linear => GltfColours::Linear,
        GltfColoursArg::AsStored => GltfColours::AsStored,
    };
    options.up_axis = match how.up {
        GltfUpArg::Z => GltfUpAxis::ZUp,
        GltfUpArg::AsStored => GltfUpAxis::AsStored,
    };
    let dir = path.parent().unwrap_or(Path::new("."));
    let mut resolve = |uri: &str| beside(dir, uri).and_then(|p| std::fs::read(p).ok());
    let refuse = |err: &dyn std::fmt::Display| {
        eprintln!("pdfcer: {}: {err}", path.display());
        exit::EDIT_REFUSED
    };
    let import = read_gltf(data, &mut resolve, options).map_err(|e| refuse(&e))?;
    let report = write_prc(&import.scene).map_err(|e| refuse(&e))?;
    let mut lines = vec![conversion_line(how)];
    lines.extend(import.inferred.iter().map(|i| format!("inferred: {i}")));
    lines.extend(import.notes.iter().map(|n| format!("note: {n}")));
    if report.flat_normals > 0 {
        lines.push(format!(
            "inferred: {} primitive(s) had no normals and were given flat ones",
            report.flat_normals
        ));
    }
    if report.empty_primitives > 0 {
        lines.push(format!(
            "note: {} primitive(s) held no triangles and were left out",
            report.empty_primitives
        ));
    }
    if report.textured > 0 {
        lines.push(format!(
            "note: {} primitive(s) drawn with their base-colour texture",
            report.textured
        ));
    }
    Ok(Converted {
        prc: report.prc,
        triangles: report.triangles,
        lines,
    })
}

/// Without the `3d` feature: refuse, saying why.
#[cfg(not(feature = "3d"))]
pub(crate) fn to_prc(path: &Path, _data: &[u8], _how: GltfHow) -> Result<Converted, u8> {
    eprintln!(
        "pdfcer: {}: this pdfcer was built without the `3d` feature; glTF import is unavailable",
        path.display()
    );
    Err(exit::EDIT_REFUSED)
}

#[cfg(feature = "3d")]
fn conversion_line(how: GltfHow) -> String {
    let colours = match how.colours {
        GltfColoursArg::Linear => "linear base colours encoded to sRGB",
        GltfColoursArg::AsStored => "base colours used as stored",
    };
    let up = match how.up {
        GltfUpArg::Z => "glTF's +Y up turned to +Z up",
        GltfUpArg::AsStored => "axes as stored",
    };
    format!("note: glTF converted to PRC: {colours}; {up}")
}

#[cfg(feature = "3d")]
/// `uri` inside `dir`, or `None` when it is absolute, names a drive or
/// scheme, or has a `..` component.
fn beside(dir: &Path, uri: &str) -> Option<PathBuf> {
    use std::path::Component;
    let rel = Path::new(uri);
    let plain = !uri.contains(':')
        && rel
            .components()
            .all(|c| matches!(c, Component::Normal(_) | Component::CurDir));
    plain.then(|| dir.join(rel))
}

/// `3d-convert`: write the PRC a glTF model converts to.
pub(crate) fn cmd_convert_3d(input: &Path, output: &Path, how: GltfHow) -> u8 {
    let data = match std::fs::read(input) {
        Ok(d) => d,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", input.display());
            return exit::IO_ERROR;
        }
    };
    let converted = match to_prc(input, &data, how) {
        Ok(c) => c,
        Err(code) => return code,
    };
    if let Err(err) = write_output(output, &converted.prc) {
        eprintln!("pdfcer: {}: {err}", output.display());
        return exit::IO_ERROR;
    }
    println!(
        "3d-convert {} output={} format=prc triangles={} bytes={}",
        input.display(),
        output.display(),
        converted.triangles,
        converted.prc.len()
    );
    for line in &converted.lines {
        println!("{line}");
    }
    exit::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "3d")]
    #[test]
    fn external_buffers_stay_beside_the_model() {
        let dir = Path::new("models");
        assert_eq!(beside(dir, "a/b.bin"), Some(dir.join("a/b.bin")));
        assert_eq!(beside(dir, "../secret.bin"), None);
        assert_eq!(beside(dir, "a/../../x"), None);
        assert_eq!(beside(dir, "/etc/passwd"), None);
        assert_eq!(beside(dir, "C:/x.bin"), None);
        assert_eq!(beside(dir, "file://x"), None);
    }

    #[test]
    fn gltf_is_known_by_signature_or_extension() {
        assert!(is_gltf(Path::new("m.GLB"), b""));
        assert!(is_gltf(Path::new("m.gltf"), b"{"));
        assert!(is_gltf(Path::new("m.bin"), b"glTF\x02"));
        assert!(!is_gltf(Path::new("m.prc"), b"PRC"));
    }
}
