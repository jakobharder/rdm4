use bytes::{Buf, Bytes};
use std::{cmp::Ordering, path::Path};

use std::fs::File;

use std::str;

use half::f16;

use nalgebra::*;

#[macro_use]
extern crate log;

#[macro_use]
extern crate approx;

pub mod gltf_export;
pub mod gltf_reader;
pub mod rdm_anim;
pub mod rdm_anim_writer;
pub mod rdm_material;
pub mod rdm_writer;
pub mod vertex;
use crate::rdm_anim::RDAnim;
use rdm_material::RDMaterial;

use vertex::VertexFormat2;

#[derive(Debug)]
pub struct RDModell {
    size: u32,
    buffer: Bytes,
    mesh_info: Vec<MeshInstance>,
    pub joints: Option<Vec<RDJoint>>,
    pub triangle_indices: Vec<Triangle>,

    meta: u32,
    pub vertex: VertexFormat2,

    triangles_offset: u32,
    pub triangles_idx_count: u32,
    triangles_idx_size: u32,

    anim: Option<RDAnim>,
    pub mat: Option<RDMaterial>,
}

trait Seek {
    fn seek(&mut self, from_start: u32, file_size: u32);
}

impl Seek for Bytes {
    fn seek(&mut self, offset_from_start: u32, file_size: u32) {
        let already_read = file_size - self.remaining() as u32;
        let cnt: usize = (offset_from_start.checked_sub(already_read).unwrap()) as usize;
        self.advance(cnt);
    }
}

#[derive(Debug, Clone)]
pub struct RDJoint {
    name: String,
    nameptr: u32,
    transition: [f32; 3],
    quaternion: [f32; 4],
    parent: u8,
    locked: bool,
}

#[derive(Debug, Eq)]
pub struct MeshInstance {
    start_index_location: u32,
    index_count: u32,
    mesh: u32,
}

impl Ord for MeshInstance {
    fn cmp(&self, other: &Self) -> Ordering {
        self.mesh.cmp(&other.mesh)
    }
}

impl PartialOrd for MeshInstance {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for MeshInstance {
    fn eq(&self, other: &Self) -> bool {
        self.mesh == other.mesh
    }
}

#[allow(dead_code)]
impl RDModell {
    const META_OFFSET: u32 = 32;
    const META_COUNT: u32 = 8; //neg
    const META_SIZE: u32 = 4; //neg
    const VERTEX_META: u32 = 12;
    const TRIANGLES_META: u32 = 16;

    pub fn has_skin(&self) -> bool {
        self.joints.is_some()
    }

    pub fn add_anim(&mut self, anim: RDAnim) {
        self.anim = Some(anim);
    }

    pub fn check_has_magic_byte(bytes: &[u8]) {
        assert_eq!(
            bytes[0], 0x52,
            "Magic Bytes 0x52, 0x44, 0x4D, 0x01, 0x14 not found !"
        );
        assert_eq!(
            bytes[1], 0x44,
            "Magic Bytes 0x52, 0x44, 0x4D, 0x01, 0x14 not found !"
        );
        assert_eq!(
            bytes[2], 0x4D,
            "Magic Bytes 0x52, 0x44, 0x4D, 0x01, 0x14 not found !"
        );
        assert_eq!(
            bytes[3], 0x01,
            "Magic Bytes 0x52, 0x44, 0x4D, 0x01, 0x14 not found !"
        );
        assert_eq!(
            bytes[4], 0x14,
            "Magic Bytes 0x52, 0x44, 0x4D, 0x01, 0x14 not found !"
        );
    }

    pub fn check_multi_mesh(
        mut multi_buffer: Bytes,
        meta_deref: u32,
        size: u32,
    ) -> Vec<MeshInstance> {
        multi_buffer.seek(meta_deref + 20, size);
        let first_instance = multi_buffer.get_u32_le();

        multi_buffer.seek(first_instance - RDModell::META_COUNT, size);
        let mesh_count = multi_buffer.get_u32_le();
        assert_eq!(multi_buffer.get_u32_le(), 28);
        warn!("mesh_count: {}", mesh_count);
        let mut v = Vec::with_capacity(mesh_count as usize);
        for _ in 0..mesh_count {
            v.push(MeshInstance {
                start_index_location: multi_buffer.get_u32_le(),
                index_count: multi_buffer.get_u32_le(),
                mesh: multi_buffer.get_u32_le(),
            });
            multi_buffer.advance(28 - 12);
        }
        warn!("meshes: {:?}", v);
        assert_eq!(v.is_empty(), false);
        v.sort();
        v
    }

    pub fn add_skin(&mut self) {
        let mut skin_buffer = self.buffer.clone();
        skin_buffer.advance(40);
        let skin_offset = skin_buffer.get_u32_le();
        assert_eq!(skin_offset != 0, true, "File does not contain a skin !");

        skin_buffer.seek(skin_offset, self.size);

        let first_skin_offset = skin_buffer.get_u32_le();
        let joint_count_ptr = first_skin_offset - RDModell::META_COUNT;

        skin_buffer.seek(joint_count_ptr, self.size);

        let joint_count = skin_buffer.get_u32_le();
        let joint_size = skin_buffer.get_u32_le();

        let mut joints_vec: Vec<RDJoint> = Vec::with_capacity(joint_count as usize);

        let mut joint_name_buffer = skin_buffer.clone();

        let len_first_joint_name_ptr = joint_name_buffer.get_u32_le() - RDModell::META_COUNT;
        joint_name_buffer.seek(len_first_joint_name_ptr, self.size);

        assert_eq!(joint_size, 84);
        for _ in 0..joint_count {
            let len_joint_name = joint_name_buffer.get_u32_le();
            assert_eq!(joint_name_buffer.get_u32_le(), 1);
            let name = str::from_utf8(&joint_name_buffer[..len_joint_name as usize]).unwrap();
            let joint_name = String::from(name);
            joint_name_buffer.advance(len_joint_name as usize);

            let nameptr = skin_buffer.get_u32_le();

            let tx = skin_buffer.get_f32_le();
            let ty = skin_buffer.get_f32_le();
            let tz = skin_buffer.get_f32_le();

            let rx = -skin_buffer.get_f32_le();
            let ry = -skin_buffer.get_f32_le();
            let rz = -skin_buffer.get_f32_le();
            let rw = -skin_buffer.get_f32_le();

            let quaternion = Quaternion::new(rw, rx, ry, rz);
            let unit_quaternion = UnitQuaternion::from_quaternion(quaternion);

            let quaternion_mat4 = unit_quaternion.quaternion().coords;

            let joint_translatio: Translation3<f32> = Translation3::new(tx, ty, tz);

            let inv_bindmat =
                (unit_quaternion.to_homogeneous()) * (joint_translatio.to_homogeneous());
            let iv_x = inv_bindmat.m14;
            let iv_y = inv_bindmat.m24;
            let iv_z = inv_bindmat.m34;

            let trans_point = Translation3::new(iv_x, iv_y, iv_z).inverse();

            let parent_id = skin_buffer.get_u8();

            let joint = RDJoint {
                name: joint_name,
                nameptr,
                transition: [trans_point.x, trans_point.y, trans_point.z],
                quaternion: [
                    quaternion_mat4.x,
                    quaternion_mat4.y,
                    quaternion_mat4.z,
                    quaternion_mat4.w,
                ],
                parent: parent_id,
                locked: false,
            };

            joints_vec.push(joint);
            skin_buffer.advance(84 - 33);
        }

        self.joints = Some(joints_vec);
    }

    fn new(buf: Vec<u8>) -> Self {
        RDModell::check_has_magic_byte(&buf);

        let size = buf.len() as u32;
        let buffer = Bytes::from(buf);
        let vvert = VertexFormat2::read_format(buffer.clone(), size);

        info!(
            "Read {} vertices of type {} ({} bytes)",
            vvert.len(),
            vvert,
            vvert.get_size()
        );
        let mut nbuffer = buffer.clone();

        nbuffer.advance(RDModell::META_OFFSET as usize);
        let meta = nbuffer.get_u32_le();

        nbuffer.get_u32_le();

        let _skin_there = nbuffer.get_u32_le() > 0;
        let mesh_info = RDModell::check_multi_mesh(buffer.clone(), meta, size);

        nbuffer.seek(meta, size);
        nbuffer.advance(RDModell::VERTEX_META as usize);
        let vertex_offset = nbuffer.get_u32_le();

        let triangles_offset = nbuffer.get_u32_le();

        let vertex_count_off = vertex_offset - RDModell::META_COUNT;
        info!("off : {}", vertex_count_off);
        nbuffer.seek(vertex_count_off, size);

        let triangles_count_off = triangles_offset - RDModell::META_COUNT;
        nbuffer.seek(triangles_count_off, size);
        let triangles_idx_count = nbuffer.get_u32_le();
        let triangles_idx_size = nbuffer.get_u32_le();

        // read indices for triangles
        assert_eq!(triangles_idx_size, 2);
        assert_eq!(triangles_idx_count % 3, 0);

        //let mut triangles_idx_buffer = nbuffer.clone();
        let mut triangles_idx_buffer = nbuffer;
        triangles_idx_buffer.truncate((triangles_idx_size * triangles_idx_count) as usize);
        let triangles_real_count = triangles_idx_count / 3;
        let mut triangles = Vec::with_capacity(triangles_real_count as usize);
        for _ in 0..triangles_real_count {
            let t = Triangle {
                indices: [
                    triangles_idx_buffer.get_u16_le(),
                    triangles_idx_buffer.get_u16_le(),
                    triangles_idx_buffer.get_u16_le(),
                ],
            };
            triangles.push(t);
        }

        RDModell {
            size,
            buffer,
            mesh_info,
            joints: None,
            triangle_indices: triangles,
            meta,
            vertex: vvert,
            triangles_offset,
            triangles_idx_count,
            triangles_idx_size,
            anim: None,
            mat: None,
        }
    }
}

#[derive(Debug, Copy, Clone)]
#[repr(C)]
pub struct Triangle {
    indices: [u16; 3],
}

#[derive(Clone, Debug)]
#[repr(C)]
pub struct P4h<T> {
    pos: [T; 4],
}
#[derive(Clone, Debug)]
#[repr(C)]
pub struct N4b {
    normals: [u8; 4],
}
#[derive(Clone, Debug)]
#[repr(C)]
pub struct G4b {
    tangent: [u8; 4],
}
#[derive(Clone, Debug)]
#[repr(C)]
pub struct B4b {
    binormal: [u8; 4],
}
#[derive(Clone, Debug)]
#[repr(C)]
pub struct T2h {
    tex: [f16; 2],
}

#[derive(Clone, Debug)]
#[repr(C)]
pub struct I4b {
    blend_idx: [u8; 4],
}

#[derive(Clone, Debug)]
#[repr(C)]
pub struct W4b {
    blend_weight: [u8; 4],
}

#[derive(Clone, Debug)]
#[repr(C)]
pub struct C4c {
    unknown: [u8; 4],
}

struct VertexFormatSize;

#[allow(dead_code)]
#[allow(non_upper_case_globals)]
impl VertexFormatSize {
    const P4h: u32 = 8;
    const P4h_N4b_T2h: u32 = 16;
    const P4h_N4b_T2h_C4c: u32 = 20;
    const P4h_N4b_T2h_I4b: u32 = 20;
    const P4h_N4b_G4b_B4b_T2h: u32 = 24;
    const P4h_N4b_T2h_I4b_W4b: u32 = 24;
    const P4h_N4b_G4b_B4b_T2h_C4c: u32 = 28;
    const P4h_N4b_G4b_B4b_T2h_I4b: u32 = 28;
    const P4h_N4b_G4b_B4b_T2h_I4b_W4b: u32 = 32;
    const P4h_N4b_G4b_B4b_T2h_I4b_I4b_I4b_I4b_W4b_W4b_W4b_W4b: u32 = 56;
}

impl From<&Path> for RDModell {
    fn from(f_path: &Path) -> Self {
        let mut f = File::open(f_path).unwrap();
        let metadata = f.metadata().unwrap();
        let len = metadata.len() as usize;
        let mut buffer = vec![0; len];
        std::io::Read::read_exact(&mut f, &mut buffer).expect("I/O ERROR");

        let buffer_len = buffer.len();
        info!("loaded {:?} into buffer", f_path.to_str().unwrap());

        info!("buffer size: {}", buffer_len);
        RDModell::new(buffer)
    }
}

impl From<&str> for RDModell {
    fn from(str_path: &str) -> Self {
        RDModell::from(Path::new(str_path))
    }
}

impl From<&String> for RDModell {
    fn from(string_path: &String) -> Self {
        RDModell::from(Path::new(string_path))
    }
}

#[cfg(test)]
mod tests_intern {

    use super::*;

    #[test]
    fn fishery_others_lod2() {
        // for Miri test
        // fishery_others_lod2.rdm
        let v = vec![
            0x52, 0x44, 0x4D, 0x01, 0x14, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x04, 0x00,
            0x00, 0x00, 0x1C, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x30, 0x00, 0x00, 0x00,
            0x54, 0x00, 0x00, 0x00, 0x29, 0x01, 0x00, 0x00, 0xE3, 0x03, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x48, 0x00, 0x00, 0x00,
            0xA4, 0x00, 0x00, 0x00, 0x17, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x6B, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x47, 0x3A, 0x5C, 0x67,
            0x72, 0x61, 0x70, 0x68, 0x69, 0x63, 0x5F, 0x62, 0x61, 0x63, 0x6B, 0x75, 0x70, 0x5C,
            0x74, 0x6F, 0x62, 0x69, 0x61, 0x73, 0x5C, 0x61, 0x6E, 0x6E, 0x6F, 0x35, 0x5C, 0x61,
            0x73, 0x73, 0x65, 0x74, 0x73, 0x5C, 0x62, 0x75, 0x69, 0x6C, 0x64, 0x69, 0x6E, 0x67,
            0x73, 0x5C, 0x6F, 0x74, 0x68, 0x65, 0x72, 0x73, 0x5C, 0x66, 0x69, 0x73, 0x68, 0x65,
            0x72, 0x79, 0x5F, 0x6F, 0x74, 0x68, 0x65, 0x72, 0x73, 0x5C, 0x70, 0x6F, 0x6C, 0x69,
            0x73, 0x68, 0x5C, 0x75, 0x6D, 0x62, 0x61, 0x75, 0x5C, 0x66, 0x69, 0x73, 0x68, 0x65,
            0x72, 0x79, 0x5F, 0x75, 0x6D, 0x62, 0x61, 0x75, 0x5F, 0x62, 0x61, 0x6B, 0x69, 0x6E,
            0x67, 0x2E, 0x6D, 0x61, 0x78, 0x0A, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x43,
            0x75, 0x74, 0x6F, 0x75, 0x74, 0x2E, 0x72, 0x6D, 0x70, 0x01, 0x00, 0x00, 0x00, 0x5C,
            0x00, 0x00, 0x00, 0x8D, 0x01, 0x00, 0x00, 0xBF, 0x01, 0x00, 0x00, 0xF7, 0x01, 0x00,
            0x00, 0x37, 0x02, 0x00, 0x00, 0x3F, 0x03, 0x00, 0x00, 0x13, 0x02, 0x00, 0x00, 0xFF,
            0xFF, 0xFF, 0xFF, 0x00, 0x20, 0xBE, 0xBF, 0x00, 0x80, 0xE3, 0xBE, 0x00, 0xE0, 0x59,
            0xC0, 0x00, 0x00, 0xBA, 0x3F, 0x00, 0x80, 0xCF, 0xBE, 0x00, 0xC0, 0x03, 0xBF, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00,
            0x00, 0x1C, 0x00, 0x00, 0x00, 0xB1, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x06, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x63,
            0x75, 0x74, 0x6F, 0x75, 0x74, 0x01, 0x00, 0x00, 0x00, 0x18, 0x00, 0x00, 0x00, 0xDF,
            0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x10,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x06, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x04, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x14, 0x00, 0x00, 0x00, 0x03,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x1C, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x4E, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x20,
            0x00, 0x00, 0x00, 0x08, 0x00, 0x00, 0x00, 0xD0, 0x3D, 0x12, 0xB7, 0xDC, 0xBA, 0x00,
            0x00, 0xCC, 0x3D, 0x12, 0xB7, 0x3A, 0xBC, 0x00, 0x00, 0x25, 0x3D, 0x12, 0xB7, 0xF2,
            0xBC, 0x00, 0x00, 0x8D, 0x35, 0x12, 0xB7, 0xDC, 0xBA, 0x00, 0x00, 0x8D, 0x35, 0x12,
            0xB7, 0xEE, 0xBC, 0x00, 0x00, 0x9E, 0x32, 0x7C, 0xB6, 0x89, 0xBD, 0x00, 0x00, 0x9E,
            0x32, 0x7C, 0xB6, 0x24, 0xC0, 0x00, 0x00, 0x9E, 0x32, 0x7C, 0xB6, 0x07, 0xC1, 0x00,
            0x00, 0x9E, 0x32, 0x7C, 0xB6, 0xEA, 0xC1, 0x00, 0x00, 0x8D, 0x30, 0x7C, 0xB6, 0x89,
            0xBD, 0x00, 0x00, 0x8D, 0x30, 0x7C, 0xB6, 0x24, 0xC0, 0x00, 0x00, 0x8D, 0x30, 0x7C,
            0xB6, 0x07, 0xC1, 0x00, 0x00, 0x8D, 0x30, 0x7C, 0xB6, 0xEA, 0xC1, 0x00, 0x00, 0x14,
            0xAE, 0x7C, 0xB6, 0xCF, 0xC2, 0x00, 0x00, 0xB5, 0xAF, 0x7C, 0xB6, 0xAE, 0xC2, 0x00,
            0x00, 0x8B, 0xB4, 0x12, 0xB7, 0xDC, 0xBA, 0x00, 0x00, 0x90, 0xB4, 0x12, 0xB7, 0xB6,
            0xBF, 0x00, 0x00, 0x93, 0xB4, 0x16, 0xB7, 0xCD, 0xC1, 0x00, 0x00, 0x57, 0xB7, 0x1C,
            0xB7, 0x24, 0xC2, 0x00, 0x00, 0x45, 0xBC, 0x1C, 0xB7, 0x24, 0xC2, 0x00, 0x00, 0x9E,
            0xBC, 0x7C, 0xB6, 0xAE, 0xC2, 0x00, 0x00, 0xB8, 0xBC, 0x7C, 0xB6, 0xCF, 0xC2, 0x00,
            0x00, 0xF6, 0xBC, 0x16, 0xB7, 0xCD, 0xC1, 0x00, 0x00, 0xF7, 0xBC, 0x12, 0xB7, 0x62,
            0xBD, 0x00, 0x00, 0xAF, 0xBD, 0x7C, 0xB6, 0xDB, 0xC1, 0x00, 0x00, 0xAF, 0xBD, 0x7C,
            0xB6, 0x5D, 0xBE, 0x00, 0x00, 0xAF, 0xBD, 0x7C, 0xB6, 0xD9, 0xC0, 0x00, 0x00, 0xAF,
            0xBD, 0x7C, 0xB6, 0x55, 0xB8, 0x00, 0x00, 0xF1, 0xBD, 0x7C, 0xB6, 0xEA, 0xC1, 0x00,
            0x00, 0xF1, 0xBD, 0x7C, 0xB6, 0xD9, 0xC0, 0x00, 0x00, 0xF1, 0xBD, 0x7C, 0xB6, 0x5D,
            0xBE, 0x00, 0x00, 0xF1, 0xBD, 0x7C, 0xB6, 0x1E, 0xB8, 0x00, 0x00, 0x4E, 0x00, 0x00,
            0x00, 0x02, 0x00, 0x00, 0x00, 0x16, 0x00, 0x10, 0x00, 0x11, 0x00, 0x17, 0x00, 0x10,
            0x00, 0x16, 0x00, 0x17, 0x00, 0x0F, 0x00, 0x10, 0x00, 0x04, 0x00, 0x01, 0x00, 0x02,
            0x00, 0x03, 0x00, 0x01, 0x00, 0x04, 0x00, 0x03, 0x00, 0x00, 0x00, 0x01, 0x00, 0x1D,
            0x00, 0x19, 0x00, 0x1A, 0x00, 0x19, 0x00, 0x1D, 0x00, 0x1E, 0x00, 0x18, 0x00, 0x15,
            0x00, 0x1C, 0x00, 0x15, 0x00, 0x18, 0x00, 0x14, 0x00, 0x14, 0x00, 0x0D, 0x00, 0x15,
            0x00, 0x0D, 0x00, 0x14, 0x00, 0x0E, 0x00, 0x09, 0x00, 0x06, 0x00, 0x0A, 0x00, 0x06,
            0x00, 0x09, 0x00, 0x05, 0x00, 0x0C, 0x00, 0x0D, 0x00, 0x0E, 0x00, 0x0D, 0x00, 0x0C,
            0x00, 0x08, 0x00, 0x10, 0x00, 0x03, 0x00, 0x04, 0x00, 0x03, 0x00, 0x10, 0x00, 0x0F,
            0x00, 0x16, 0x00, 0x12, 0x00, 0x13, 0x00, 0x12, 0x00, 0x16, 0x00, 0x11, 0x00, 0x0C,
            0x00, 0x07, 0x00, 0x08, 0x00, 0x07, 0x00, 0x0C, 0x00, 0x0B, 0x00, 0x1D, 0x00, 0x18,
            0x00, 0x1C, 0x00, 0x18, 0x00, 0x1D, 0x00, 0x1A, 0x00, 0x1B, 0x00, 0x1E, 0x00, 0x1F,
            0x00, 0x1E, 0x00, 0x1B, 0x00, 0x19, 0x00, 0x01, 0x00, 0x00, 0x00, 0x1C, 0x00, 0x00,
            0x00, 0x07, 0x04, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x01, 0x00, 0x00, 0x00, 0x30, 0x00, 0x00, 0x00, 0x3F, 0x04, 0x00, 0x00, 0x4E,
            0x04, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x07, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x66, 0x69, 0x73, 0x68, 0x65,
            0x72, 0x79, 0x5E, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x67, 0x3A, 0x2F, 0x67,
            0x72, 0x61, 0x70, 0x68, 0x69, 0x63, 0x5F, 0x62, 0x61, 0x63, 0x6B, 0x75, 0x70, 0x2F,
            0x74, 0x6F, 0x62, 0x69, 0x61, 0x73, 0x2F, 0x61, 0x6E, 0x6E, 0x6F, 0x35, 0x2F, 0x61,
            0x73, 0x73, 0x65, 0x74, 0x73, 0x2F, 0x62, 0x75, 0x69, 0x6C, 0x64, 0x69, 0x6E, 0x67,
            0x73, 0x2F, 0x6F, 0x74, 0x68, 0x65, 0x72, 0x73, 0x2F, 0x66, 0x69, 0x73, 0x68, 0x65,
            0x72, 0x79, 0x5F, 0x6F, 0x74, 0x68, 0x65, 0x72, 0x73, 0x2F, 0x70, 0x6F, 0x6C, 0x69,
            0x73, 0x68, 0x2F, 0x75, 0x6D, 0x62, 0x61, 0x75, 0x2F, 0x64, 0x69, 0x66, 0x66, 0x75,
            0x73, 0x65, 0x2E, 0x70, 0x73, 0x64,
        ];

        let rdm = RDModell::new(v);
        assert_eq!(rdm.vertex.len(), 32);
        assert_eq!(rdm.vertex.get_size(), 8);
        assert_eq!(rdm.triangles_idx_count, 78);

        assert_eq!(
            rdm.triangles_idx_count as usize,
            rdm.triangle_indices.len() * 3
        );
    }
}
