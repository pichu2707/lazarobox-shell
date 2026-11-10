//! The tool badge glyph is injected by the runtime and defaults to text.

use super::*;
use crate::core::ai::marks::ToolGlyph;

#[test]
fn the_glyph_is_text_until_the_runtime_sets_it() {
    assert_eq!(App::new(80, 25).tool_glyph(), ToolGlyph::Text);
}

#[test]
fn the_runtime_sets_the_glyph_once() {
    let a = App::new(80, 25).with_tool_glyph(ToolGlyph::Emoji);
    assert_eq!(a.tool_glyph(), ToolGlyph::Emoji);
}
