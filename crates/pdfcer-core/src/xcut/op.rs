//! [`Op`]: the one list of operations every [`ObjectKind`](super::ObjectKind)
//! is checked against, grouped as the policy tables and the matrix are.

/// Defines [`Op`], [`OpGroup`], their `ALL` lists, stable ids and the
/// op-to-group map from one table.
macro_rules! ops {
    ($( $(#[$gmeta:meta])* $group:ident => $gid:literal { $( $(#[$meta:meta])* $variant:ident => $id:literal, )* } )*) => {
        /// A family of operations: one policy table and one matrix section
        /// each.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub enum OpGroup {
            $( $(#[$gmeta])* $group, )*
        }

        impl OpGroup {
            /// Every group, in matrix order.
            pub const ALL: &'static [OpGroup] = &[$(OpGroup::$group,)*];

            /// The stable id (the matrix section heading).
            #[must_use]
            pub const fn id(self) -> &'static str {
                match self { $(Self::$group => $gid,)* }
            }

            /// The operations in this group, in matrix column order.
            #[must_use]
            pub const fn ops(self) -> &'static [Op] {
                match self { $(Self::$group => &[$(Op::$variant,)*],)* }
            }
        }

        /// An operation whose effect on every [`ObjectKind`](super::ObjectKind)
        /// is declared by [`expect`](super::expect) (decision 228).
        ///
        /// Not `#[non_exhaustive]`, for the same reason
        /// [`ObjectKind`](super::ObjectKind) is not.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub enum Op {
            $($( $(#[$meta])* $variant, )*)*
        }

        impl Op {
            /// Every operation, in matrix column order.
            pub const ALL: &'static [Op] = &[$($(Op::$variant,)*)*];

            /// The stable kebab-case id (the matrix column key).
            #[must_use]
            pub const fn id(self) -> &'static str {
                match self { $($(Self::$variant => $id,)*)* }
            }

            /// The group this operation's policy lives in.
            #[must_use]
            pub const fn group(self) -> OpGroup {
                match self { $($(Self::$variant => OpGroup::$group,)*)* }
            }
        }
    };
}

ops! {
    /// Copy, cut, paste, duplicate, and a save-and-reopen of the result.
    Clipboard => "clipboard" {
        /// Copy to a clip, then paste into a different document.
        Copy => "copy",
        /// Cut (copy and delete as one undo entry), then paste.
        Cut => "cut",
        /// Paste a clip onto another page of the same document, offset.
        Paste => "paste",
        /// Copy and paste onto the same page, offset.
        Duplicate => "duplicate",
        /// A full rewrite and a reopen.
        ReloadRoundTrip => "reload",
    }
    /// The undo stack and the two save modes.
    History => "history" {
        /// Undo the last command.
        Undo => "undo",
        /// Redo the last undone command.
        Redo => "redo",
        /// An incremental save (§7.5.6).
        SaveIncremental => "save-incremental",
        /// A full rewrite.
        SaveFull => "save-full",
    }
    /// Geometry and arrangement verbs.
    Arrange => "arrange" {
        /// Translate.
        Move => "move",
        /// Rotate, scale, skew.
        Transform => "transform",
        /// Align and distribute a selection.
        AlignDistribute => "align",
        /// Change paint or `/Annots` order.
        Restack => "restack",
        /// Group a selection.
        Group => "group",
        /// Ungroup.
        Ungroup => "ungroup",
        /// Move onto a layer (§8.11).
        LayerAssign => "layer",
    }
    /// Deletion and the page-level routes.
    Pages => "pages" {
        /// Delete the object.
        Delete => "delete",
        /// Insert pages from another document.
        PageInsert => "page-insert",
        /// Extract pages to a new document.
        PageExtract => "page-extract",
        /// Merge documents.
        PageMerge => "page-merge",
        /// Rotate a page.
        PageRotate => "page-rotate",
        /// Reorder pages.
        PageReorder => "page-reorder",
    }
    /// Operations whose contract is that content goes away.
    Destructive => "destructive" {
        /// Apply redaction.
        Redact => "redact",
        /// Sanitize (remove hidden information).
        Sanitize => "sanitize",
    }
    /// Operations that turn the object into something else: page content,
    /// another format, a device.
    Output => "output" {
        /// Flatten into page content.
        Flatten => "flatten",
        /// Export to SVG.
        ExportSvg => "export-svg",
        /// Export to DOCX.
        ExportDocx => "export-docx",
        /// Export to a raster image.
        ExportImage => "export-image",
        /// Print.
        Print => "print",
        /// Convert to PDF/A.
        PdfaConvert => "pdfa",
    }
    /// Operations that read the object or expose it.
    Inspect => "inspect" {
        /// Structure tagging.
        StructureTags => "tags",
        /// Text search.
        Search => "search",
        /// Document compare.
        Compare => "compare",
        /// Hit-testing a point.
        HitTest => "hit-test",
        /// Reading the object's properties.
        PropertiesRead => "props-read",
        /// Writing the object's properties.
        PropertiesWrite => "props-write",
        /// The CLI verb that reaches it.
        CliVerb => "cli",
        /// The `docs/core-api/` entry that documents it.
        CoreApiDoc => "core-api",
    }
}

impl std::fmt::Display for Op {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.id())
    }
}
