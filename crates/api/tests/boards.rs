use hermes_dash_api::boards::parse_boards;
use hermes_dash_core::source::SourceError;

#[test]
fn parses_observed_table_with_marker_and_deduplicates_in_order() {
    let text = "    SLUG                      NAME                          COUNTS\n●   default                   Default                       done=4\n    reporty                   Reporty                       todo=4\n    zaha                      Zaha Team                     running=2\n    default                   Duplicate                     done=0\n\nCurrent board: default\nSwitch boards with `hermes kanban boards switch <slug>`.\n";
    assert_eq!(parse_boards(text).unwrap(), ["default", "reporty", "zaha"]);
}

#[test]
fn parses_without_marker_and_variable_name_spacing() {
    assert_eq!(
        parse_boards("SLUG NAME COUNTS\nzaha    Zaha   Team    done=2\n").unwrap(),
        ["zaha"]
    );
}

#[test]
fn accepts_header_only_empty_board_table() {
    assert!(parse_boards("SLUG NAME COUNTS\n").unwrap().is_empty());
}

#[test]
fn rejects_unrecognized_or_invalid_rows() {
    for text in [
        "",
        "stderr sentinel",
        "SLUG NAME COUNTS\nbad.slug Name done=1",
        "SLUG NAME COUNTS\nzaha",
        "SLUG NAME COUNTS\nzaha Name nonsense",
    ] {
        assert_eq!(parse_boards(text), Err(SourceError::InvalidData));
    }
}
