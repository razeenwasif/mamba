//! Lightweight formula rendering: turns plain-ASCII math markers in a problem
//! prompt into clean terminal typography, so prompts can be *authored in ASCII*
//! instead of hand-embedding Unicode.
//!
//! Three transforms, applied line by line:
//! * `^…` → superscripts — `x^2` → `x²`, `x^(n-1)` → `xⁿ⁻¹`, `7^128` → `7¹²⁸`,
//!   `A^T` → `Aᵀ`.
//! * `_…` → subscripts — `a_i` → `aᵢ`, `x_1` → `x₁`.
//! * a line that is wholly a matrix literal `[[a, b], [c, d]]` → a multi-line
//!   Unicode-bracketed layout:
//!   ```text
//!       [[4, 7], [2, 6]]   →   ⎡ 4  7 ⎤
//!                              ⎣ 2  6 ⎦
//!   ```
//!
//! Each script run is **all-or-nothing**: if any character has no Unicode
//! super/subscript form (e.g. `q`, most uppercase as subscripts), the run is
//! left as the literal `^…`/`_…` so output never looks half-converted. Existing
//! Unicode in a prompt passes through untouched, so the renderer is safe to run
//! over already-typeset prompts.

use ratatui::text::Line;

/// Renders a prompt into display lines, expanding matrix literals and applying
/// super/subscript conversion to the rest.
pub fn render(prompt: &str) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    for raw in prompt.lines() {
        match try_matrix(raw) {
            Some(rows) => lines.extend(rows.into_iter().map(Line::from)),
            None => lines.push(Line::from(convert_scripts(raw))),
        }
    }
    lines
}

/// Converts every `^token` / `_token` run in a line to super/subscripts, where
/// `token` is either a parenthesized group `(…)` or an optional sign followed by
/// an alphanumeric run.
fn convert_scripts(line: &str) -> String {
    let chars: Vec<char> = line.chars().collect();
    let mut out = String::with_capacity(line.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '^' || c == '_' {
            let map = if c == '^' { superscript } else { subscript };
            if let Some((mapped, next)) = take_script(&chars, i + 1, map) {
                out.push_str(&mapped);
                i = next;
                continue;
            }
        }
        out.push(c);
        i += 1;
    }
    out
}

/// Reads the token after a `^`/`_` at `start` and maps it. Returns the mapped
/// string and the index just past the token, or `None` to fall back to literal.
fn take_script(
    chars: &[char],
    start: usize,
    map: fn(char) -> Option<char>,
) -> Option<(String, usize)> {
    if chars.get(start) == Some(&'(') {
        // Parenthesized group: map the inside, drop the parens.
        let close = (start + 1..chars.len()).find(|&k| chars[k] == ')')?;
        let inner: String = chars[start + 1..close].iter().collect();
        return Some((map_all(&inner, map)?, close + 1));
    }
    // Optional leading sign, then an alphanumeric run.
    let mut j = start;
    if matches!(chars.get(j), Some('+' | '-')) {
        j += 1;
    }
    while j < chars.len() && chars[j].is_alphanumeric() {
        j += 1;
    }
    if j == start {
        return None; // nothing to superscript
    }
    let token: String = chars[start..j].iter().collect();
    Some((map_all(&token, map)?, j))
}

/// Maps every char of `s` through `map`, or `None` if any char has no form.
fn map_all(s: &str, map: fn(char) -> Option<char>) -> Option<String> {
    s.chars().map(map).collect()
}

/// Expands a line that is wholly a `[[…], […]]` matrix literal into bracketed
/// rows, preserving the line's indentation. Returns `None` for any other line.
fn try_matrix(line: &str) -> Option<Vec<String>> {
    let indent: String = line.chars().take_while(|c| c.is_whitespace()).collect();
    let t = line.trim();
    if !(t.starts_with("[[") && t.ends_with("]]")) {
        return None;
    }
    let inner = &t[1..t.len() - 1]; // strip one outer pair → "[a, b], [c, d]"

    let mut rows: Vec<Vec<String>> = Vec::new();
    for part in inner.split('[').skip(1) {
        let row_str = part.split(']').next()?;
        let cells: Vec<String> = row_str
            .split(',')
            .map(|c| convert_scripts(c.trim()))
            .collect();
        rows.push(cells);
    }
    if rows.is_empty() || rows.iter().any(|r| r.is_empty()) {
        return None;
    }

    // Per-column widths (in chars) for right-aligned cells.
    let ncols = rows.iter().map(Vec::len).max().unwrap_or(0);
    let mut widths = vec![0usize; ncols];
    for row in &rows {
        for (ci, cell) in row.iter().enumerate() {
            widths[ci] = widths[ci].max(cell.chars().count());
        }
    }

    let n = rows.len();
    let out = rows
        .iter()
        .enumerate()
        .map(|(ri, row)| {
            let (lb, rb) = brackets(ri, n);
            let body = row
                .iter()
                .enumerate()
                .map(|(ci, cell)| {
                    let pad = widths[ci].saturating_sub(cell.chars().count());
                    format!("{}{cell}", " ".repeat(pad))
                })
                .collect::<Vec<_>>()
                .join("  ");
            format!("{indent}{lb} {body} {rb}")
        })
        .collect();
    Some(out)
}

/// The left/right bracket glyphs for row `ri` of an `n`-row matrix.
fn brackets(ri: usize, n: usize) -> (char, char) {
    if n == 1 {
        ('[', ']')
    } else if ri == 0 {
        ('⎡', '⎤')
    } else if ri == n - 1 {
        ('⎣', '⎦')
    } else {
        ('⎢', '⎥')
    }
}

/// Superscript form of a character, if one exists in Unicode.
fn superscript(c: char) -> Option<char> {
    Some(match c {
        '0' => '⁰',
        '1' => '¹',
        '2' => '²',
        '3' => '³',
        '4' => '⁴',
        '5' => '⁵',
        '6' => '⁶',
        '7' => '⁷',
        '8' => '⁸',
        '9' => '⁹',
        '+' => '⁺',
        '-' => '⁻',
        '=' => '⁼',
        '(' => '⁽',
        ')' => '⁾',
        'a' => 'ᵃ',
        'b' => 'ᵇ',
        'c' => 'ᶜ',
        'd' => 'ᵈ',
        'e' => 'ᵉ',
        'f' => 'ᶠ',
        'g' => 'ᵍ',
        'h' => 'ʰ',
        'i' => 'ⁱ',
        'j' => 'ʲ',
        'k' => 'ᵏ',
        'l' => 'ˡ',
        'm' => 'ᵐ',
        'n' => 'ⁿ',
        'o' => 'ᵒ',
        'p' => 'ᵖ',
        'r' => 'ʳ',
        's' => 'ˢ',
        't' => 'ᵗ',
        'u' => 'ᵘ',
        'v' => 'ᵛ',
        'w' => 'ʷ',
        'x' => 'ˣ',
        'y' => 'ʸ',
        'z' => 'ᶻ',
        'A' => 'ᴬ',
        'B' => 'ᴮ',
        'D' => 'ᴰ',
        'E' => 'ᴱ',
        'G' => 'ᴳ',
        'H' => 'ᴴ',
        'I' => 'ᴵ',
        'J' => 'ᴶ',
        'K' => 'ᴷ',
        'L' => 'ᴸ',
        'M' => 'ᴹ',
        'N' => 'ᴺ',
        'O' => 'ᴼ',
        'P' => 'ᴾ',
        'R' => 'ᴿ',
        'T' => 'ᵀ',
        'U' => 'ᵁ',
        'V' => 'ⱽ',
        'W' => 'ᵂ',
        // q, and C/F/Q/S/X/Y/Z, have no standard superscript form.
        _ => return None,
    })
}

/// Subscript form of a character, if one exists in Unicode.
fn subscript(c: char) -> Option<char> {
    Some(match c {
        '0' => '₀',
        '1' => '₁',
        '2' => '₂',
        '3' => '₃',
        '4' => '₄',
        '5' => '₅',
        '6' => '₆',
        '7' => '₇',
        '8' => '₈',
        '9' => '₉',
        '+' => '₊',
        '-' => '₋',
        '=' => '₌',
        '(' => '₍',
        ')' => '₎',
        'a' => 'ₐ',
        'e' => 'ₑ',
        'h' => 'ₕ',
        'i' => 'ᵢ',
        'j' => 'ⱼ',
        'k' => 'ₖ',
        'l' => 'ₗ',
        'm' => 'ₘ',
        'n' => 'ₙ',
        'o' => 'ₒ',
        'p' => 'ₚ',
        'r' => 'ᵣ',
        's' => 'ₛ',
        't' => 'ₜ',
        'u' => 'ᵤ',
        'v' => 'ᵥ',
        'x' => 'ₓ',
        // Most consonants have no subscript form.
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rendered(prompt: &str) -> Vec<String> {
        render(prompt)
            .into_iter()
            .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect())
            .collect()
    }

    #[test]
    fn superscripts_digits_letters_and_groups() {
        assert_eq!(convert_scripts("x^2"), "x²");
        assert_eq!(convert_scripts("3x^4 + 1"), "3x⁴ + 1");
        assert_eq!(convert_scripts("7^128 mod 13"), "7¹²⁸ mod 13");
        assert_eq!(convert_scripts("x^(n-1)"), "xⁿ⁻¹");
        assert_eq!(convert_scripts("x^-1"), "x⁻¹");
        assert_eq!(convert_scripts("A^T"), "Aᵀ");
    }

    #[test]
    fn subscripts_convert() {
        assert_eq!(convert_scripts("a_i + a_1"), "aᵢ + a₁");
        assert_eq!(convert_scripts("x_10"), "x₁₀");
    }

    #[test]
    fn unmappable_runs_fall_back_to_literal() {
        // `q` has no superscript form → leave the whole run literal.
        assert_eq!(convert_scripts("x^q"), "x^q");
        // An identifier with an underscore is not a subscript request gone wrong.
        assert_eq!(convert_scripts("some_word"), "some_word");
        // A bare caret stays.
        assert_eq!(convert_scripts("2 ^ 3"), "2 ^ 3");
    }

    #[test]
    fn matrix_literal_expands_with_brackets() {
        let out = rendered("    [[4, 7], [2, 6]]");
        assert_eq!(out, vec!["    ⎡ 4  7 ⎤", "    ⎣ 2  6 ⎦"]);
    }

    #[test]
    fn single_row_matrix_uses_plain_brackets() {
        let out = rendered("[[1, 2, 3]]");
        assert_eq!(out, vec!["[ 1  2  3 ]"]);
    }

    #[test]
    fn inline_brackets_in_prose_are_left_alone() {
        let out = rendered("For [[a, b], [c, d]], det = ad - bc.");
        assert_eq!(out, vec!["For [[a, b], [c, d]], det = ad - bc."]);
    }

    #[test]
    fn matrix_cells_are_right_aligned_and_may_be_fractions() {
        let out = rendered("[[3/5, -7/10], [-1/5, 2/5]]");
        assert_eq!(out, vec!["⎡  3/5  -7/10 ⎤", "⎣ -1/5    2/5 ⎦"]);
    }
}
