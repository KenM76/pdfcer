use super::*;

/// Arguments for [`cmd_add_image`], grouped so the dispatcher stays one
/// expression and `clippy::too_many_arguments` is satisfied honestly rather
/// than by an `#[allow]`.
pub(crate) struct AddImageArgs<'a> {
    pub(crate) input: &'a Path,
    pub(crate) image: &'a Path,
    pub(crate) page: usize,
    pub(crate) rect: &'a str,
    pub(crate) stretch: bool,
    pub(crate) natural: bool,
    pub(crate) compression: pdfcer_core::image_import::ImageCompression,
    pub(crate) output: &'a Path,
    pub(crate) mode: SaveMode,
    pub(crate) verify_undo: bool,
}

/// `add-image` — place a raster image on a page (§8.9.5).
///
/// ## Contract
///
/// - Emits one `add-image …` line on stdout carrying **every** disclosure as
///   a field, then defers the exit code to [`finish_edit`].
/// - Emits the prose form of the same disclosures on stderr.
/// - Every refusal — an unreadable file, an unsupported format, an
///   unsupported sub-feature, a degenerate rectangle, a page out of range, a
///   certified document — is reported BEFORE any mutation.
/// - `--page` is 1-BASED here and 0-based in the core call, matching every
///   other page-taking subcommand.
///
/// ## Why both channels carry the same facts
///
/// This is the `add-push-button` precedent generalised. A disclosure
/// delivered only as an English sentence on stderr is invisible to a script
/// that captures stdout and discards stderr — and image placement has
/// several disclosures a batch job genuinely needs to act on: that a JPEG
/// was embedded verbatim (`verbatim=1`) or a BMP re-compressed
/// (`recompressed=`), that a soft mask was written that pdfcer's own renderer
/// will not show (`smask_not_previewed=1`), that an image is being enlarged
/// past its own resolution (`low_res=1`), or that a CMYK JPEG's polarity is
/// undeclared (`cmyk_polarity_unverifiable=1`, R30). Those belong in the
/// machine-readable line, not only in the human one.
pub(crate) fn cmd_add_image(args: &AddImageArgs<'_>) -> u8 {
    use pdfcer_core::edit::NewImage;
    use pdfcer_core::image_import;

    let (page_index, requested) = match parse_page_and_rect(args.input, args.page, args.rect) {
        Ok(pair) => pair,
        Err(code) => return code,
    };

    // Read and parse the image BEFORE opening the PDF: a bad image is by far
    // the likelier mistake, and diagnosing it without having parsed a
    // possibly-large document first is both faster and clearer.
    let bytes = match std::fs::read(args.image) {
        Ok(b) => b,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", args.image.display());
            return exit::IO_ERROR;
        }
    };
    let options = image_import::ImportOptions::new().with_compression(args.compression);
    let img = match image_import::import_with(&bytes, &options) {
        Ok(img) => img,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", args.image.display());
            if let image_import::ImageImportError::Unsupported { feature } = &err {
                eprintln!(
                    "pdfcer: the unsupported feature is {feature}. pdfcer places \
                     {}; re-saving the file without that feature is usually enough.",
                    image_import::SUPPORTED_FORMATS
                );
            }
            return exit::EDIT_REFUSED;
        }
    };

    // `--natural` keeps the rectangle's lower-left corner and replaces its
    // SIZE. Applied here rather than in the core so the core's placement
    // contract stays "the caller's rectangle, fitted" with no metadata-driven
    // branch inside it.
    let rect = if args.natural {
        let (w, h) = img.natural_size_pt();
        pdfcer_core::page_tree::Rect {
            llx: requested.llx,
            lly: requested.lly,
            urx: requested.llx + w,
            ury: requested.lly + h,
        }
    } else {
        requested
    };

    let (source, mut session) = match open_for_edit(args.input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };

    let mut spec = NewImage::new(page_index, rect, &img);
    if args.stretch || args.natural {
        // `--natural` implies an exact fit: the rectangle WAS computed from
        // the image's own aspect ratio, so "contain" would be a no-op that
        // could still round its way into a spurious letterboxed= report.
        spec = spec.stretching();
    }
    let placed = match session.add_image(&spec) {
        Ok(outcome) => outcome,
        Err(err) => return report_edit_error(args.input, &err),
    };
    report_image_disclosures(args.image, &placed);

    let outcome = match save_edited(
        &mut session,
        &source,
        args.output,
        args.mode,
        ProducerArg::Preserve,
        args.verify_undo,
    ) {
        Ok(outcome) => outcome,
        Err(code) => return code,
    };

    let d = &placed.disclosures;
    let p = placed.placed_rect;
    let (px_w, px_h) = img.display_size_px();
    let r = &outcome.report;
    println!(
        "add-image {} image={} format={} page={} pixels={}x{} bpc={} colorspace={} \
         filter={} verbatim={} rect={},{},{},{} placed={:.3},{:.3},{:.3},{:.3} fit={} \
         image_obj={} {} smask={} name={} dpi={} dpi_source={} eff_dpi={:.1},{:.1} \
         compression_requested={} compression_applied={} quality={} \
         source_bytes={} stored_bytes={} lossless_from_lossy={} jpeg_from_lossy={} \
         letterboxed={} distorted={} low_res={} recompressed={} smask_written={} \
         transparency_not_previewed={} colour_key={} profile_dropped={} \
         cmyk_polarity_unverifiable={} progressive={} exif_orientation={} \
         bmp_padding_ignored={} version_needed={} tagged={} mode={} -> {}; \
         changed={} objects={} appended={} out_bytes={} undo_verified={} undo_identical={}",
        args.input.display(),
        args.image.display(),
        img.format.name(),
        args.page,
        px_w,
        px_h,
        img.bits_per_component,
        colorspace_name(&img.color_space),
        filter_name(img.filter),
        u32::from(d.recompressed.is_none()),
        requested.llx,
        requested.lly,
        requested.urx,
        requested.ury,
        p.llx,
        p.lly,
        p.urx,
        p.ury,
        if args.stretch || args.natural {
            "stretch"
        } else {
            "contain"
        },
        placed.image_id.num,
        placed.image_id.generation,
        placed.soft_mask_id.map_or_else(
            || "-".to_owned(),
            |id| format!("{} {}", id.num, id.generation)
        ),
        String::from_utf8_lossy(&placed.resource_name),
        img.dpi
            .map_or_else(|| "-".to_owned(), |(x, y)| format!("{x:.1},{y:.1}")),
        dpi_source_key(img.notes.dpi_source),
        d.effective_dpi.0,
        d.effective_dpi.1,
        d.requested_compression.key(),
        d.applied_compression.key(),
        d.jpeg_quality
            .map_or_else(|| "-".to_owned(), |q| q.to_string()),
        d.source_bytes,
        d.stored_bytes,
        u32::from(d.lossless_from_lossy),
        u32::from(d.jpeg_from_lossy),
        u32::from(d.letterboxed),
        u32::from(d.aspect_distorted),
        u32::from(d.below_screen_resolution),
        d.recompressed.map_or("-", |r| r.key()),
        u32::from(d.soft_mask_written),
        u32::from(d.transparency_not_previewed),
        u32::from(d.colour_key_mask_written),
        u32::from(d.colour_profile_dropped),
        u32::from(d.cmyk_polarity_unverifiable),
        u32::from(d.progressive_jpeg),
        d.exif_orientation_applied
            .map_or_else(|| "-".to_owned(), |v| v.to_string()),
        u32::from(d.bmp_fourth_byte_ignored),
        d.version_ahead_of_document.map_or_else(
            || "-".to_owned(),
            |(f, doc)| {
                let (major, minor) = f.since();
                format!("{major}.{minor}(doc {doc})")
            }
        ),
        u32::from(d.tagged_document),
        args.mode.name(),
        args.output.display(),
        outcome.changed,
        r.objects_written,
        r.bytes_appended,
        r.bytes_written,
        u32::from(outcome.undo_verified),
        u32::from(outcome.undo_identical),
    );
    finish_edit(args.input, &outcome)
}

/// Why a compression policy was substituted, in one clause.
///
/// The substitution itself is reported unconditionally; this supplies the
/// *reason*, which is the part an operator can act on. Both directions are
/// real: a passthrough that could not happen, and a lossless re-encode that
/// turned out to be unnecessary.
pub(crate) fn compression_substitution_reason(
    d: &pdfcer_core::edit::ImageAuthorDisclosures,
) -> &'static str {
    use pdfcer_core::image_import::RecompressReason;
    match d.recompressed {
        Some(RecompressReason::NoCompressedSource) => {
            "This format stores no compressed bytes to pass through, so pdfcer compressed it losslessly on the way in — the pixels are unchanged."
        }
        Some(RecompressReason::AlphaSplit) => {
            "This image's alpha channel is interleaved with its colour channels and PDF carries opacity in a separate image, so the samples had to be split and re-compressed — losslessly; the pixels are unchanged."
        }
        None => {
            "The source was already stored losslessly, so its own bytes were kept unchanged rather than re-compressed — strictly better than what was asked for."
        }
        // `LosslessRequested` and `JpegRequested` are policies the operator
        // CHOSE, so a "substitution" involving them is not a substitution at
        // all — their own dedicated notes below say what the choice cost.
        Some(_) => "See the note above for why.",
    }
}

/// The `/ColorSpace` an imported image landed in, as a stable key.
pub(crate) fn colorspace_name(cs: &pdfcer_core::image_import::ImportColorSpace) -> &'static str {
    use pdfcer_core::image_import::ImportColorSpace;
    match cs {
        ImportColorSpace::DeviceGray => "DeviceGray",
        ImportColorSpace::DeviceRgb => "DeviceRGB",
        ImportColorSpace::DeviceCmyk => "DeviceCMYK",
        ImportColorSpace::Indexed { .. } => "Indexed",
        // `ImportColorSpace` is `#[non_exhaustive]`, so a wildcard is
        // required. It reports "?" rather than guessing a plausible name:
        // a new colour space reaching this line means the CLI's vocabulary
        // is out of date, and a wrong-but-plausible label would hide that
        // from the script reading the field.
        _ => "?",
    }
}

/// The `/Filter` (and predictor, where there is one) as a stable key.
pub(crate) fn filter_name(f: pdfcer_core::image_import::ImportFilter) -> &'static str {
    use pdfcer_core::image_import::ImportFilter;
    match f {
        ImportFilter::DctDecode => "DCTDecode",
        ImportFilter::Flate => "FlateDecode",
        ImportFilter::FlatePngPredictor { .. } => "FlateDecode+Predictor15",
        // See `colorspace_name` for why the wildcard reports "?".
        _ => "?",
    }
}

/// Where a declared resolution came from, as a stable key. `assumed` is not
/// a claim the file made — it is pdfcer saying it had nothing to go on.
pub(crate) fn dpi_source_key(s: pdfcer_core::image_import::DpiSource) -> &'static str {
    use pdfcer_core::image_import::DpiSource;
    match s {
        DpiSource::Assumed => "assumed-72",
        DpiSource::JfifDensity => "jfif",
        DpiSource::ExifResolution => "exif",
        DpiSource::PngPhys => "png-phys",
        DpiSource::BmpPelsPerMeter => "bmp-ppm",
        // See `colorspace_name` for why the wildcard reports "?".
        _ => "?",
    }
}

/// Print, on stderr, every disclosure an image placement owes the operator.
///
/// One function rather than a run of inline `eprintln!`s at the call site,
/// for the same reason [`report_field_disclosures`] is one: a disclosure
/// that is never printed looks exactly like a disclosure that did not apply,
/// so the set has to live in one place where it can be read as a set.
pub(crate) fn report_image_disclosures(
    image: &Path,
    outcome: &pdfcer_core::edit::ImageAuthorOutcome,
) {
    let d = &outcome.disclosures;
    let name = image.display();
    if d.requested_compression != d.applied_compression {
        eprintln!(
            "pdfcer: {name}: --compression {} was asked for and {} was used. {}",
            d.requested_compression.key(),
            d.applied_compression.key(),
            compression_substitution_reason(d),
        );
    }
    if d.lossless_from_lossy {
        eprintln!(
            "pdfcer: {name}: this is a LOSSY source stored losslessly, as asked. That preserves exactly the pixels it decodes to — compression artefacts included — and RECOVERS NOTHING that was already lost, while typically multiplying the stored size several-fold. If the goal was a better picture rather than a stable one to edit, --compression passthrough is the cheaper answer."
        );
    }
    // The lossy re-encode, stated in two sentences rather than one, because
    // "your bytes changed" and "your picture got worse, twice over" are
    // different facts and only the second is unrecoverable.
    if d.recompressed == Some(pdfcer_core::image_import::RecompressReason::JpegRequested) {
        let q = d
            .jpeg_quality
            .map_or_else(|| "?".to_owned(), |q| q.to_string());
        eprintln!(
            "pdfcer: {name}: re-encoded as JPEG at quality {q}, as asked — {} bytes in, {} bytes stored. This is LOSSY: the stored picture is not the one you handed over, and no later step can recover the difference.",
            d.source_bytes, d.stored_bytes
        );
        if d.jpeg_from_lossy {
            eprintln!(
                "pdfcer: {name}: that source was ALREADY a JPEG, so this is a SECOND lossy pass. The DCT has now run twice, and the second pass quantises the first pass's ringing and blocking INTO the picture rather than smoothing them out — the damage compounds, it is invisible at editing zoom, and raising --quality does not undo it. If a lossless original exists, place that instead."
            );
        }
    }
    if d.letterboxed {
        let p = outcome.placed_rect;
        eprintln!(
            "pdfcer: {name}: the image kept its shape and was CENTRED in the rectangle, so it landed at {:.2},{:.2},{:.2},{:.2} rather than filling it. Pass --stretch to fill the rectangle exactly.",
            p.llx, p.lly, p.urx, p.ury
        );
    }
    if d.aspect_distorted {
        eprintln!(
            "pdfcer: {name}: --stretch was used and the rectangle's shape differs from the image's, so the image is DISTORTED — as asked."
        );
    }
    if d.below_screen_resolution {
        eprintln!(
            "pdfcer: {name}: at this size the image works out to {:.0} × {:.0} dpi, below one image pixel per point — it is being enlarged past its own resolution and will look soft in print.",
            d.effective_dpi.0, d.effective_dpi.1
        );
    }
    match d.recompressed {
        None => {}
        Some(pdfcer_core::image_import::RecompressReason::AlphaSplit) => eprintln!(
            "pdfcer: {name}: this image's alpha channel is interleaved with its colour channels, and PDF carries opacity in a SEPARATE image, so the samples were split and re-compressed. The pixels are unchanged — the re-compression is lossless — but the embedded bytes are no longer the file's own."
        ),
        Some(pdfcer_core::image_import::RecompressReason::NoCompressedSource) => eprintln!(
            "pdfcer: {name}: a BMP is uncompressed, so pdfcer compressed it on the way in. The pixels are unchanged and the result is far smaller, but the embedded bytes are no longer the file's own."
        ),
        // Deliberately silent: the operator ASKED for this one, and the
        // `lossless_from_lossy` disclosure above already says what it cost.
        // A second sentence restating the same fact is how a disclosure set
        // trains people to skim.
        Some(pdfcer_core::image_import::RecompressReason::LosslessRequested) => {}
        // Likewise: asked for, and already reported above with the size
        // change and the generation-loss warning it earns.
        Some(pdfcer_core::image_import::RecompressReason::JpegRequested) => {}
        // `RecompressReason` is `#[non_exhaustive]`. A future reason must
        // still SAY that a re-compression happened, even before this CLI
        // learns to explain it — silence is the one unacceptable answer.
        Some(other) => eprintln!(
            "pdfcer: {name}: pdfcer decoded and re-compressed this image ({}). The pixels are unchanged, but the embedded bytes are no longer the file's own.",
            other.key()
        ),
    }
    if d.soft_mask_written {
        eprintln!(
            "pdfcer: {name}: the transparency was written as a soft mask (/SMask), not flattened against white — the page shows through, as it should."
        );
    }
    // The `transparency_not_previewed` note is GONE — `pdfcer-render` now
    // composites both `/SMask` and colour-key `/Mask`, so the field is
    // retired to a constant `false` in core and there is nothing to say.
    // The stdout field survives (a stable-line key is a contract) and
    // reports `0`; only the prose is removed, because prose that fires when
    // nothing is wrong is what teaches an operator to stop reading it.
    if d.colour_key_mask_written {
        eprintln!(
            "pdfcer: {name}: the image declared one fully-transparent colour, written as a colour-key /Mask. The image data itself was embedded unchanged."
        );
    }
    if d.colour_profile_dropped {
        eprintln!(
            "pdfcer: {name}: this file carries embedded colour-management data (an ICC profile or a gamma/chromaticity claim) that pdfcer did NOT carry over — the image was placed in a device colour space, so colours may shift slightly."
        );
    }
    if d.cmyk_polarity_unverifiable {
        eprintln!(
            "pdfcer: {name}: this is a four-component (CMYK) JPEG whose stored polarity NOTHING in the file declares. pdfcer embedded it unchanged and wrote no /Decode array, which is what every production PDF engine does. If it appears as a photographic negative, that is why — and the fix belongs in the source file, not here."
        );
    }
    if d.progressive_jpeg {
        eprintln!(
            "pdfcer: {name}: this is a progressive JPEG. It is legal inside a PDF from version 1.3 and was embedded unchanged, but it decodes more slowly and uses more memory than a baseline one. Re-save it as baseline if the document will be opened often."
        );
    }
    if let Some(o) = d.exif_orientation_applied {
        eprintln!(
            "pdfcer: {name}: this image carries EXIF orientation {o}, which pdfcer applied by turning the PLACEMENT rather than re-encoding the pixels — so it appears the right way up at no cost in quality."
        );
    }
    if d.bmp_fourth_byte_ignored {
        eprintln!(
            "pdfcer: {name}: this 32-bit BMP's fourth byte per pixel was ignored. A BI_RGB bitmap has no alpha channel — that byte is padding, and treating it as opacity would have made the image invisible."
        );
    }
    if let Some((feature, doc)) = d.version_ahead_of_document {
        let (major, minor) = feature.since();
        eprintln!(
            "pdfcer: {name}: this image uses {} (PDF {major}.{minor}) in a document that declares PDF {doc}. pdfcer did NOT rewrite the document's version header — that is a structural change you did not ask for. Readers are overwhelmingly version-tolerant, so the image will almost certainly display.",
            feature.name()
        );
    }
    if d.tagged_document {
        eprintln!(
            "pdfcer: {name}: this document is tagged (/StructTreeRoot) and the new image is NOT in its structure tree, so it has no alternate text and assistive technology cannot describe it. pdfcer does not write structure elements."
        );
    }
}
