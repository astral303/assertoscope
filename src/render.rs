//! Renders the failure diagram: the assertion on one line, a row of pipes,
//! and each recorded value under the token it belongs to. Values too wide
//! for the diagram are numbered and listed below it.

use crate::capture::TRUNCATION_MARKER;
use crate::recorder::Layout;

/// Widest value shown inside the diagram; wider values move to the legend.
pub const MAX_INLINE_WIDTH: usize = 60;

const INDENT: &str = "    ";
const PIPE: char = '|';

/// A recorded value and the subexpression it belongs to.
struct Entry<'a> {
    column: usize,
    label: &'a str,
    text: String,
}

/// A value's text in the diagram and the row it was placed on.
struct Placement {
    column: usize,
    row: usize,
    text: String,
}

/// Renders the diagram and legend for `values`, indexed like `layout.slots`.
#[must_use]
pub fn render(layout: &Layout<'_>, values: &[Option<String>]) -> String {
    let mut entries: Vec<Entry<'_>> = layout
        .slots
        .iter()
        .zip(values)
        .filter_map(|(slot, value)| {
            value.as_ref().map(|text| Entry {
                column: slot.column,
                label: &layout.source[slot.start..slot.end],
                text: text.clone(),
            })
        })
        .collect();
    entries.sort_by_key(|entry| entry.column);

    let mut legend: Vec<&Entry<'_>> = Vec::new();
    let mut inline: Vec<(usize, String)> = Vec::new();
    for entry in &entries {
        if fits_inline(&entry.text) {
            inline.push((entry.column, entry.text.clone()));
        } else {
            legend.push(entry);
            inline.push((entry.column, format!("#{}", legend.len())));
        }
    }

    let mut output = format!("{INDENT}{}", layout.source);
    for row in diagram_rows(&place(inline)) {
        output.push('\n');
        output.push_str(INDENT);
        output.push_str(row.trim_end());
    }
    for (index, entry) in legend.iter().enumerate() {
        output.push_str("\n\n");
        output.push_str(&legend_entry(index + 1, entry));
    }
    output
}

fn fits_inline(text: &str) -> bool {
    text.chars().count() <= MAX_INLINE_WIDTH
        && !text.contains('\n')
        && debug_string_lines(text).is_none()
}

/// Assigns each value a row, from the rightmost value leftwards: the first
/// row where the text ends at least one space before every value placed
/// further right on that row or below it, so pipes from the rows above
/// stay clear.
fn place(values: Vec<(usize, String)>) -> Vec<Placement> {
    let mut placed: Vec<Placement> = Vec::new();
    for (column, text) in values.into_iter().rev() {
        let width = text.chars().count();
        let mut row = 1;
        while !placed
            .iter()
            .filter(|other| other.row >= row)
            .all(|other| column + width < other.column)
        {
            row += 1;
        }
        placed.push(Placement { column, row, text });
    }
    placed
}

/// Row 0 holds a pipe for every value; each later row holds the values placed
/// on it and pipes for the values placed below it.
fn diagram_rows(placed: &[Placement]) -> Vec<String> {
    let Some(rows) = placed.iter().map(|value| value.row).max() else {
        return Vec::new();
    };
    let mut grid: Vec<Vec<char>> = vec![Vec::new(); rows + 1];
    for value in placed {
        for row in &mut grid[..value.row] {
            write_at(row, value.column, PIPE.to_string().as_str());
        }
        write_at(&mut grid[value.row], value.column, &value.text);
    }
    grid.into_iter().map(String::from_iter).collect()
}

fn write_at(row: &mut Vec<char>, column: usize, text: &str) {
    let end = column + text.chars().count();
    if row.len() < end {
        row.resize(end, ' ');
    }
    for (offset, character) in text.chars().enumerate() {
        row[column + offset] = character;
    }
}

fn legend_entry(number: usize, entry: &Entry<'_>) -> String {
    let mut block = format!("{INDENT}#{number} {} =", entry.label);
    if let Some(lines) = debug_string_lines(&entry.text) {
        for line in lines {
            block.push('\n');
            block.push_str(INDENT);
            block.push_str(INDENT);
            block.push_str(&line);
        }
    } else {
        let continuation = format!("\n{INDENT}{INDENT}");
        block.push(' ');
        block.push_str(&entry.text.replace('\n', &continuation));
    }
    block
}

/// The lines of a `Debug`-rendered string that contains a newline, unescaped.
///
/// `None` for any other rendering, including strings on one line, so those
/// keep their `Debug` form.
fn debug_string_lines(text: &str) -> Option<Vec<String>> {
    let body = text.strip_prefix('"')?;
    let (body, truncated) = match body.strip_suffix(TRUNCATION_MARKER) {
        Some(body) => (body, true),
        None => (body.strip_suffix('"')?, false),
    };
    let unescaped = unescape(body)?;
    if !unescaped.contains('\n') {
        return None;
    }
    let mut lines: Vec<String> = unescaped.split('\n').map(str::to_owned).collect();
    if lines.last().is_some_and(String::is_empty) {
        lines.pop();
    }
    if truncated {
        lines.push(TRUNCATION_MARKER.to_owned());
    }
    Some(lines)
}

/// Turns `\n`, `\\`, `\"` and `\'` back into the characters they stand for.
/// `\r`, `\t`, `\0` and `\u{..}` stay escaped, so a carriage return, a tab
/// or a terminal control sequence remains visible. An escape `Debug` does
/// not produce yields `None`; one cut short by truncation is dropped.
fn unescape(body: &str) -> Option<String> {
    let mut output = String::with_capacity(body.len());
    let mut characters = body.chars();
    while let Some(character) = characters.next() {
        if character != '\\' {
            output.push(character);
            continue;
        }
        let Some(escaped) = characters.next() else {
            break;
        };
        match escaped {
            'n' => output.push('\n'),
            '\\' => output.push('\\'),
            '"' => output.push('"'),
            '\'' => output.push('\''),
            'r' | 't' | '0' => {
                output.push('\\');
                output.push(escaped);
            }
            'u' => {
                let rest = characters.as_str();
                let Some(close) = rest.strip_prefix('{').and_then(|digits| digits.find('}')) else {
                    break;
                };
                output.push_str("\\u");
                output.push_str(&rest[..close + 2]);
                characters = rest[close + 2..].chars();
            }
            _ => return None,
        }
    }
    Some(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recorder::Slot;

    fn slot(column: usize, start: usize, end: usize) -> Slot {
        Slot { column, start, end }
    }

    #[allow(clippy::unnecessary_wraps)]
    fn some(text: &str) -> Option<String> {
        Some(text.to_owned())
    }

    fn lines(lines: &[&str]) -> String {
        lines.join("\n")
    }

    #[test]
    fn values_sit_under_their_tokens_and_fall_to_lower_rows_when_they_collide() {
        let source = "price * quantity + shipping == expected";
        let slots = [
            slot(0, 0, 5),
            slot(6, 0, 16),
            slot(8, 8, 16),
            slot(17, 0, 27),
            slot(19, 19, 27),
            slot(28, 0, 39),
            slot(31, 31, 39),
        ];
        let layout = Layout {
            source,
            slots: &slots,
        };
        let values = [
            some("12"),
            some("36"),
            some("3"),
            some("41"),
            some("5"),
            some("false"),
            some("40"),
        ];
        let expected = lines(&[
            "    price * quantity + shipping == expected",
            "    |     | |        | |        |  |",
            "    12    | 3        | 5        |  40",
            "          36         41         false",
        ]);
        assert_eq!(render(&layout, &values), expected);
    }

    #[test]
    fn value_without_rendering_has_no_pipe() {
        let slots = [slot(0, 0, 1), slot(2, 0, 6)];
        let layout = Layout {
            source: "a && b",
            slots: &slots,
        };
        let values = [None, some("false")];
        let expected = lines(&["    a && b", "      |", "      false"]);
        assert_eq!(render(&layout, &values), expected);
    }

    #[test]
    fn adjacent_columns_stack_downwards() {
        let slots = [slot(0, 0, 5), slot(1, 1, 5)];
        let layout = Layout {
            source: "!flag",
            slots: &slots,
        };
        let values = [some("false"), some("true")];
        let expected = lines(&["    !flag", "    ||", "    |true", "    false"]);
        assert_eq!(render(&layout, &values), expected);
    }

    #[test]
    fn without_values_only_the_source_line_is_rendered() {
        let layout = Layout {
            source: "false",
            slots: &[],
        };
        assert_eq!(render(&layout, &[]), "    false");
    }

    #[test]
    fn wide_values_move_to_a_numbered_legend_in_column_order() {
        let slots = [slot(0, 0, 4), slot(8, 8, 13)];
        let layout = Layout {
            source: "left == right",
            slots: &slots,
        };
        let wide = "x".repeat(MAX_INLINE_WIDTH + 1);
        let values = [some(&wide), some(&wide)];
        let expected = lines(&[
            "    left == right",
            "    |       |",
            "    #1      #2",
            "",
            &format!("    #1 left = {wide}"),
            "",
            &format!("    #2 right = {wide}"),
        ]);
        assert_eq!(render(&layout, &values), expected);
    }

    #[test]
    fn multi_line_string_is_listed_as_text_lines() {
        let slots = [slot(0, 0, 6)];
        let layout = Layout {
            source: "output",
            slots: &slots,
        };
        let values = [some(r#""listed=3\nwarnings=0\nsaid \"hi\"\n""#)];
        let expected = lines(&[
            "    output",
            "    |",
            "    #1",
            "",
            "    #1 output =",
            "        listed=3",
            "        warnings=0",
            "        said \"hi\"",
        ]);
        assert_eq!(render(&layout, &values), expected);
    }

    #[test]
    fn truncated_multi_line_string_ends_with_the_marker() {
        let lines = debug_string_lines("\"a\\nb…").expect("a cut string still lists its lines");
        assert_eq!(lines, ["a", "b", TRUNCATION_MARKER]);
    }

    #[test]
    fn single_line_string_keeps_its_debug_form() {
        assert_eq!(debug_string_lines("\"a b\""), None);
    }

    #[test]
    fn unknown_escape_keeps_the_debug_form() {
        assert_eq!(debug_string_lines("\"a\\nb\\q\""), None);
    }

    #[test]
    fn carriage_returns_tabs_and_control_characters_stay_visible() {
        assert_eq!(
            unescape("a\\r\\n\\tb\\u{1b}[31m\\0"),
            Some("a\\r\n\\tb\\u{1b}[31m\\0".to_owned())
        );
    }

    #[test]
    fn quotes_and_backslashes_are_unescaped() {
        assert_eq!(unescape("\\\"a\\\\b\\'"), Some("\"a\\b'".to_owned()));
    }

    #[test]
    fn raw_newlines_in_a_wide_value_are_indented_in_the_legend() {
        let slots = [slot(0, 0, 1)];
        let layout = Layout {
            source: "v",
            slots: &slots,
        };
        let values = [some("Custom {\n  a: 1\n}")];
        let expected = lines(&[
            "    v",
            "    |",
            "    #1",
            "",
            "    #1 v = Custom {",
            "          a: 1",
            "        }",
        ]);
        assert_eq!(render(&layout, &values), expected);
    }
}
