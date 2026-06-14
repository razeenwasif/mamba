//! # Tensor pipeline (linear algebra)
//!
//! Verifies matrix and scalar answers by parsing both sides into an
//! [`ndarray::Array2<f64>`] and comparing **shape first, then every element**
//! within a tolerance. A scalar answer (e.g. a determinant) is simply a 1×1
//! matrix, so determinants, transposes, and products all flow through one
//! comparison path.
//!
//! ## Input syntax
//!
//! Rows are separated by `;` (or newlines); within a row, elements are
//! separated by commas or whitespace:
//!
//! ```text
//!   1 2 3; 4 5 6      →  ⎡1 2 3⎤
//!                        ⎣4 5 6⎦
//!   10                →  [10]   (a 1×1 scalar)
//! ```

use ndarray::Array2;

use crate::engine::Verdict;

/// Element-wise absolute+relative tolerance, so `0.3333333` matches `1/3`.
const EPSILON: f64 = 1e-6;

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
                Some("rows separated by ';', elements by spaces or commas, e.g. 1 2; 3 4".into()),
            );
        }
    };

    if got.dim() != want.dim() {
        let (gr, gc) = got.dim();
        let (wr, wc) = want.dim();
        return Verdict::incorrect(format!("shapes differ: got {gr}×{gc}, expected {wr}×{wc}"));
    }

    // Shapes match → compare element-wise and report the first disagreement.
    for ((r, c), &w) in want.indexed_iter() {
        let g = got[[r, c]];
        if (g - w).abs() > EPSILON * (1.0 + w.abs()) {
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
/// [`Array2<f64>`]. Errors on ragged rows or non-numeric cells.
fn parse_matrix(src: &str) -> Result<Array2<f64>, String> {
    let rows: Vec<Vec<f64>> = src
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

    let flat: Vec<f64> = rows.into_iter().flatten().collect();
    Array2::from_shape_vec((flat.len() / cols, cols), flat)
        .map_err(|e| format!("could not assemble matrix: {e}"))
}

/// Parses one row: split on commas/whitespace, parse each token as `f64`.
fn parse_row(row: &str) -> Result<Vec<f64>, String> {
    row.split([',', ' ', '\t'])
        .filter(|t| !t.is_empty())
        .map(|t| t.parse::<f64>().map_err(|_| format!("'{t}' is not a number")))
        .collect()
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
}
