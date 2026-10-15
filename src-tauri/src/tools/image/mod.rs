pub mod compress;
pub mod convert;
pub mod decode;
pub mod encode;
pub mod logo_pack;
pub mod optimize_png;
pub mod palette;
pub mod resize;
pub mod svg;
pub mod trace;
pub mod webp;

/// Shared image dimension guards used by both raster decode and SVG render.
pub const MAX_DIMENSION: u32 = 16_384;
pub const MAX_PIXELS: u64 = 64_000_000;

#[cfg(test)]
pub(crate) mod test_util;
