//! glTF to PRC, for `3d-convert` and `3d-embed --model x.gltf|x.glb`.

use super::*;

/// The glTF reading settings both commands take.
// Without `3d` the settings are parsed but there is nothing to apply them to.
#[cfg_attr(not(feature = "3d"), allow(dead_code))]
#[derive(Clone, Copy, Debug)]
pub(crate) struct GltfHow<'a> {
    pub(crate) colours: GltfColoursArg,
    pub(crate) up: GltfUpArg,
    pub(crate) occlusion: GltfOcclusionArg,
    pub(crate) lights: GltfLightsArg,
    /// `--light`: lights added after the file's own.
    pub(crate) added: &'a [super::LightArg],
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
    /// The model lights written, the file's and `--light`'s.
    pub(crate) lights: usize,
}

/// Convert the glTF `data` read from `path`, or the exit code after the
/// reason. External buffers and images are read beside `path`; a URI that
/// is absolute or climbs out of that folder is refused, and so is a file
/// whose real path (links followed) lies outside it.
#[cfg(feature = "3d")]
pub(crate) fn to_prc(path: &Path, data: &[u8], how: GltfHow<'_>) -> Result<Converted, u8> {
    use pdfcer_3d::{read_gltf, write_prc};
    let dir = path
        .parent()
        .filter(|d| !d.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let real_dir = dir.canonicalize().ok();
    let mut outside = Vec::new();
    let mut resolve = |uri: &str| match read_beside(dir, real_dir.as_deref(), uri) {
        Ok(bytes) => Some(bytes),
        Err(Unread::Outside) => {
            outside.push(uri.to_owned());
            None
        }
        Err(Unread::Missing) => None,
    };
    let refuse = |err: &dyn std::fmt::Display| {
        eprintln!("pdfcer: {}: {err}", path.display());
        exit::EDIT_REFUSED
    };
    let read = read_gltf(data, &mut resolve, options(how));
    if let Some(uri) = outside.first() {
        return Err(refuse(&format!(
            "{uri} leads outside the model's folder (through a link), so it was not read"
        )));
    }
    let mut import = read.map_err(|e| refuse(&e))?;
    let lights = &mut import.scene.lights;
    lights.extend(how.added.iter().map(super::LightArg::to_model));
    let lights = lights.len();
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
    if !how.added.is_empty() {
        lines.push(format!("note: {} --light light(s) added", how.added.len()));
    }
    if report.textured > 0 {
        lines.push(format!(
            "note: {} primitive(s) drawn with a texture",
            report.textured
        ));
    }
    Ok(Converted {
        prc: report.prc,
        triangles: report.triangles,
        lines,
        lights,
    })
}

/// Without the `3d` feature: refuse, saying why.
#[cfg(not(feature = "3d"))]
pub(crate) fn to_prc(path: &Path, _data: &[u8], _how: GltfHow<'_>) -> Result<Converted, u8> {
    eprintln!(
        "pdfcer: {}: this pdfcer was built without the `3d` feature; glTF import is unavailable",
        path.display()
    );
    Err(exit::EDIT_REFUSED)
}

#[cfg(feature = "3d")]
fn conversion_line(how: GltfHow<'_>) -> String {
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

/// The importer settings `how` asks for.
#[cfg(feature = "3d")]
fn options(how: GltfHow<'_>) -> pdfcer_3d::GltfOptions {
    use pdfcer_3d::{GltfColours, GltfLights, GltfOcclusion, GltfOptions, GltfUpAxis};
    let mut options = GltfOptions::default();
    options.occlusion = match how.occlusion {
        GltfOcclusionArg::Bake => GltfOcclusion::Bake,
        GltfOcclusionArg::Drop => GltfOcclusion::Drop,
    };
    options.colours = match how.colours {
        GltfColoursArg::Linear => GltfColours::Linear,
        GltfColoursArg::AsStored => GltfColours::AsStored,
    };
    options.up_axis = match how.up {
        GltfUpArg::Z => GltfUpAxis::ZUp,
        GltfUpArg::AsStored => GltfUpAxis::AsStored,
    };
    options.lights = match how.lights {
        GltfLightsArg::Exposure => GltfLights::Exposure,
        GltfLightsArg::Normalise => GltfLights::Normalise,
        GltfLightsArg::Stored => GltfLights::Stored,
        GltfLightsArg::Drop => GltfLights::Drop,
    };
    options
}

/// Why a resource beside the model was not read.
#[cfg(feature = "3d")]
#[derive(Debug, PartialEq, Eq)]
enum Unread {
    /// Refused by [`beside`], or the file could not be read.
    Missing,
    /// Its real path lies outside the model's folder: a link leads out.
    Outside,
}

/// The bytes of `uri` beside the model in `dir`, whose real path is
/// `real_dir`. The resource is read only when its real path, links
/// followed, starts with `real_dir`, so a link beside the model cannot
/// lead out of its folder.
#[cfg(feature = "3d")]
fn read_beside(dir: &Path, real_dir: Option<&Path>, uri: &str) -> Result<Vec<u8>, Unread> {
    let joined = beside(dir, uri).ok_or(Unread::Missing)?;
    let real = joined.canonicalize().map_err(|_| Unread::Missing)?;
    if !real_dir.is_some_and(|d| real.starts_with(d)) {
        return Err(Unread::Outside);
    }
    std::fs::read(real).map_err(|_| Unread::Missing)
}

/// `3d-convert`: write the PRC a glTF model converts to.
pub(crate) fn cmd_convert_3d(input: &Path, output: &Path, how: GltfHow<'_>) -> u8 {
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
        "3d-convert {} output={} format=prc triangles={} lights={} bytes={}",
        input.display(),
        output.display(),
        converted.triangles,
        converted.lights,
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

    /// Makes a file link at `link` to `target`; false where links cannot be made (Windows
    /// without developer mode).
    #[cfg(feature = "3d")]
    fn link(target: &Path, link: &Path) -> bool {
        #[cfg(windows)]
        let made = std::os::windows::fs::symlink_file(target, link);
        #[cfg(unix)]
        let made = std::os::unix::fs::symlink(target, link);
        #[cfg(not(any(windows, unix)))]
        let made: std::io::Result<()> = Err(std::io::ErrorKind::Unsupported.into());
        made.is_ok()
    }

    #[cfg(feature = "3d")]
    #[test]
    fn a_link_beside_the_model_cannot_lead_out_of_its_folder() {
        let root = std::env::temp_dir().join(format!("pdfcer-gltf-link-{}", std::process::id()));
        let (model, elsewhere) = (root.join("model"), root.join("elsewhere"));
        std::fs::create_dir_all(&model).unwrap();
        std::fs::create_dir_all(&elsewhere).unwrap();
        std::fs::write(elsewhere.join("secret.bin"), b"secret").unwrap();
        std::fs::write(model.join("mesh.bin"), b"mesh").unwrap();
        let real = model.canonicalize().unwrap();
        assert_eq!(
            read_beside(&model, Some(&real), "mesh.bin"),
            Ok(b"mesh".to_vec())
        );
        if link(&elsewhere.join("secret.bin"), &model.join("link.bin")) {
            assert_eq!(
                read_beside(&model, Some(&real), "link.bin"),
                Err(Unread::Outside)
            );
        } else {
            eprintln!("SKIP: links cannot be made here");
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn gltf_is_known_by_signature_or_extension() {
        assert!(is_gltf(Path::new("m.GLB"), b""));
        assert!(is_gltf(Path::new("m.gltf"), b"{"));
        assert!(is_gltf(Path::new("m.bin"), b"glTF\x02"));
        assert!(!is_gltf(Path::new("m.prc"), b"PRC"));
    }
}
