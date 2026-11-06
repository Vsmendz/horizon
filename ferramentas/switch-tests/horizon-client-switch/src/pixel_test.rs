use super::{CameraUniform, HORIZON_WGSL_SHADER, TerrainVertex};
use glow::HasContext;
use khronos_egl as egl;
use naga::back::glsl;

#[test]
fn gles_offscreen_pixels() {
    unsafe { run() }
}

unsafe fn run() {
    let egl = unsafe {
        egl::DynamicInstance::<egl::EGL1_4>::load_required_from_filename("libEGL.so.1").unwrap()
    };
    let display = unsafe { egl.get_display(egl::DEFAULT_DISPLAY) }.unwrap();
    egl.initialize(display).unwrap();
    egl.bind_api(egl::OPENGL_ES_API).unwrap();
    let config = egl
        .choose_first_config(
            display,
            &[
                egl::SURFACE_TYPE,
                egl::PBUFFER_BIT,
                egl::RENDERABLE_TYPE,
                egl::OPENGL_ES3_BIT,
                egl::RED_SIZE,
                8,
                egl::GREEN_SIZE,
                8,
                egl::BLUE_SIZE,
                8,
                egl::ALPHA_SIZE,
                8,
                egl::DEPTH_SIZE,
                24,
                egl::NONE,
            ],
        )
        .unwrap()
        .unwrap();
    let surface = egl
        .create_pbuffer_surface(
            display,
            config,
            &[egl::WIDTH, 64, egl::HEIGHT, 64, egl::NONE],
        )
        .unwrap();
    let context = egl
        .create_context(
            display,
            config,
            None,
            &[egl::CONTEXT_CLIENT_VERSION, 3, egl::NONE],
        )
        .unwrap();
    egl.make_current(display, Some(surface), Some(surface), Some(context))
        .unwrap();
    unsafe {
        let gl = glow::Context::from_loader_function(|name| {
            egl.get_proc_address(name)
                .map_or(std::ptr::null(), |ptr| ptr as *const _)
        });
        println!(
            "GLES {} | {}",
            gl.get_parameter_string(glow::VERSION),
            gl.get_parameter_string(glow::RENDERER)
        );
        verify_pixels(&gl);
    }
    egl.make_current(display, None, None, None).unwrap();
    egl.destroy_context(display, context).unwrap();
    egl.destroy_surface(display, surface).unwrap();
    egl.terminate(display).unwrap();
}

unsafe fn verify_pixels(gl: &glow::Context) {
    unsafe {
        let vao = gl.create_vertex_array().unwrap();
        gl.bind_vertex_array(Some(vao));
        let vertices = gl.create_buffer().unwrap();
        gl.bind_buffer(glow::ARRAY_BUFFER, Some(vertices));
        for (location, size, offset) in [(0, 3, 0), (1, 3, 12), (2, 3, 24), (3, 4, 36)] {
            gl.enable_vertex_attrib_array(location);
            gl.vertex_attrib_pointer_f32(
                location,
                size,
                glow::FLOAT,
                false,
                size_of::<TerrainVertex>() as i32,
                offset,
            );
        }
        let identity = glam::Mat4::IDENTITY.to_cols_array_2d();
        let camera = CameraUniform {
            view_proj: identity,
            hand_projection: identity,
            sky_color: [0.0, 0.0, 1.0, 1.0],
            sun_dir: [0.0, 0.0, 1.0, 0.0],
            camera_pos: [0.0, 0.0, 0.0, 1.0],
        };
        let uniforms = gl.create_buffer().unwrap();
        gl.bind_buffer(glow::UNIFORM_BUFFER, Some(uniforms));
        gl.buffer_data_u8_slice(
            glow::UNIFORM_BUFFER,
            bytemuck::bytes_of(&camera),
            glow::STATIC_DRAW,
        );
        gl.bind_buffer_base(glow::UNIFORM_BUFFER, 0, Some(uniforms));
        let texture = gl.create_texture().unwrap();
        gl.active_texture(glow::TEXTURE0);
        gl.bind_texture(glow::TEXTURE_2D_ARRAY, Some(texture));
        gl.tex_storage_3d(glow::TEXTURE_2D_ARRAY, 1, glow::RGBA8, 16, 16, 42);
        gl.tex_parameter_i32(glow::TEXTURE_2D_ARRAY, glow::TEXTURE_MAX_LEVEL, 0);
        let sampler = gl.create_sampler().unwrap();
        gl.sampler_parameter_i32(
            sampler,
            glow::TEXTURE_MIN_FILTER,
            glow::NEAREST_MIPMAP_NEAREST as i32,
        );
        gl.sampler_parameter_i32(sampler, glow::TEXTURE_MAG_FILTER, glow::NEAREST as i32);
        gl.bind_sampler(0, Some(sampler));
        let mut layers = vec![0; 42 * 1024];
        for (i, texel) in layers[40 * 1024..41 * 1024].chunks_exact_mut(4).enumerate() {
            texel.copy_from_slice(if i % 16 < 8 {
                &[200, 100, 40, 255]
            } else {
                &[30, 80, 150, 255]
            });
        }
        layers[41 * 1024..].fill(255);
        let unpack = gl.create_buffer().unwrap();
        gl.bind_buffer(glow::PIXEL_UNPACK_BUFFER, Some(unpack));
        gl.buffer_data_size(
            glow::PIXEL_UNPACK_BUFFER,
            layers.len() as i32,
            glow::STREAM_DRAW,
        );
        let mapped = gl.map_buffer_range(
            glow::PIXEL_UNPACK_BUFFER,
            0,
            layers.len() as i32,
            glow::MAP_WRITE_BIT | glow::MAP_FLUSH_EXPLICIT_BIT,
        );
        assert!(!mapped.is_null());
        std::ptr::copy_nonoverlapping(layers.as_ptr(), mapped, layers.len());
        gl.flush_mapped_buffer_range(glow::PIXEL_UNPACK_BUFFER, 0, layers.len() as i32);
        gl.unmap_buffer(glow::PIXEL_UNPACK_BUFFER);
        for layer in 0..42 {
            gl.tex_sub_image_3d(
                glow::TEXTURE_2D_ARRAY,
                0,
                0,
                0,
                layer,
                16,
                16,
                1,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelUnpackData::BufferOffset(layer as u32 * 1024),
            );
        }
        gl.bind_buffer(glow::PIXEL_UNPACK_BUFFER, None);
        gl.viewport(0, 0, 64, 64);
        gl.enable(glow::DEPTH_TEST);
        gl.depth_func(glow::LESS);
        gl.enable(glow::CULL_FACE);
        gl.front_face(glow::CW); // Naga's GLES conversion flips clip-space Y.
        gl.clear_color(0.0, 0.0, 1.0, 1.0);
        assert_eq!(
            gl.get_error(),
            glow::NO_ERROR,
            "texture upload and draw setup"
        );

        let flat = HORIZON_WGSL_SHADER.replace(
            "let tex = textureSample(terrain_tex, terrain_sampler, in.uv_layer.xy, layer_idx);",
            "let tex = vec4<f32>(1.0);",
        );
        let unlit = HORIZON_WGSL_SHADER.replace(
            "let light = clamp((0.45 + 0.55 * ndotl) * face_shade, 0.25, 1.20);",
            "let light = 1.0;",
        );
        for (label, source, entry, layer, shade, expected) in [
            (
                "flat geometry",
                flat.as_str(),
                "fs_main",
                0.0,
                1.0,
                [32, 192, 64, 255],
            ),
            (
                "unlit atlas",
                unlit.as_str(),
                "fs_main",
                40.0,
                0.5,
                [200, 100, 40, 255],
            ),
            (
                "lit terrain",
                HORIZON_WGSL_SHADER,
                "fs_main",
                40.0,
                0.5,
                [100, 50, 20, 255],
            ),
            (
                "HUD white layer",
                HORIZON_WGSL_SHADER,
                "fs_main",
                41.0,
                1.0,
                [255, 255, 255, 255],
            ),
        ] {
            let program = program(gl, source, entry);
            gl.use_program(Some(program));
            gl.clear(glow::COLOR_BUFFER_BIT | glow::DEPTH_BUFFER_BIT);
            let geometry =
                [[-0.75, -0.75], [0.75, -0.75], [0.0, 0.75]].map(|[x, y]| TerrainVertex {
                    pos: [x, y, 0.2],
                    normal: [0.0, 0.0, 1.0],
                    uv_layer: [0.25, 0.75, layer],
                    color_ao: if label == "flat geometry" {
                        [32.0 / 255.0, 192.0 / 255.0, 64.0 / 255.0, shade]
                    } else {
                        [1.0, 1.0, 1.0, shade]
                    },
                });
            gl.buffer_data_u8_slice(
                glow::ARRAY_BUFFER,
                bytemuck::cast_slice(&geometry),
                glow::STREAM_DRAW,
            );
            gl.draw_arrays(glow::TRIANGLES, 0, 3);
            assert_eq!(pixel(gl, 32, 32), expected, "{label}");
            assert_eq!(pixel(gl, 1, 1), [0, 0, 255, 255], "{label} background");
            if label == "lit terrain" {
                let behind = geometry.map(|mut vertex| {
                    vertex.pos[2] = 0.7;
                    vertex.uv_layer[2] = 41.0;
                    vertex.color_ao[3] = 1.0;
                    vertex
                });
                gl.buffer_data_u8_slice(
                    glow::ARRAY_BUFFER,
                    bytemuck::cast_slice(&behind),
                    glow::STREAM_DRAW,
                );
                gl.draw_arrays(glow::TRIANGLES, 0, 3);
                assert_eq!(
                    pixel(gl, 32, 32),
                    expected,
                    "depth rejects occluded geometry"
                );
                gl.clear(glow::DEPTH_BUFFER_BIT);
                let reversed = [behind[2], behind[1], behind[0]];
                gl.buffer_data_u8_slice(
                    glow::ARRAY_BUFFER,
                    bytemuck::cast_slice(&reversed),
                    glow::STREAM_DRAW,
                );
                gl.draw_arrays(glow::TRIANGLES, 0, 3);
                assert_eq!(
                    pixel(gl, 32, 32),
                    expected,
                    "back face rejected in overlay pass"
                );
                gl.buffer_data_u8_slice(
                    glow::ARRAY_BUFFER,
                    bytemuck::cast_slice(&behind),
                    glow::STREAM_DRAW,
                );
                gl.draw_arrays(glow::TRIANGLES, 0, 3);
                assert_eq!(
                    pixel(gl, 32, 32),
                    [255; 4],
                    "overlay drawn after depth clear"
                );
                assert_eq!(
                    pixel(gl, 1, 1),
                    [0, 0, 255, 255],
                    "overlay preserved clear color"
                );
            }
            assert_eq!(gl.get_error(), glow::NO_ERROR, "{label}");
            println!("{label}: {expected:?}");
            gl.delete_program(program);
        }
    }
}

unsafe fn pixel(gl: &glow::Context, x: i32, y: i32) -> [u8; 4] {
    let mut rgba = [0; 4];
    unsafe {
        gl.read_pixels(
            x,
            y,
            1,
            1,
            glow::RGBA,
            glow::UNSIGNED_BYTE,
            glow::PixelPackData::Slice(Some(&mut rgba)),
        )
    };
    rgba
}

unsafe fn program(gl: &glow::Context, wgsl: &str, fragment: &str) -> glow::Program {
    let module = naga::front::wgsl::parse_str(wgsl).unwrap();
    let info = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::empty(),
    )
    .validate(&module)
    .unwrap();
    let options = glsl::Options {
        version: glsl::Version::new_gles(320),
        ..Default::default()
    };
    unsafe {
        let program = gl.create_program().unwrap();
        let mut texture_names = Vec::new();
        for (stage, entry, kind) in [
            (naga::ShaderStage::Vertex, "vs_main", glow::VERTEX_SHADER),
            (naga::ShaderStage::Fragment, fragment, glow::FRAGMENT_SHADER),
        ] {
            let pipeline = glsl::PipelineOptions {
                shader_stage: stage,
                entry_point: entry.into(),
                multiview: None,
            };
            let mut source = String::new();
            let reflection = glsl::Writer::new(
                &mut source,
                &module,
                &info,
                &options,
                &pipeline,
                Default::default(),
            )
            .unwrap()
            .write()
            .unwrap();
            texture_names.extend(reflection.texture_mapping.into_keys());
            let shader = gl.create_shader(kind).unwrap();
            gl.shader_source(shader, &source);
            gl.compile_shader(shader);
            assert!(
                gl.get_shader_compile_status(shader),
                "{}",
                gl.get_shader_info_log(shader)
            );
            gl.attach_shader(program, shader);
            gl.delete_shader(shader);
        }
        gl.link_program(program);
        assert!(
            gl.get_program_link_status(program),
            "{}",
            gl.get_program_info_log(program)
        );
        gl.use_program(Some(program));
        for block in 0..gl.get_program_parameter_i32(program, glow::ACTIVE_UNIFORM_BLOCKS) {
            gl.uniform_block_binding(program, block as u32, 0);
        }
        for name in texture_names {
            gl.uniform_1_i32(gl.get_uniform_location(program, &name).as_ref(), 0);
        }
        program
    }
}
