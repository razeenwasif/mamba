//! # Tensor pipeline (linear algebra)
//!
//! Verifies matrix and scalar answers by parsing both sides into an
//! [`ndarray::Array2`] of [`Cell`]s and comparing **shape first, then every
//! element**. A scalar answer (e.g. a determinant) is simply a 1×1 matrix, so
//! determinants, transposes, products, and inverses all flow through one path.
//!
//! ## Hybrid exact / approximate comparison
//!
//! Each cell is parsed as either:
//! * an **exact rational** ([`num_rational::Ratio<i64>`]) when written as an
//!   integer (`10`) or a fraction (`-7/10`), or
//! * an **approximate float** when written as a decimal (`0.5`, `1.41421356`).
//!
//! Two cells are equal when **both are exact** (compared with `==`, so `2/6`
//! equals `1/3` and a matrix inverse verifies with no rounding error) or, if
//! either side is approximate, when they agree within a float tolerance (so an
//! author-stored irrational like `1.41421356` matches a close decimal). This
//! gives exactness where it's available and tolerance where it's needed.
//!
//! ## Input syntax
//!
//! Rows are separated by `;` (or newlines); within a row, elements are
//! separated by commas or whitespace:
//!
//! ```text
//!   1 2 3; 4 5 6           →  ⎡1 2 3⎤
//!                             ⎣4 5 6⎦
//!   3/5 -7/10; -1/5 2/5    →  a 2×2 matrix of exact fractions
//!   10                     →  [10]   (a 1×1 scalar)
//! ```

use std::fmt;

use ndarray::Array2;
use num_bigint::BigInt;
use num_rational::BigRational;
use num_traits::{ToPrimitive, Zero};

use crate::engine::Verdict;

/// Float tolerance used only when at least one cell is approximate, so e.g.
/// `0.3333333` matches `0.3333334`.
const EPSILON: f64 = 1e-6;

/// One matrix element: an exact fraction, or an approximate decimal.
#[derive(Debug, Clone, PartialEq)]
enum Cell {
    /// An integer or `a/b` fraction — compared exactly. Arbitrary-precision
    /// ([`BigRational`]), so no numerator/denominator can ever overflow.
    Exact(BigRational),
    /// A decimal literal — compared within [`EPSILON`].
    Approx(f64),
}

impl Cell {
    /// The cell's value as an `f64` (lossy for `Exact`), used for the
    /// approximate comparison path.
    fn to_f64(&self) -> f64 {
        match self {
            Cell::Exact(r) => r.to_f64().unwrap_or(f64::NAN),
            Cell::Approx(v) => *v,
        }
    }

    /// Whether two cells represent the same value: exact equality when both are
    /// fractions, float tolerance when either is a decimal.
    fn matches(&self, other: &Cell) -> bool {
        match (self, other) {
            (Cell::Exact(a), Cell::Exact(b)) => a == b,
            _ => {
                let (x, y) = (self.to_f64(), other.to_f64());
                (x - y).abs() <= EPSILON * (1.0 + x.abs())
            }
        }
    }
}

impl fmt::Display for Cell {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Cell::Exact(r) if r.is_integer() => write!(f, "{}", r.numer()),
            Cell::Exact(r) => write!(f, "{}/{}", r.numer(), r.denom()),
            Cell::Approx(v) => write!(f, "{v}"),
        }
    }
}

/// Verifies `input` against `expected` by element-wise matrix comparison.
pub fn verify(expected: &str, input: &str) -> Verdict {
    // Expected is authored by us; a parse failure is a bank bug.
    let want = match parse_matrix(expected) {
        Ok(m) => m,
        Err(e) => {
            return Verdict::parse_error(format!("internal: expected matrix invalid ({e})"), None);
        }
    };

    let got = match parse_matrix(input) {
        Ok(m) => m,
        Err(e) => {
            return Verdict::parse_error(
                format!("couldn't parse matrix: {e}"),
                Some(
                    "rows separated by ';', elements by spaces or commas; \
                     fractions allowed, e.g. 1 2; 3/5 -7/10"
                        .into(),
                ),
            );
        }
    };

    if got.dim() != want.dim() {
        let (gr, gc) = got.dim();
        let (wr, wc) = want.dim();
        return Verdict::incorrect(format!("shapes differ: got {gr}×{gc}, expected {wr}×{wc}"));
    }

    // Shapes match → compare element-wise and report the first disagreement.
    for ((r, c), w) in want.indexed_iter() {
        let g = &got[[r, c]];
        if !w.matches(g) {
            return Verdict::incorrect(format!(
                "differs at row {}, col {}: got {g}, expected {w}",
                r + 1,
                c + 1
            ));
        }
    }

    Verdict::Correct
}

/// Parses a `;`/newline-by-row, whitespace-or-comma-by-element matrix into an
/// [`Array2<Cell>`]. Errors on ragged rows or non-numeric cells.
fn parse_matrix(src: &str) -> Result<Array2<Cell>, String> {
    let rows: Vec<Vec<Cell>> = src
        .split(['\n', ';'])
        .map(str::trim)
        .filter(|r| !r.is_empty())
        .map(parse_row)
        .collect::<Result<_, _>>()?;

    if rows.is_empty() {
        return Err("no numbers found".into());
    }

    let cols = rows[0].len();
    if let Some(bad) = rows.iter().position(|r| r.len() != cols) {
        return Err(format!(
            "row {} has {} element(s) but row 1 has {cols} — rows must be equal length",
            bad + 1,
            rows[bad].len()
        ));
    }

    let flat: Vec<Cell> = rows.into_iter().flatten().collect();
    Array2::from_shape_vec((flat.len() / cols, cols), flat)
        .map_err(|e| format!("could not assemble matrix: {e}"))
}

/// Parses one row: split on commas/whitespace, parse each token as a [`Cell`].
fn parse_row(row: &str) -> Result<Vec<Cell>, String> {
    row.split([',', ' ', '\t'])
        .filter(|t| !t.is_empty())
        .map(parse_cell)
        .collect()
}

/// Parses one element: an `a/b` fraction or bare integer → [`Cell::Exact`]; a
/// decimal or scientific literal → [`Cell::Approx`].
fn parse_cell(tok: &str) -> Result<Cell, String> {
    let tok = tok.trim();
    if let Some((num, den)) = tok.split_once('/') {
        let n: BigInt = num
            .trim()
            .parse()
            .map_err(|_| format!("'{num}' is not an integer numerator in '{tok}'"))?;
        let d: BigInt = den
            .trim()
            .parse()
            .map_err(|_| format!("'{den}' is not an integer denominator in '{tok}'"))?;
        if d.is_zero() {
            return Err(format!("division by zero in '{tok}'"));
        }
        return Ok(Cell::Exact(BigRational::new(n, d)));
    }
    // A decimal point or exponent signals an approximate value.
    if tok.contains('.') || tok.contains(['e', 'E']) {
        return tok
            .parse::<f64>()
            .map(Cell::Approx)
            .map_err(|_| format!("'{tok}' is not a number"));
    }
    tok.parse::<BigInt>()
        .map(|n| Cell::Exact(BigRational::from_integer(n)))
        .map_err(|_| format!("'{tok}' is not a number"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scalar_determinant_is_a_one_by_one_matrix() {
        assert!(verify("10", "10").is_correct());
        assert!(verify("10", "10.0").is_correct());
        assert!(matches!(verify("10", "11"), Verdict::Incorrect { .. }));
    }

    #[test]
    fn matrix_compare_is_whitespace_and_separator_agnostic() {
        assert!(verify("1 4; 2 5; 3 6", "1,4 ; 2,5 ; 3,6").is_correct());
        assert!(verify("1 4; 2 5; 3 6", "1 4\n2 5\n3 6").is_correct());
    }

    #[test]
    fn fractions_compare_exactly_and_reduce() {
        assert!(verify("1/3", "1/3").is_correct());
        assert!(verify("1/3", "2/6").is_correct()); // Ratio auto-reduces
        assert!(verify("-7/10", "-14/20").is_correct());
        // A decimal vs an exact fraction falls back to tolerance.
        assert!(verify("1/2", "0.5").is_correct());
        // A coarse decimal approximation of an exact fraction is rejected.
        assert!(matches!(verify("1/3", "0.3333"), Verdict::Incorrect { .. }));
    }

    #[test]
    fn matrix_of_fractions_verifies_exactly() {
        // The inverse of [[4,7],[2,6]] (det 10): (1/10)[[6,-7],[-2,4]].
        let inverse = "3/5 -7/10; -1/5 2/5";
        assert!(verify(inverse, "3/5 -7/10; -1/5 2/5").is_correct());
        assert!(verify(inverse, "6/10 -7/10; -2/10 4/10").is_correct());
        assert!(matches!(
            verify(inverse, "3/5 -7/10; -1/5 1/5"),
            Verdict::Incorrect { .. }
        ));
    }

    #[test]
    fn huge_fractions_never_overflow() {
        // Numerator beyond i64::MAX (~9.2e18); the old Ratio<i64> would panic.
        let big = "12345678901234567890/2";
        assert!(verify(big, "6172839450617283945").is_correct());
        assert!(verify("1/98765432109876543210", "1/98765432109876543210").is_correct());
        assert!(matches!(
            verify("1/98765432109876543210", "1/98765432109876543211"),
            Verdict::Incorrect { .. }
        ));
    }

    #[test]
    fn approximate_cells_use_tolerance() {
        assert!(verify("1.41421356", "1.41421357").is_correct());
        assert!(matches!(
            verify("1.41421356", "1.5"),
            Verdict::Incorrect { .. }
        ));
    }

    #[test]
    fn shape_mismatch_is_reported_clearly() {
        match verify("1 2; 3 4", "1 2 3 4") {
            Verdict::Incorrect { detail } => assert!(detail.contains("shapes differ")),
            other => panic!("expected shape mismatch, got {other:?}"),
        }
    }

    #[test]
    fn ragged_input_is_a_parse_error() {
        assert!(matches!(
            verify("1 2; 3 4", "1 2; 3"),
            Verdict::ParseError { .. }
        ));
    }

    #[test]
    fn division_by_zero_in_a_fraction_is_a_parse_error() {
        assert!(matches!(verify("1/2", "1/0"), Verdict::ParseError { .. }));
    }
}
