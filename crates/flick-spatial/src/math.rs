use nalgebra::{Matrix3, Rotation3, SMatrix, SVector, UnitQuaternion, Vector3};

pub(crate) const EPS: f32 = 1.0e-6;

pub(crate) type Vec3 = Vector3<f32>;
pub(crate) type Mat3 = Matrix3<f32>;
pub(crate) type Mat6 = SMatrix<f32, 6, 6>;
pub(crate) type Vec6 = SVector<f32, 6>;

#[must_use]
pub(crate) fn v3(value: [f32; 3]) -> Vec3 {
    Vec3::new(value[0], value[1], value[2])
}

#[must_use]
pub(crate) fn a3(value: Vec3) -> [f32; 3] {
    [value.x, value.y, value.z]
}

#[must_use]
pub(crate) fn normalize_or(value: Vec3, fallback: Vec3) -> Vec3 {
    let norm = value.norm();
    if norm > EPS && norm.is_finite() {
        value / norm
    } else {
        fallback
    }
}

#[must_use]
pub(crate) fn unit_or_z(value: Vec3) -> Vec3 {
    normalize_or(value, Vec3::new(0.0, 0.0, 1.0))
}

#[must_use]
pub(crate) fn angle_rad(a: Vec3, b: Vec3) -> f32 {
    let an = normalize_or(a, Vec3::new(0.0, 0.0, 1.0));
    let bn = normalize_or(b, Vec3::new(0.0, 0.0, 1.0));
    an.dot(&bn).clamp(-1.0, 1.0).acos()
}

#[must_use]
pub(crate) fn angle_deg(a: Vec3, b: Vec3) -> f32 {
    angle_rad(a, b).to_degrees()
}

#[must_use]
pub(crate) fn rotation_from_scaled_axis(axis: Vec3) -> Mat3 {
    UnitQuaternion::from_scaled_axis(axis)
        .to_rotation_matrix()
        .into_inner()
}

#[must_use]
pub(crate) fn scaled_axis_from_rotation(rotation: Mat3) -> Vec3 {
    let rot = Rotation3::from_matrix_unchecked(rotation);
    UnitQuaternion::from_rotation_matrix(&rot).scaled_axis()
}

#[must_use]
pub(crate) fn nearest_rotation(matrix: Mat3) -> Option<Mat3> {
    let svd = matrix.svd(true, true);
    let (Some(u), Some(v_t)) = (svd.u, svd.v_t) else {
        return None;
    };
    let mut fix = Mat3::identity();
    if (u * v_t).determinant() < 0.0 {
        fix[(2, 2)] = -1.0;
    }
    Some(u * fix * v_t)
}

#[must_use]
pub(crate) fn median(values: &mut [f32]) -> f32 {
    values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let len = values.len();
    if len == 0 {
        return 0.0;
    }
    if len % 2 == 1 {
        values[len / 2]
    } else {
        (values[len / 2 - 1] + values[len / 2]) * 0.5
    }
}
