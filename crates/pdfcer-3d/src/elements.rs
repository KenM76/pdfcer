//! The decoded-element allowance: a bound on what one file structure's
//! readers may build, independent of how few input bits describe it.

use crate::PrcError;

/// Decoded elements one reading pass may build from one file structure:
/// its schema (each entry costs 1 plus its token count), and the
/// triangles its tessellations produce (1 each, compressed or not). The
/// schema, tree and globals readers of a walk share one allowance per file
/// structure, as do the schema and tessellation readers of
/// [`FileStructure::tessellations`](crate::FileStructure::tessellations).
/// The model-file stream has its own. About 46x the largest measured need:
/// 181,006 elements, a building model in one file structure.
pub const MAX_DECODED_ELEMENTS: u64 = 1 << 23;

/// Charge `n` elements to `spent`; refused, before anything is built, once
/// the total would pass [`MAX_DECODED_ELEMENTS`].
pub(crate) fn charge(spent: &mut u64, n: u64) -> Result<(), PrcError> {
    let total = spent.saturating_add(n);
    if total > MAX_DECODED_ELEMENTS {
        return Err(PrcError::TooManyElements {
            limit: MAX_DECODED_ELEMENTS,
        });
    }
    *spent = total;
    Ok(())
}
