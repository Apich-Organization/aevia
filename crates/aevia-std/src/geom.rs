//! Elementary geometry (§8.16).
//!
//! `polygon_area` is the shoelace formula and `distance` is Euclidean; both are
//! exact in `f64` for representable input, and both carry the dimension of
//! their arguments (`L` and `L^2`), which the caller attaches.

/// The area of a polygon given its vertices, by the shoelace formula.
///
/// `points` is `[x0, y0, x1, y1, …]`; the result is non-negative and does not
/// depend on the winding direction.
#[must_use]
pub fn polygon_area(points: &[f64]) -> f64 {
    let count = points.len() / 2;
    if count < 3 {
        return 0.0;
    }
    let mut total = 0.0;
    for index in 0..count {
        let next = (index + 1) % count;
        total += points[index * 2] * points[next * 2 + 1]
            - points[next * 2] * points[index * 2 + 1];
    }
    (total / 2.0).abs()
}

/// The Euclidean distance between two points of the same dimension.
///
/// # Errors
///
/// [`crate::NumericError::ShapeMismatch`] when the dimensions differ.
pub fn distance(left: &[f64], right: &[f64]) -> Result<f64, crate::NumericError> {
    if left.len() != right.len() {
        return Err(crate::NumericError::ShapeMismatch(format!(
            "distance needs two points of one dimension, found {} and {}",
            left.len(),
            right.len()
        )));
    }
    Ok(left
        .iter()
        .zip(right)
        .map(|(a, b)| (a - b) * (a - b))
        .sum::<f64>()
        .sqrt())
}

/// The signed volume of a tetrahedron, from four vertices in `R^3`.
#[must_use]
pub fn tetrahedron_volume(points: &[f64]) -> f64 {
    if points.len() < 12 {
        return 0.0;
    }
    let a = [points[3] - points[0], points[4] - points[1], points[5] - points[2]];
    let b = [points[6] - points[0], points[7] - points[1], points[8] - points[2]];
    let c = [points[9] - points[0], points[10] - points[1], points[11] - points[2]];
    let cross = [
        b[1] * c[2] - b[2] * c[1],
        b[2] * c[0] - b[0] * c[2],
        b[0] * c[1] - b[1] * c[0],
    ];
    (a[0] * cross[0] + a[1] * cross[1] + a[2] * cross[2]).abs() / 6.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_area_of_a_unit_square() {
        assert!((polygon_area(&[0.0, 0.0, 1.0, 0.0, 1.0, 1.0, 0.0, 1.0]) - 1.0).abs() < 1e-12);
        // The same square wound the other way.
        assert!((polygon_area(&[0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 0.0]) - 1.0).abs() < 1e-12);
    }

    #[test]
    fn the_area_of_a_triangle() {
        assert!((polygon_area(&[0.0, 0.0, 4.0, 0.0, 0.0, 3.0]) - 6.0).abs() < 1e-12);
        assert!((polygon_area(&[0.0, 0.0]) - 0.0).abs() < 1e-12);
    }

    #[test]
    fn the_distance_of_a_pythagorean_triple() {
        assert!((distance(&[0.0, 0.0], &[3.0, 4.0]).unwrap() - 5.0).abs() < 1e-12);
        assert!(distance(&[0.0], &[1.0, 2.0]).is_err());
    }

    #[test]
    fn the_volume_of_a_unit_tetrahedron() {
        let points = [
            0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0,
        ];
        assert!((tetrahedron_volume(&points) - 1.0 / 6.0).abs() < 1e-12);
    }
}
