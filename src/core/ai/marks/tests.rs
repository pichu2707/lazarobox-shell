use super::*;

#[test]
fn params_of_reads_the_ollama_size_strings() {
    assert_eq!(params_of("8.0B"), Some(8_000_000_000));
    assert_eq!(params_of("137M"), Some(137_000_000));
    assert_eq!(params_of("70.6B"), Some(70_600_000_000));
    assert_eq!(params_of("8x7B"), Some(56_000_000_000));
}

#[test]
fn params_of_rejects_what_is_not_a_size() {
    assert_eq!(params_of(""), None);
    assert_eq!(params_of("junk"), None);
    assert_eq!(params_of("B"), None);
}

#[test]
fn tools_and_three_billion_or_more_is_capable() {
    assert_eq!(
        tool_mark(Tools::Yes, Some(8_000_000_000)),
        Some(ToolMark::Capable)
    );
    assert_eq!(
        tool_mark(Tools::Yes, Some(3_000_000_000)),
        Some(ToolMark::Capable)
    );
}

#[test]
fn tools_under_three_billion_is_unreliable() {
    assert_eq!(
        tool_mark(Tools::Yes, Some(2_900_000_000)),
        Some(ToolMark::Unreliable)
    );
    assert_eq!(
        tool_mark(Tools::Yes, Some(137_000_000)),
        Some(ToolMark::Unreliable)
    );
}

#[test]
fn no_tools_unknown_or_unknown_size_gives_no_mark() {
    assert_eq!(tool_mark(Tools::No, Some(8_000_000_000)), None);
    assert_eq!(tool_mark(Tools::Unknown, Some(8_000_000_000)), None);
    assert_eq!(tool_mark(Tools::Yes, None), None);
}
