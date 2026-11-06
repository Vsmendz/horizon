/*!
Nintendo Switch (Horizon OS / libnx) external EGL + OpenGL ES 3.2 integration for `wgpu-hal`.

On Nintendo Switch, EGL (`libEGL.a`) and OpenGL ES (`libglapi.a` + `libdrm_nouveau.a`) are statically
linked via devkitPro/libnx and bound to `nwindowGetDefault()`. Neither `dlopen`/`libloading` nor Wayland
exists on Horizon OS. This module provides `AdapterContext`, `Adapter::new_external`, `Surface::new_external`,
and the `wgpu_hal::Surface` swapchain implementation (offscreen renderbuffer + Y-flip blit to framebuffer 0)
over the active libnx EGL context.
*/

use alloc::{format, string::String, sync::Arc, vec::Vec};
use core::{ffi, fmt, mem::ManuallyDrop, ops, ptr, time::Duration};
use glow::HasContext;
use parking_lot::{Mutex, MutexGuard, RwLock};

/// Wrapper around the active `glow::Context` bound on the Nintendo Switch thread.
pub struct AdapterContext {
    pub glow: Mutex<ManuallyDrop<glow::Context>>,
}

impl fmt::Debug for AdapterContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AdapterContext")
            .field("platform", &"Nintendo Switch (Horizon OS Static EGL)")
            .finish()
    }
}

unsafe impl Sync for AdapterContext {}
unsafe impl Send for AdapterContext {}

impl AdapterContext {
    pub fn is_owned(&self) -> bool {
        false
    }

    /// Returns the raw context pointer (null for external contexts).
    pub fn raw_context(&self) -> *mut ffi::c_void {
        ptr::null_mut()
    }

    /// Obtain a lock to the OpenGL context without affecting the EGL context binding.
    pub unsafe fn get_without_egl_lock(&self) -> MutexGuard<'_, ManuallyDrop<glow::Context>> {
        self.glow.lock()
    }

    /// Obtain a lock to the OpenGL context.
    pub fn lock<'a>(&'a self) -> AdapterContextLock<'a> {
        let glow = self.glow.lock();
        AdapterContextLock { glow }
    }
}

pub struct AdapterContextLock<'a> {
    glow: MutexGuard<'a, ManuallyDrop<glow::Context>>,
}

impl<'a> fmt::Debug for AdapterContextLock<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AdapterContextLock").finish()
    }
}

impl<'a> ops::Deref for AdapterContextLock<'a> {
    type Target = glow::Context;

    fn deref(&self) -> &Self::Target {
        &self.glow
    }
}

impl Drop for AdapterContext {
    fn drop(&mut self) {
        let mut gl = self.glow.lock();
        unsafe { ManuallyDrop::drop(&mut *gl) };
    }
}

#[derive(Debug)]
pub struct Instance {
    options: wgt::GlBackendOptions,
}

unsafe impl Send for Instance {}
unsafe impl Sync for Instance {}

impl crate::Instance for Instance {
    type A = super::Api;

    unsafe fn init(desc: &crate::InstanceDescriptor<'_>) -> Result<Self, crate::InstanceError> {
        Ok(Instance {
            options: desc.backend_options.gl.clone(),
        })
    }

    unsafe fn create_surface(
        &self,
        _display_handle: raw_window_handle::RawDisplayHandle,
        _window_handle: raw_window_handle::RawWindowHandle,
    ) -> Result<Surface, crate::InstanceError> {
        Ok(Surface {
            presentable: true,
            swap_fn: None,
            swapchain: RwLock::new(None),
            present_probe: Mutex::new(None),
        })
    }

    unsafe fn enumerate_adapters(
        &self,
        _surface_hint: Option<&Surface>,
    ) -> Vec<crate::ExposedAdapter<super::Api>> {
        let _ = &self.options;
        Vec::new()
    }
}

impl super::Adapter {
    /// Creates a new external adapter using the specified loader function (`eglGetProcAddress`).
    ///
    /// # Safety
    /// - The underlying OpenGL ES context must be current on the calling thread.
    pub unsafe fn new_external(
        fun: impl FnMut(&str) -> *const ffi::c_void,
        options: wgt::GlBackendOptions,
    ) -> Option<crate::ExposedAdapter<super::Api>> {
        let context = unsafe { glow::Context::from_loader_function(fun) };
        unsafe {
            Self::expose(
                AdapterContext {
                    glow: Mutex::new(ManuallyDrop::new(context)),
                },
                options,
            )
        }
    }

    pub fn adapter_context(&self) -> &AdapterContext {
        &self.shared.context
    }
}

impl super::Device {
    /// Returns the underlying adapter context.
    pub fn context(&self) -> &AdapterContext {
        &self.shared.context
    }

    pub fn diagnostic_atlas_sampling(
        &self,
        texture: &super::Texture,
        sampler: &super::Sampler,
        layers: &[[u8; 4]],
        mut log: impl FnMut(&str),
    ) -> Result<usize, String> {
        let (raw, _) = texture.inner.as_native();
        diagnostic_atlas::probe(&self.context().lock(), raw, sampler.raw, layers, &mut log)
    }

    /// Inspect the GLES state left by the last textured draw in a submission.
    pub fn diagnostic_atlas_binding(
        &self,
        pipeline: &super::RenderPipeline,
        view: &super::TextureView,
        sampler: &super::Sampler,
        mut log: impl FnMut(&str),
    ) {
        let gl = self.context().lock();
        unsafe {
            let program = pipeline.inner.program;
            let current = gl.get_parameter_program(glow::CURRENT_PROGRAM);
            let (expected_texture, target) = view.inner.as_native();
            let active = gl.get_parameter_i32(glow::ACTIVE_TEXTURE);
            let mut found = false;
            for index in 0..gl.get_active_uniforms(program) {
                let Some(uniform) = gl.get_active_uniform(program, index) else {
                    continue;
                };
                if uniform.utype != glow::SAMPLER_2D_ARRAY {
                    continue;
                }
                found = true;
                let Some(location) = gl.get_uniform_location(program, &uniform.name) else {
                    log(&format!(
                        "GLES_ATLAS_BIND missing_location={:?}",
                        uniform.name
                    ));
                    continue;
                };
                let mut units = [-1];
                gl.get_uniform_i32(program, &location, &mut units);
                let unit = units[0];
                let unit_count = gl.get_parameter_i32(glow::MAX_COMBINED_TEXTURE_IMAGE_UNITS);
                if !(0..unit_count).contains(&unit) {
                    log(&format!(
                        "GLES_ATLAS_BIND uniform={:?} invalid_unit={unit}",
                        uniform.name
                    ));
                    continue;
                }
                gl.active_texture(glow::TEXTURE0 + unit as u32);
                let bound = gl.get_parameter_texture(glow::TEXTURE_BINDING_2D_ARRAY);
                let bound_sampler = gl.get_parameter_sampler(glow::SAMPLER_BINDING);
                let parameters = bound.map(|_| {
                    [
                        glow::TEXTURE_BASE_LEVEL,
                        glow::TEXTURE_MAX_LEVEL,
                        glow::TEXTURE_MIN_FILTER,
                        glow::TEXTURE_MAG_FILTER,
                    ]
                    .map(|param| gl.get_tex_parameter_i32(glow::TEXTURE_2D_ARRAY, param))
                });
                let sampler_filter = bound_sampler
                    .map(|raw| gl.get_sampler_parameter_i32(raw, glow::TEXTURE_MIN_FILTER));
                let version = gl.version();
                let level_size = if bound.is_some() && (version.major, version.minor) >= (3, 1) {
                    Some(
                        [
                            glow::TEXTURE_WIDTH,
                            glow::TEXTURE_HEIGHT,
                            glow::TEXTURE_DEPTH,
                        ]
                        .map(|param| {
                            gl.get_tex_level_parameter_i32(glow::TEXTURE_2D_ARRAY, 0, param)
                        }),
                    )
                } else {
                    None
                };
                gl.active_texture(active as u32);
                log(&format!(
                    "GLES_ATLAS_BIND uniform={:?} unit={unit} active={active:#x} current={current:?} expected_program={program:?} target={target:#x} bound={bound:?} expected_texture={expected_texture:?} bound_sampler={bound_sampler:?} client_sampler={:?} sampler_map={:?} view_layers={:?} view_mips={:?} params(base,max,min,mag)={parameters:?} sampler_min={sampler_filter:?} level0={level_size:?} error={:#x}",
                    uniform.name,
                    sampler.raw,
                    pipeline.inner.sampler_map.get(unit as usize),
                    view.array_layers,
                    view.mip_levels,
                    gl.get_error(),
                ));
            }
            if !found {
                log(&format!(
                    "GLES_ATLAS_BIND no_active_sampler2DArray current={current:?} expected_program={program:?} error={:#x}",
                    gl.get_error()
                ));
            }
        }
    }

    /// Diagnostic readback of one RGBA8 array layer between submissions.
    pub fn diagnostic_atlas_layer(
        &self,
        texture: &super::Texture,
        layer: i32,
        bytes: &mut [u8],
    ) -> (u32, u32) {
        let gl = self.context().lock();
        unsafe {
            let (raw, _) = texture.inner.as_native();
            let previous = gl.get_parameter_framebuffer(glow::READ_FRAMEBUFFER_BINDING);
            let framebuffer = gl.create_framebuffer().expect("diagnostic framebuffer");
            gl.bind_framebuffer(glow::READ_FRAMEBUFFER, Some(framebuffer));
            gl.framebuffer_texture_layer(
                glow::READ_FRAMEBUFFER,
                glow::COLOR_ATTACHMENT0,
                Some(raw),
                0,
                layer,
            );
            let status = gl.check_framebuffer_status(glow::READ_FRAMEBUFFER);
            if status == glow::FRAMEBUFFER_COMPLETE {
                gl.read_pixels(
                    0,
                    0,
                    texture.copy_size.width as i32,
                    texture.copy_size.height as i32,
                    glow::RGBA,
                    glow::UNSIGNED_BYTE,
                    glow::PixelPackData::Slice(Some(bytes)),
                );
            }
            gl.bind_framebuffer(glow::READ_FRAMEBUFFER, previous);
            gl.delete_framebuffer(framebuffer);
            (status, gl.get_error())
        }
    }
}

#[derive(Debug)]
pub struct Swapchain {
    framebuffer: glow::Framebuffer,
    renderbuffer: glow::Renderbuffer,
    extent: wgt::Extent3d,
    format: wgt::TextureFormat,
    format_desc: super::TextureFormatDesc,
}

pub struct Surface {
    pub(super) presentable: bool,
    swap_fn: Option<Arc<dyn Fn() -> bool + Send + Sync>>,
    swapchain: RwLock<Option<Swapchain>>,
    present_probe: Mutex<Option<PresentProbe>>,
}

#[derive(Debug)]
enum PresentProbe {
    Requested([i32; 2]),
    Captured(FrameProbe),
}

#[path = "diagnostic_atlas.rs"]
mod diagnostic_atlas;
#[path = "diagnostic_readback.rs"]
mod diagnostic_readback;
use diagnostic_readback::{FrameProbe, sample_frame};

impl fmt::Debug for Surface {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Surface")
            .field("presentable", &self.presentable)
            .field("has_swap_fn", &self.swap_fn.is_some())
            .field("swapchain", &*self.swapchain.read())
            .finish()
    }
}

unsafe impl Send for Surface {}
unsafe impl Sync for Surface {}

impl Surface {
    /// Sample the rendered swapchain before the window blit.
    pub fn diagnostic_frame(
        &self,
        device: &super::Device,
        window: Option<[i32; 2]>,
    ) -> Option<FrameProbe> {
        let swapchain = self.swapchain.read();
        let sc = swapchain.as_ref()?;
        let gl = device.context().lock();
        *self.present_probe.lock() = window.map(PresentProbe::Requested);
        Some(sample_frame(
            &gl,
            Some(sc.framebuffer),
            [sc.extent.width as i32, sc.extent.height as i32],
        ))
    }

    pub fn take_diagnostic_present(&self) -> Option<FrameProbe> {
        match self.present_probe.lock().take()? {
            PresentProbe::Captured(frame) => Some(frame),
            PresentProbe::Requested(_) => None,
        }
    }

    /// Creates a `wgpu_hal::gles::Surface` backed by the currently bound EGL window surface (`framebuffer 0`).
    /// When `queue.present(&surface, texture)` is called, `Surface::present` blits the offscreen swapchain
    /// renderbuffer to framebuffer 0 (with Y-flip matching `wgpu-hal`'s `ADJUST_COORDINATE_SPACE`) and invokes
    /// `swap_fn` (`eglSwapBuffers`).
    pub unsafe fn new_external(swap_fn: impl Fn() -> bool + Send + Sync + 'static) -> Self {
        Self {
            presentable: true,
            swap_fn: Some(Arc::new(swap_fn)),
            swapchain: RwLock::new(None),
            present_probe: Mutex::new(None),
        }
    }

    pub fn supports_srgb(&self) -> bool {
        true
    }

    pub(super) unsafe fn present(
        &self,
        _suf_texture: super::Texture,
        context: &AdapterContext,
    ) -> Result<(), crate::SurfaceError> {
        let gl = unsafe { context.get_without_egl_lock() };
        let swapchain = self.swapchain.read();
        let sc = swapchain.as_ref().ok_or(crate::SurfaceError::Other(
            "Surface has no swap-chain configured",
        ))?;

        unsafe { gl.disable(glow::SCISSOR_TEST) };
        unsafe { gl.color_mask(true, true, true, true) };

        unsafe { gl.bind_framebuffer(glow::DRAW_FRAMEBUFFER, None) };
        unsafe { gl.bind_framebuffer(glow::READ_FRAMEBUFFER, Some(sc.framebuffer)) };

        // Note the Y-flipping here: wgpu-hal negates gl_Position.y in vertex shaders
        // (WriterFlags::ADJUST_COORDINATE_SPACE), so blitting from [height..0] to [0..height]
        // restores the upright image in OpenGL's bottom-left window coordinate space.
        unsafe {
            gl.blit_framebuffer(
                0,
                sc.extent.height as i32,
                sc.extent.width as i32,
                0,
                0,
                0,
                sc.extent.width as i32,
                sc.extent.height as i32,
                glow::COLOR_BUFFER_BIT,
                glow::NEAREST,
            )
        };

        unsafe { gl.bind_framebuffer(glow::READ_FRAMEBUFFER, None) };
        {
            let mut probe = self.present_probe.lock();
            if let Some(PresentProbe::Requested(size)) = probe.take() {
                *probe = Some(PresentProbe::Captured(sample_frame(&gl, None, size)));
            }
        }

        if let Some(ref swap_fn) = self.swap_fn {
            if !swap_fn() {
                return Err(crate::SurfaceError::Lost);
            }
        }

        Ok(())
    }

    unsafe fn unconfigure_impl(&self, device: &super::Device) {
        let gl = &device.shared.context.lock();
        if let Some(sc) = self.swapchain.write().take() {
            unsafe { gl.delete_renderbuffer(sc.renderbuffer) };
            unsafe { gl.delete_framebuffer(sc.framebuffer) };
        }
    }
}

impl crate::Surface for Surface {
    type A = super::Api;

    unsafe fn configure(
        &self,
        device: &super::Device,
        config: &crate::SurfaceConfiguration,
    ) -> Result<(), crate::SurfaceError> {
        unsafe { self.unconfigure_impl(device) };

        let format_desc = device.shared.describe_texture_format(config.format);
        let (renderbuffer, framebuffer) = {
            let gl = &device.shared.context.lock();
            let renderbuffer = unsafe { gl.create_renderbuffer() }.map_err(|error| {
                log::error!("Internal swapchain renderbuffer creation failed: {error}");
                crate::DeviceError::OutOfMemory
            })?;
            unsafe { gl.bind_renderbuffer(glow::RENDERBUFFER, Some(renderbuffer)) };
            unsafe {
                gl.renderbuffer_storage(
                    glow::RENDERBUFFER,
                    format_desc.internal,
                    config.extent.width as _,
                    config.extent.height as _,
                )
            };
            let framebuffer = unsafe { gl.create_framebuffer() }.map_err(|error| {
                log::error!("Internal swapchain framebuffer creation failed: {error}");
                crate::DeviceError::OutOfMemory
            })?;
            unsafe { gl.bind_framebuffer(glow::READ_FRAMEBUFFER, Some(framebuffer)) };
            unsafe {
                gl.framebuffer_renderbuffer(
                    glow::READ_FRAMEBUFFER,
                    glow::COLOR_ATTACHMENT0,
                    glow::RENDERBUFFER,
                    Some(renderbuffer),
                )
            };
            unsafe { gl.bind_renderbuffer(glow::RENDERBUFFER, None) };
            unsafe { gl.bind_framebuffer(glow::READ_FRAMEBUFFER, None) };
            (renderbuffer, framebuffer)
        };

        let mut swapchain = self.swapchain.write();
        *swapchain = Some(Swapchain {
            renderbuffer,
            framebuffer,
            extent: config.extent,
            format: config.format,
            format_desc,
        });

        Ok(())
    }

    unsafe fn unconfigure(&self, device: &super::Device) {
        unsafe { self.unconfigure_impl(device) };
    }

    unsafe fn acquire_texture(
        &self,
        _timeout_ms: Option<Duration>,
        _fence: &super::Fence,
    ) -> Result<crate::AcquiredSurfaceTexture<super::Api>, crate::SurfaceError> {
        let swapchain = self.swapchain.read();
        let sc = swapchain.as_ref().ok_or(crate::SurfaceError::Other(
            "Surface has no swap-chain configured",
        ))?;
        let texture = super::Texture {
            inner: super::TextureInner::Renderbuffer {
                raw: sc.renderbuffer,
            },
            drop_guard: None,
            array_layer_count: 1,
            mip_level_count: 1,
            format: sc.format,
            format_desc: sc.format_desc.clone(),
            copy_size: crate::CopyExtent {
                width: sc.extent.width,
                height: sc.extent.height,
                depth: 1,
            },
        };
        Ok(crate::AcquiredSurfaceTexture {
            texture,
            suboptimal: false,
        })
    }

    unsafe fn discard_texture(&self, _texture: super::Texture) {}
}
