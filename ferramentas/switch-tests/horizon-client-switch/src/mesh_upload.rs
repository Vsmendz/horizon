use super::*;

type SectionKey = (Key, i32);
type Buffer = <GlesApi as HalApi>::Buffer;

#[derive(Default)]
pub(super) struct SectionUploads {
    sections: BTreeMap<SectionKey, ResidentSection>,
}

struct ResidentSection {
    revision: u64,
    buffer: Buffer,
    bytes: usize,
    uploaded: usize,
}

fn upload_slice(uploaded: usize, bytes: usize, staging_bytes: usize) -> std::ops::Range<usize> {
    let remaining = (STAGING_BUFFER_BYTES as usize - staging_bytes) & !3;
    uploaded..(uploaded + remaining.min(bytes - uploaded))
}

impl SectionUploads {
    pub(super) unsafe fn stage(
        &mut self,
        device: &<GlesApi as HalApi>::Device,
        meshes: &BTreeMap<SectionKey, SectionMesh>,
        staging: &mut Vec<u8>,
        center: Key,
        eye_section: i32,
    ) -> Result<Option<(SectionKey, BufferCopy)>, wgpu_hal::DeviceError> {
        let expired: Vec<_> = self
            .sections
            .keys()
            .copied()
            .filter(|key| !meshes.contains_key(key))
            .collect();
        for key in expired {
            unsafe { device.destroy_buffer(self.sections.remove(&key).unwrap().buffer) };
        }
        let next = meshes
            .iter()
            .filter(|(key, mesh)| {
                self.sections
                    .get(key)
                    .is_none_or(|gpu| gpu.revision != mesh.revision || gpu.uploaded != gpu.bytes)
            })
            .min_by_key(|(key, _)| {
                let ((x, z), y) = **key;
                (
                    x.abs_diff(center.0) + z.abs_diff(center.1) + y.abs_diff(eye_section),
                    **key,
                )
            });
        let Some((&key, mesh)) = next else {
            return Ok(None);
        };
        let data = bytemuck::cast_slice(&mesh.vertices);
        if self
            .sections
            .get(&key)
            .is_none_or(|gpu| gpu.revision != mesh.revision)
        {
            let buffer = unsafe {
                device.create_buffer(&BufferDescriptor {
                    label: Some("vanilla terrain section"),
                    size: data.len() as u64,
                    usage: wgpu_types::BufferUses::COPY_DST | wgpu_types::BufferUses::VERTEX,
                    memory_flags: MemoryFlags::empty(),
                })
            }?;
            let section = ResidentSection {
                revision: mesh.revision,
                buffer,
                bytes: data.len(),
                uploaded: 0,
            };
            if let Some(old) = self.sections.insert(key, section) {
                unsafe { device.destroy_buffer(old.buffer) };
            }
        }
        let section = self.sections.get_mut(&key).unwrap();
        let range = upload_slice(section.uploaded, section.bytes, staging.len());
        let Some(size) = NonZeroU64::new(range.len() as u64) else {
            return Ok(None);
        };
        let copy = BufferCopy {
            src_offset: staging.len() as u64,
            dst_offset: range.start as u64,
            size,
        };
        section.uploaded = range.end;
        staging.extend_from_slice(&data[range]);
        Ok(Some((key, copy)))
    }

    pub(super) fn buffer(&self, key: SectionKey) -> &Buffer {
        &self.sections[&key].buffer
    }

    pub(super) unsafe fn draw(&self, encoder: &mut <GlesApi as HalApi>::CommandEncoder, vp: Mat4) {
        for (&key, section) in &self.sections {
            if section.uploaded == section.bytes && section_visible(vp, key) {
                unsafe {
                    encoder.set_vertex_buffer(
                        0,
                        BufferBinding::new_unchecked(&section.buffer, 0, None),
                    );
                    encoder.draw(0, (section.bytes / size_of::<TerrainVertex>()) as u32, 0, 1);
                }
            }
        }
    }

    pub(super) unsafe fn shutdown(self, device: &<GlesApi as HalApi>::Device) {
        for section in self.sections.into_values() {
            unsafe { device.destroy_buffer(section.buffer) };
        }
    }
}

#[test]
fn section_larger_than_frame_budget_uploads_without_dropping_its_tail() {
    let bytes = 8 * 1024 * 1024;
    let first = upload_slice(0, bytes, 1024);
    assert_eq!(first, 0..6 * 1024 * 1024 - 1024);
    let second = upload_slice(first.end, bytes, 1024);
    assert_eq!(second, first.end..bytes);
    assert_eq!(upload_slice(bytes, bytes, 1024), bytes..bytes);
}
