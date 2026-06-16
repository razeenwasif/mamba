//! Dimensional analysis for physics & engineering answers.
//!
//! A problem may declare the physical `unit` its answer carries (e.g. `N`,
//! `m/s^2`, `kg*m/s`). When the user appends a unit to their number, the router
//! checks it is **dimensionally consistent** with the expected one before
//! comparing values — so `9.8 m/s^2` is accepted for an acceleration but
//! `9.8 kg` is rejected outright, even though the number matches.
//!
//! This is a *dimensional* check, not a unit *conversion*: it compares the
//! exponents of the base quantities (mass, length, time, current, temperature),
//! catching category errors (force vs mass). It deliberately does **not** police
//! SI prefixes — `ms` and `s` are both "time" — so it accepts any unit of the
//! right dimension and never blocks a correct value over a prefix. A user who
//! omits the unit is never penalised.

/// Exponents of the five base quantities we model: mass, length, time, current,
/// temperature. (`Hz` = T⁻¹, `N` = M·L·T⁻², and so on.)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Dim([i32; 5]);

impl Dim {
    const ZERO: Dim = Dim([0; 5]);

    /// Adds `k ×` another dimension, component-wise (for `*`, `/`, and `^`).
    fn add_scaled(self, other: Dim, k: i32) -> Dim {
        let mut d = self.0;
        for (di, oi) in d.iter_mut().zip(other.0) {
            *di += k * oi;
        }
        Dim(d)
    }
}

/// The base/derived units we recognize, as `(token, dimension)`. Compound units
/// (`m/s`, `N*m`) are built from these by [`parse_compound`]; SI prefixes are
/// handled by [`parse_unit`].
fn lookup(tok: &str) -> Option<Dim> {
    // [mass, length, time, current, temperature]
    Some(Dim(match tok {
        "kg" | "g" => [1, 0, 0, 0, 0],
        "m" => [0, 1, 0, 0, 0],
        "s" | "sec" => [0, 0, 1, 0, 0],
        "A" => [0, 0, 0, 1, 0],
        "K" => [0, 0, 0, 0, 1],
        "N" => [1, 1, -2, 0, 0],                  // force
        "J" => [1, 2, -2, 0, 0],                  // energy
        "W" => [1, 2, -3, 0, 0],                  // power
        "V" => [1, 2, -3, -1, 0],                 // voltage
        "ohm" | "Ohm" | "Ω" => [1, 2, -3, -2, 0], // resistance
        "Pa" => [1, -1, -2, 0, 0],                // pressure
        "Hz" => [0, 0, -1, 0, 0],                 // frequency
        "C" => [0, 0, 1, 1, 0],                   // charge
        "F" => [-1, -2, 4, 2, 0],                 // capacitance
        "H" => [1, 2, -2, -2, 0],                 // inductance
        "S" => [-1, -2, 3, 2, 0],                 // conductance (siemens)
        "T" => [1, 0, -2, -1, 0],                 // magnetic flux density (tesla)
        _ => return None,
    }))
}

/// Single-character SI prefixes we strip for the *dimensional* lookup (their
/// magnitude is irrelevant to dimension). `µ` and `u` are micro.
fn is_prefix(c: char) -> bool {
    matches!(c, 'G' | 'M' | 'k' | 'm' | 'u' | 'µ' | 'n' | 'p')
}

/// Parses one unit token to a dimension: a direct hit, else a known unit behind
/// a single SI prefix (`ms` → `s`, `MPa` → `Pa`, `kΩ` → `Ω`).
fn parse_unit(tok: &str) -> Option<Dim> {
    if let Some(d) = lookup(tok) {
        return Some(d);
    }
    let mut chars = tok.chars();
    let first = chars.next()?;
    let rest: String = chars.collect();
    if is_prefix(first) && !rest.is_empty() {
        return lookup(&rest);
    }
    None
}

/// Parses a compound unit string (`kg*m/s^2`, `N·m`, `m/s`) to a dimension.
/// `1` and the empty string are dimensionless. Returns `None` on any unknown
/// token, so an unrecognized unit is treated leniently by [`check`].
fn parse_compound(src: &str) -> Option<Dim> {
    let src = src.trim();
    if src.is_empty() || src == "1" {
        return Some(Dim::ZERO);
    }
    // First '/'-group is the numerator; every later one is a denominator.
    let mut dim = Dim::ZERO;
    for (group_idx, group) in src.split('/').enumerate() {
        let sign = if group_idx == 0 { 1 } else { -1 };
        for factor in group
            .split(['*', '·', '×', ' ', '\t'])
            .filter(|t| !t.is_empty())
        {
            let (base, exp) = match factor.split_once('^') {
                Some((b, e)) => (b.trim(), e.trim().parse::<i32>().ok()?),
                None => (factor.trim(), 1),
            };
            if base == "1" {
                continue; // a dimensionless factor, e.g. the numerator of `1/s`
            }
            dim = dim.add_scaled(parse_unit(base)?, sign * exp);
        }
    }
    Some(dim)
}

/// A friendly name for a few common dimensions, for the mismatch message.
fn dimension_name(d: Dim) -> Option<&'static str> {
    Some(match d.0 {
        [1, 1, -2, 0, 0] => "force",
        [1, 2, -2, 0, 0] => "energy",
        [1, 2, -3, 0, 0] => "power",
        [0, 1, 0, 0, 0] => "length",
        [1, 0, 0, 0, 0] => "mass",
        [0, 0, 1, 0, 0] => "time",
        [0, 1, -1, 0, 0] => "velocity",
        [0, 1, -2, 0, 0] => "acceleration",
        [1, 1, -1, 0, 0] => "momentum",
        [1, 2, -3, -1, 0] => "voltage",
        [1, 2, -3, -2, 0] => "resistance",
        [1, -1, -2, 0, 0] => "pressure",
        _ => return None,
    })
}

/// Splits a `"9.8 m/s^2"`-style answer into its numeric part and unit part. The
/// unit begins at the first alphabetic character that isn't the `e`/`E` of
/// scientific notation; if there is none, the unit is empty.
fn split_value_unit(input: &str) -> (&str, &str) {
    let chars: Vec<(usize, char)> = input.char_indices().collect();
    for (k, &(i, c)) in chars.iter().enumerate() {
        let sci_e = (c == 'e' || c == 'E')
            && k > 0
            && chars[k - 1].1.is_ascii_digit()
            && chars
                .get(k + 1)
                .is_some_and(|&(_, n)| n.is_ascii_digit() || n == '+' || n == '-');
        if c.is_alphabetic() && !sci_e {
            return (input[..i].trim(), input[i..].trim());
        }
    }
    (input.trim(), "")
}

/// The dimensional gate the router runs when a problem declares a unit.
///
/// Returns `Ok(value)` — the numeric part to verify (the unit stripped) — when
/// the user omitted a unit, used a dimensionally-correct one, or used one we
/// don't recognize (lenient). Returns `Err(detail)` only when the user's unit is
/// recognized *and* its dimension disagrees with the expected one.
pub fn check<'a>(expected_unit: &str, input: &'a str) -> Result<&'a str, String> {
    let (value, unit) = split_value_unit(input);
    if unit.is_empty() {
        return Ok(input.trim());
    }
    if let (Some(want), Some(got)) = (parse_compound(expected_unit), parse_compound(unit))
        && want != got
    {
        let name = |d: Dim, fallback: &str| match dimension_name(d) {
            Some(n) => format!("{fallback} ({n})"),
            None => fallback.to_string(),
        };
        return Err(format!(
            "wrong units: expected {}, but got {}",
            name(want, expected_unit),
            name(got, unit),
        ));
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_base_derived_and_compound_units() {
        assert_eq!(parse_compound("N"), parse_compound("kg*m/s^2"));
        assert_eq!(parse_compound("J"), parse_compound("N*m"));
        assert_eq!(parse_compound("W"), parse_compound("J/s"));
        assert_eq!(parse_compound("Hz"), parse_compound("1/s"));
        assert_ne!(parse_compound("N"), parse_compound("kg"));
    }

    #[test]
    fn strips_si_prefixes_for_dimension() {
        assert_eq!(parse_compound("ms"), parse_compound("s")); // milli
        assert_eq!(parse_compound("MPa"), parse_compound("Pa"));
        assert_eq!(parse_compound("kΩ"), parse_compound("ohm"));
        assert_eq!(parse_compound("µF"), parse_compound("F"));
    }

    #[test]
    fn splits_value_from_unit() {
        assert_eq!(split_value_unit("9.8 m/s^2"), ("9.8", "m/s^2"));
        assert_eq!(split_value_unit("30N"), ("30", "N"));
        assert_eq!(split_value_unit("-5 kg"), ("-5", "kg"));
        assert_eq!(split_value_unit("42"), ("42", ""));
        // Scientific notation is not mistaken for a unit.
        assert_eq!(split_value_unit("1e3"), ("1e3", ""));
        // A bare fraction has no unit.
        assert_eq!(split_value_unit("3/5"), ("3/5", ""));
    }

    #[test]
    fn check_strips_correct_units_and_rejects_wrong_ones() {
        // Right dimension → stripped to the value.
        assert_eq!(check("m/s^2", "9.8 m/s^2"), Ok("9.8"));
        assert_eq!(check("N", "30 N"), Ok("30"));
        // No unit typed → lenient, value passes through.
        assert_eq!(check("N", "30"), Ok("30"));
        // Unknown unit → lenient (don't punish what we can't parse).
        assert_eq!(check("N", "30 furlongs"), Ok("30"));
        // Wrong dimension → rejected with a named message.
        let err = check("m/s^2", "9.8 kg").unwrap_err();
        assert!(err.contains("wrong units"), "{err}");
        assert!(
            err.contains("acceleration") && err.contains("mass"),
            "{err}"
        );
    }
}
