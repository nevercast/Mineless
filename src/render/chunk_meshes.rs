use std::collections::HashMap;

use glam::IVec3;
use wgpu::util::DeviceExt;

use sim::world::CHUNK_SIZE;

use super::camera::Frustum;
use crate::mesher::Mesh;

/// GPU buffers for each chunk that has visible faces, keyed by chunk coordinate.
/// One buffer pair per chunk for now; region arenas replace this when batched draws are needed.
#[derive(Default)]
pub struct ChunkMeshes {
    meshes: HashMap<IVec3, GpuMesh>,
}

pub struct GpuMesh {
    pub vertex_buffer: wgpu::Buffer,
    /// The chunk's world position, read once per vertex as an instance attribute.
    pub origin_buffer: wgpu::Buffer,
    pub index_buffer: wgpu::Buffer,
    pub index_count: u32,
}

impl ChunkMeshes {
    pub fn insert(&mut self, device: &wgpu::Device, chunk: IVec3, mesh: &Mesh) {
        if mesh.indices.is_empty() {
            self.meshes.remove(&chunk);
            return;
        }
        let gpu_mesh = GpuMesh {
            vertex_buffer: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("chunk vertices"),
                contents: bytemuck::cast_slice(&mesh.vertices),
                usage: wgpu::BufferUsages::VERTEX,
            }),
            origin_buffer: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("chunk origin"),
                contents: bytemuck::bytes_of(&(chunk * CHUNK_SIZE as i32).as_vec3().to_array()),
                usage: wgpu::BufferUsages::VERTEX,
            }),
            index_buffer: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("chunk indices"),
                contents: bytemuck::cast_slice(&mesh.indices),
                usage: wgpu::BufferUsages::INDEX,
            }),
            index_count: mesh.indices.len() as u32,
        };
        self.meshes.insert(chunk, gpu_mesh);
    }

    /// Meshes of the chunks that might be on screen.
    pub fn visible<'a>(&'a self, frustum: &'a Frustum) -> impl Iterator<Item = &'a GpuMesh> {
        self.meshes.iter().filter_map(|(chunk, mesh)| {
            let min = (*chunk * CHUNK_SIZE as i32).as_vec3();
            frustum
                .intersects_box(min, min + CHUNK_SIZE as f32)
                .then_some(mesh)
        })
    }
}
