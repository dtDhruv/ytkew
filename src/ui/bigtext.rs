//! Three-row box-drawing block letters, as btop uses for its menu.
//!
//! The font is defined once in single-line glyphs; the selected variant is
//! derived by substituting the double-line equivalents, which is exactly the
//! relationship between btop's `menu_normal` and `menu_selected` tables and
//! saves maintaining two copies that could drift apart.

fn glyph(c: char) -> [&'static str; 3] {
    match c.to_ascii_uppercase() {
        'A' => ["┌─┐", "├─┤", "┴ ┴"],
        'B' => ["┌┐ ", "├┴┐", "└─┘"],
        'C' => ["┌─┐", "│  ", "└─┘"],
        'D' => ["┌┬┐", " ││", "─┴┘"],
        'E' => ["┌─┐", "├┤ ", "└─┘"],
        'F' => ["┌─┐", "├┤ ", "┴  "],
        'G' => ["┌─┐", "│ ┬", "└─┘"],
        'H' => ["┬ ┬", "├─┤", "┴ ┴"],
        'I' => ["┬", "│", "┴"],
        'L' => ["┬  ", "│  ", "┴─┘"],
        'N' => ["┌┐┌", "│││", "┘└┘"],
        'O' => ["┌─┐", "│ │", "└─┘"],
        'P' => ["┌─┐", "├─┘", "┴  "],
        'Q' => ["┌─┐", "│ │", "└─┴"],
        'S' => ["┌─┐", "└─┐", "└─┘"],
        'T' => ["┌┬┐", " │ ", " ┴ "],
        'U' => ["┬ ┬", "│ │", "└─┘"],
        ' ' => [" ", " ", " "],
        _ => ["", "", ""],
    }
}

/// Turn single-line box drawing into its double-line counterpart.
fn thicken(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '─' => '═',
            '│' => '║',
            '┌' => '╔',
            '┐' => '╗',
            '└' => '╚',
            '┘' => '╝',
            '├' => '╠',
            '┤' => '╣',
            '┬' => '╦',
            '┴' => '╩',
            '┼' => '╬',
            other => other,
        })
        .collect()
}

/// Render `word` as three rows. `emphasised` picks the double-line variant.
pub fn render(word: &str, emphasised: bool) -> [String; 3] {
    let mut rows = [String::new(), String::new(), String::new()];
    for c in word.chars() {
        let g = glyph(c);
        for (i, row) in rows.iter_mut().enumerate() {
            row.push_str(g[i]);
        }
    }
    if emphasised {
        [thicken(&rows[0]), thicken(&rows[1]), thicken(&rows[2])]
    } else {
        rows
    }
}

/// Display width of a rendered word, in columns.
pub fn width(word: &str) -> usize {
    word.chars().map(|c| glyph(c)[0].chars().count()).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_word_renders_three_rows_of_equal_width() {
        let rows = render("OPTIONS", false);
        let w = rows[0].chars().count();
        assert!(w > 0);
        for r in &rows {
            assert_eq!(r.chars().count(), w, "rows must line up: {rows:?}");
        }
        assert_eq!(w, width("OPTIONS"));
    }

    #[test]
    fn emphasis_switches_to_double_line_glyphs() {
        let normal = render("HELP", false);
        let bold = render("HELP", true);
        assert!(normal[0].contains('┬') || normal[0].contains('┌'));
        assert!(bold[0].contains('╦') || bold[0].contains('╔'));
        for i in 0..3 {
            assert_eq!(normal[i].chars().count(), bold[i].chars().count());
        }
    }

    #[test]
    fn every_letter_the_menu_uses_has_a_glyph() {
        for word in ["OPTIONS", "HELP", "QUIT"] {
            for c in word.chars() {
                assert!(!glyph(c)[0].is_empty(), "no glyph for {c:?} in {word}");
            }
            let rows = render(word, false);
            let w = rows[0].chars().count();
            assert!(rows.iter().all(|r| r.chars().count() == w), "{word}");
        }
    }

    #[test]
    fn unknown_characters_do_not_break_alignment() {
        let rows = render("A!B", false);
        let w = rows[0].chars().count();
        assert!(rows.iter().all(|r| r.chars().count() == w));
    }
}
