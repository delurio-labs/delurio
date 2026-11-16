/// `String(n)`: ECMAScript's Number::toString, the shortest text that reads back as the same number.
pub fn number_to_string(n: f64) -> String {
    if n.is_nan() {
        return "NaN".to_string();
    }
    if n.is_infinite() {
        return if n > 0.0 { "Infinity" } else { "-Infinity" }.to_string();
    }
    if n == 0.0 {
        return "0".to_string();
    }
    let mut buf = ryu_js::Buffer::new();
    buf.format_finite(n).to_string()
}

/// `Math.round(x)`: the nearest integer, a half going up (towards +∞)
pub fn round(x: f64) -> f64 {
    if !x.is_finite() || x.fract() == 0.0 {
        return x;
    }
    let r = x.floor();
    let rounded = if x - r >= 0.5 { r + 1.0 } else { r };
    if rounded == 0.0 && x < 0.0 { -0.0 } else { rounded }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_as_javascript_writes_them() {
        assert_eq!(number_to_string(1.0), "1");
        assert_eq!(number_to_string(-0.0), "0");
        assert_eq!(number_to_string(0.1 + 0.2), "0.30000000000000004");
        assert_eq!(number_to_string(1e21), "1e+21");
        assert_eq!(number_to_string(123456789012345680000.0), "123456789012345680000");
        assert_eq!(number_to_string(1.5e-7), "1.5e-7");
        assert_eq!(number_to_string(0.000001), "0.000001");
        assert_eq!(number_to_string(f64::NAN), "NaN");
    }

    #[test]
    fn rounding_as_javascript_rounds() {
        assert_eq!(round(2.5), 3.0);
        assert_eq!(round(-2.5), -2.0);
        assert_eq!(round(0.49999999999999994), 0.0);
        assert_eq!(round(-0.4).to_string(), "-0");
        assert_eq!(round(12.34 * 10.0) / 10.0, 12.3);
    }
}
