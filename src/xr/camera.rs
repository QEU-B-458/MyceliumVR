use core::panic;

use bevy_app::{App, Plugin};
use bevy_camera::{Camera3d, CameraProjection};
use bevy_ecs::{component::Component, schedule::SystemSet};
use bevy_math::{Mat4, Vec3A, Vec4};
use bevy_reflect::std_traits::ReflectDefault;
use bevy_reflect::Reflect;
use bevy_render::extract_component::{ExtractComponent, ExtractComponentPlugin};

use crate::xr::session::XrTracker;

pub struct XrCameraPlugin;

impl Plugin for XrCameraPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(ExtractComponentPlugin::<XrCamera>::default());
    }
}

#[derive(Clone, Copy, Default, PartialEq, Eq, Debug, Hash, SystemSet)]
pub struct XrViewInit;

#[derive(Debug, Clone, Reflect)]
#[reflect(Default)]
pub struct XrProjection {
    pub projection_matrix: Mat4,
    pub near: f32,
}

impl Default for XrProjection {
    fn default() -> Self {
        Self {
            near: 0.1,
            projection_matrix: Mat4::IDENTITY,
        }
    }
}

/// Marker component for an XR view. It is the backends responsibility to update this.
#[derive(Clone, Copy, Component, ExtractComponent, Debug, Default)]
#[require(Camera3d, XrTracker)]
pub struct XrCamera(pub u32);

impl CameraProjection for XrProjection {
    fn update(&mut self, _width: f32, _height: f32) {}

    fn far(&self) -> f32 {
        self.projection_matrix.to_cols_array()[14]
            / (self.projection_matrix.to_cols_array()[10] + 1.0)
    }

    fn get_frustum_corners(&self, z_near: f32, z_far: f32) -> [Vec3A; 8] {
        fn normalized_corner(inverse_matrix: &Mat4, near: f32, ndc_x: f32, ndc_y: f32) -> Vec3A {
            let clip_pos = Vec4::new(ndc_x * near, ndc_y * near, near, near);
            Vec3A::from_vec4(inverse_matrix.mul_vec4(clip_pos)) / near * Vec3A::new(1., 1., -1.)
        }

        let inv = self.projection_matrix.inverse();
        let norm_br = normalized_corner(&inv, self.near, 1., -1.);
        let norm_tr = normalized_corner(&inv, self.near, 1., 1.);
        let norm_tl = normalized_corner(&inv, self.near, -1., 1.);
        let norm_bl = normalized_corner(&inv, self.near, -1., -1.);

        [
            norm_br * z_near,
            norm_tr * z_near,
            norm_tl * z_near,
            norm_bl * z_near,
            norm_br * z_far,
            norm_tr * z_far,
            norm_tl * z_far,
            norm_bl * z_far,
        ]
    }

    fn get_clip_from_view(&self) -> Mat4 {
        self.projection_matrix
    }

    fn get_clip_from_view_for_sub(&self, _sub_view: &bevy_camera::SubCameraView) -> Mat4 {
        panic!("sub view not supported for xr camera");
    }
}

#[doc(hidden)]
#[derive(Clone, Copy, Debug)]
pub struct Fov {
    pub angle_left: f32,
    pub angle_right: f32,
    pub angle_down: f32,
    pub angle_up: f32,
}

/// Calculates an asymmetrical perspective projection matrix for XR rendering. This API is for internal use only.
#[doc(hidden)]
pub fn calculate_projection(near_z: f32, fov: Fov) -> Mat4 {
    let far_z = -1.;

    let tan_angle_left = fov.angle_left.tan();
    let tan_angle_right = fov.angle_right.tan();

    let tan_angle_down = fov.angle_down.tan();
    let tan_angle_up = fov.angle_up.tan();

    let tan_angle_width = tan_angle_right - tan_angle_left;

    let tan_angle_height = tan_angle_up - tan_angle_down;

    let offset_z = 0.;

    let mut cols: [f32; 16] = [0.0; 16];

    if far_z <= near_z {
        cols[0] = 2. / tan_angle_width;
        cols[4] = 0.;
        cols[8] = (tan_angle_right + tan_angle_left) / tan_angle_width;
        cols[12] = 0.;

        cols[1] = 0.;
        cols[5] = 2. / tan_angle_height;
        cols[9] = (tan_angle_up + tan_angle_down) / tan_angle_height;
        cols[13] = 0.;

        cols[2] = 0.;
        cols[6] = 0.;
        cols[10] = -1.;
        cols[14] = -(near_z + offset_z);

        cols[3] = 0.;
        cols[7] = 0.;
        cols[11] = -1.;
        cols[15] = 0.;

        let z_reversal = Mat4::from_cols_array_2d(&[
            [1f32, 0., 0., 0.],
            [0., 1., 0., 0.],
            [0., 0., -1., 0.],
            [0., 0., 1., 1.],
        ]);

        return z_reversal * Mat4::from_cols_array(&cols);
    } else {
        cols[0] = 2. / tan_angle_width;
        cols[4] = 0.;
        cols[8] = (tan_angle_right + tan_angle_left) / tan_angle_width;
        cols[12] = 0.;

        cols[1] = 0.;
        cols[5] = 2. / tan_angle_height;
        cols[9] = (tan_angle_up + tan_angle_down) / tan_angle_height;
        cols[13] = 0.;

        cols[2] = 0.;
        cols[6] = 0.;
        cols[10] = -(far_z + offset_z) / (far_z - near_z);
        cols[14] = -(far_z * (near_z + offset_z)) / (far_z - near_z);

        cols[3] = 0.;
        cols[7] = 0.;
        cols[11] = -1.;
        cols[15] = 0.;
    }

    Mat4::from_cols_array(&cols)
}
