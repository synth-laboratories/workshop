//! `sourced.visual.v1`: agent TSX stored as canonical source, compiled in the pane.

pub const TEMPLATE_ID: &str = "sourced.visual.v1";
pub const KIND: &str = "sourced_visual";
pub const PROTOCOL_ID: &str = "whole_file.v1";
pub const MAX_SOURCE_BYTES: usize = 256 * 1024;
pub const MEDIA_TYPE_SOURCE: &str = "text/tsx";

pub fn is_sourced_template(template_id: &str) -> bool {
    template_id == TEMPLATE_ID
}
