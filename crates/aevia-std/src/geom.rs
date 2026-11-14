//! Elementary geometry (§8.16).
//!
//! `polygon_area` is the shoelace formula and `distance` is Euclidean; both are
//! evaluated by RSSN in floating-point, and both carry the dimension of
//! their arguments (`L` and `L^2`), which the caller attaches.

/// The non-negative area of a flat `[x0, y0, x1, y1, ...]` polygon.
#[must_use]
pub fn polygon_area(points: &[f64]) -> f64 {
    rssn::kernels::geometry::polygon_area(points)
}

/// Euclidean distance between points of equal dimension.
pub fn distance(
    left: &[f64],
    right: &[f64],
) -> Result<f64, crate::NumericError> {
    rssn::kernels::geometry::distance(left, right).map_err(map_geom_err)
}

/// Non-negative tetrahedron volume from four flat three-dimensional vertices.
#[must_use]
pub fn tetrahedron_volume(points: &[f64]) -> f64 {
    rssn::kernels::geometry::tetrahedron_volume(points)
}

pub use rssn::kernels::geometry::Aabb;
pub use rssn::kernels::geometry::Bvh;
pub use rssn::kernels::geometry::VoronoiDiagram;
pub use rssn::kernels::geometry::convex_hull_2d;
pub use rssn::kernels::geometry::convex_hull_3d;
pub use rssn::kernels::geometry::delaunay_2d;
pub use rssn::kernels::geometry::point_in_polygon;
pub use rssn::kernels::geometry::ray_segment_intersection_2d;
pub use rssn::kernels::geometry::ray_triangle_intersection_3d;
pub use rssn::kernels::geometry::segment_intersection_2d;
pub use rssn::kernels::geometry::segment_triangle_intersection_3d;
pub use rssn::kernels::geometry::voronoi_2d;

fn map_geom_err(err: rssn::kernels::geometry::GeometryError) -> crate::NumericError {
    match err {
        | rssn::kernels::geometry::GeometryError::Domain(msg) => crate::NumericError::Domain(msg),
        | rssn::kernels::geometry::GeometryError::Empty => crate::NumericError::Empty,
        | rssn::kernels::geometry::GeometryError::ShapeMismatch(msg) => {
            crate::NumericError::ShapeMismatch(msg)
        },
    }
}

/// A k-d tree for exact nearest-neighbor, k-nearest, and radius queries in `R^d`,
/// backed by `rssn::kernels::geometry::KdTree`.
#[derive(Clone, Debug)]
pub struct KdTree(rssn::kernels::geometry::KdTree);

impl KdTree {
    /// Builds a k-d tree from `points`.
    ///
    /// # Errors
    /// [`crate::NumericError::Empty`] or [`crate::NumericError::ShapeMismatch`].
    pub fn build(points: Vec<Vec<f64>>) -> Result<Self, crate::NumericError> {
        rssn::kernels::geometry::KdTree::build(points)
            .map(Self)
            .map_err(map_geom_err)
    }

    /// Returns `(point_index, distance)` for the nearest neighbor to `query`.
    ///
    /// # Errors
    /// [`crate::NumericError::ShapeMismatch`] when `query.len()` disagrees with tree dimension.
    pub fn nearest(
        &self,
        query: &[f64],
    ) -> Result<Option<(usize, f64)>, crate::NumericError> {
        self.0.nearest(query).map_err(map_geom_err)
    }

    /// Returns up to `k` nearest neighbors `(point_index, distance)` sorted by distance.
    ///
    /// # Errors
    /// [`crate::NumericError::ShapeMismatch`] when `query.len()` disagrees with tree dimension.
    pub fn k_nearest(
        &self,
        query: &[f64],
        k: usize,
    ) -> Result<Vec<(usize, f64)>, crate::NumericError> {
        self.0.k_nearest(query, k).map_err(map_geom_err)
    }

    /// Returns all `(point_index, distance)` within Euclidean `radius` of `query`.
    ///
    /// # Errors
    /// [`crate::NumericError::ShapeMismatch`] when `query.len()` disagrees with tree dimension.
    pub fn radius_search(
        &self,
        query: &[f64],
        radius: f64,
    ) -> Result<Vec<(usize, f64)>, crate::NumericError> {
        self.0.radius_search(query, radius).map_err(map_geom_err)
    }
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

    fn sub3(
        a: [f64; 3],
        b: [f64; 3],
    ) -> [f64; 3] {
        [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
    }
    fn cross3(
        a: [f64; 3],
        b: [f64; 3],
    ) -> [f64; 3] {
        [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ]
    }
    fn dot3(
        a: [f64; 3],
        b: [f64; 3],
    ) -> f64 {
        a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
    }
    fn circumcircle(
        a: [f64; 2],
        b: [f64; 2],
        c: [f64; 2],
    ) -> Option<([f64; 2], f64)> {
        let d = 2.0 * (a[0] * (b[1] - c[1]) + b[0] * (c[1] - a[1]) + c[0] * (a[1] - b[1]));
        if d.abs() < 1e-14 {
            return None;
        }
        let a2 = a[0] * a[0] + a[1] * a[1];
        let b2 = b[0] * b[0] + b[1] * b[1];
        let c2 = c[0] * c[0] + c[1] * c[1];
        let ux = (a2 * (b[1] - c[1]) + b2 * (c[1] - a[1]) + c2 * (a[1] - b[1])) / d;
        let uy = (a2 * (c[0] - b[0]) + b2 * (a[0] - c[0]) + c2 * (b[0] - a[0])) / d;
        let r2 = (a[0] - ux) * (a[0] - ux) + (a[1] - uy) * (a[1] - uy);
        Some(([ux, uy], r2))
    }

    #[test]
    fn the_distance_of_a_pythagorean_triple() {
        assert!((distance(&[0.0, 0.0], &[3.0, 4.0]).unwrap() - 5.0).abs() < 1e-12);
        assert!(distance(&[0.0], &[1.0, 2.0]).is_err());
    }

    #[test]
    fn the_volume_of_a_unit_tetrahedron() {
        let points = [0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
        assert!((tetrahedron_volume(&points) - 1.0 / 6.0).abs() < 1e-12);
    }

    #[test]
    fn convex_hull_2d_of_square_with_interior_points() {
        let pts = [
            [0.0, 0.0],
            [2.0, 0.0],
            [2.0, 2.0],
            [0.0, 2.0],
            [1.0, 1.0],
            [1.0, 0.0], // collinear on bottom edge
            [0.5, 1.2],
        ];
        let hull = convex_hull_2d(&pts);
        assert_eq!(hull.len(), 4);
        assert_eq!(hull, vec![[0.0, 0.0], [2.0, 0.0], [2.0, 2.0], [0.0, 2.0]]);
    }

    #[test]
    fn convex_hull_3d_of_cube_with_interior_point() {
        let pts = [
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [1.0, 1.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
            [1.0, 0.0, 1.0],
            [1.0, 1.0, 1.0],
            [0.0, 1.0, 1.0],
            [0.5, 0.5, 0.5], // interior
        ];
        let faces = convex_hull_3d(&pts);
        // A cube has 6 quad faces -> 12 triangles.
        assert_eq!(faces.len(), 12);
        // No face should reference the interior point (index 8), and all points
        // must lie on or behind every outward face.
        for &face in &faces {
            assert!(!face.contains(&8));
            let n = cross3(
                sub3(pts[face[1]], pts[face[0]]),
                sub3(pts[face[2]], pts[face[0]]),
            );
            for &p in &pts {
                assert!(dot3(n, sub3(p, pts[face[0]])) <= 1e-10);
            }
        }
    }

    #[test]
    fn delaunay_and_voronoi_of_unit_square_with_center() {
        let pts = [[0.0, 0.0], [2.0, 0.0], [2.0, 2.0], [0.0, 2.0], [1.0, 1.0]];
        let tris = delaunay_2d(&pts);
        assert_eq!(tris.len(), 4);
        // Verify empty-circumcircle property for every triangle.
        for &tri in &tris {
            let (center, r2) = circumcircle(pts[tri[0]], pts[tri[1]], pts[tri[2]]).unwrap();
            for (idx, &p) in pts.iter().enumerate() {
                if !tri.contains(&idx) {
                    let dx = p[0] - center[0];
                    let dy = p[1] - center[1];
                    assert!(dx * dx + dy * dy >= r2 - 1e-10);
                }
            }
        }
        let vor = voronoi_2d(&pts);
        assert_eq!(vor.vertices.len(), 4);
        assert_eq!(vor.edges.len(), 4);
        assert_eq!(vor.rays.len(), 4);
        assert_eq!(vor.cells[4].len(), 4); // Center site has a closed 4-vertex cell.
    }

    #[test]
    fn point_in_polygon_and_intersections() {
        let square = [[0.0, 0.0], [2.0, 0.0], [2.0, 2.0], [0.0, 2.0]];
        assert!(point_in_polygon([1.0, 1.0], &square));
        assert!(point_in_polygon([0.0, 1.0], &square)); // on boundary
        assert!(!point_in_polygon([3.0, 1.0], &square));

        let hit = segment_intersection_2d([0.0, 0.0], [2.0, 2.0], [0.0, 2.0], [2.0, 0.0]).unwrap();
        assert!((hit[0] - 1.0).abs() < 1e-12 && (hit[1] - 1.0).abs() < 1e-12);
        assert!(segment_intersection_2d([0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0]).is_none());

        let (t, rhit) =
            ray_segment_intersection_2d([0.0, 0.5], [1.0, 0.0], [2.0, 0.0], [2.0, 2.0]).unwrap();
        assert!((t - 2.0).abs() < 1e-12);
        assert!((rhit[0] - 2.0).abs() < 1e-12 && (rhit[1] - 0.5).abs() < 1e-12);

        let (t3, hit3) = ray_triangle_intersection_3d(
            [0.25, 0.25, -1.0],
            [0.0, 0.0, 1.0],
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
        )
        .unwrap();
        assert!((t3 - 1.0).abs() < 1e-12);
        assert!((hit3[2] - 0.0).abs() < 1e-12);
        assert!(
            segment_triangle_intersection_3d(
                [0.25, 0.25, -1.0],
                [0.25, 0.25, 1.0],
                [0.0, 0.0, 0.0],
                [1.0, 0.0, 0.0],
                [0.0, 1.0, 0.0],
            )
            .is_some()
        );
    }

    #[test]
    fn kdtree_and_bvh_nearest_and_ray_queries() {
        let pts = vec![
            vec![0.0, 0.0],
            vec![1.0, 0.0],
            vec![0.0, 1.0],
            vec![5.0, 5.0],
            vec![2.0, 2.0],
        ];
        let tree = KdTree::build(pts).unwrap();
        let (idx, d) = tree.nearest(&[0.9, 0.1]).unwrap().unwrap();
        assert_eq!(idx, 1);
        assert!((d - 0.02f64.sqrt()).abs() < 1e-12);

        let knn = tree.k_nearest(&[0.0, 0.0], 3).unwrap();
        assert_eq!(knn.len(), 3);
        assert_eq!(knn[0].0, 0);

        let rad = tree.radius_search(&[0.0, 0.0], 1.1).unwrap();
        assert_eq!(rad.len(), 3);

        let boxes = [
            Aabb {
                min: [0.0, 0.0, 0.0],
                max: [1.0, 1.0, 1.0],
            },
            Aabb {
                min: [5.0, 5.0, 5.0],
                max: [6.0, 6.0, 6.0],
            },
        ];
        let bvh = Bvh::build(&boxes);
        let (bidx, bd) = bvh.nearest_aabb([2.0, 0.5, 0.5]).unwrap();
        assert_eq!(bidx, 0);
        assert!((bd - 1.0).abs() < 1e-12);
        assert_eq!(
            bvh.ray_intersect([0.5, 0.5, -2.0], [0.0, 0.0, 1.0]),
            vec![0]
        );
    }
}

#[cfg(test)]
mod kernel_regressions {
    use super::*;

    #[test]
    fn spatial_indexes_refuse_nonfinite_coordinates() {
        assert!(KdTree::build(vec![vec![f64::NAN]]).is_err());
        let tree = KdTree::build(vec![vec![1.0, 2.0]]).unwrap();
        assert!(tree.nearest(&[f64::INFINITY, 2.0]).is_err());
        assert!(tree.radius_search(&[1.0, 2.0], f64::NAN).is_err());
    }
}
