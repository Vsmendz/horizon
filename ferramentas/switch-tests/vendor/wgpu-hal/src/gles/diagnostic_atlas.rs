extern crate alloc;

use alloc::{format, string::String, vec::Vec};
use glow::HasContext;

const VERTEX: &str = "#version 300 es
void main() {
    vec2 uv = vec2(float((gl_VertexID << 1) & 2), float(gl_VertexID & 2));
    gl_Position = vec4(uv * 2.0 - 1.0, 0.0, 1.0);
}";

fn program(gl: &glow::Context, array: bool) -> Result<glow::Program, String> {
    let (sampler, fetch, uv) = if array {
        (
            "sampler2DArray",
            "ivec3(0, 0, layer)",
            "vec3(vec2(0.5) / vec2(textureSize(atlas, 0).xy), float(layer))",
        )
    } else {
        (
            "sampler2D",
            "ivec2(0)",
            "vec2(0.5) / vec2(textureSize(atlas, 0))",
        )
    };
    let fragment = format!(
        "#version 300 es
uniform highp {sampler} atlas;
uniform highp int layer;
uniform highp int fetch;
out highp vec4 pixel;
void main() {{
    pixel = fetch != 0 ? texelFetch(atlas, {fetch}, 0) : texture(atlas, {uv});
}}"
    );
    unsafe {
        let program = gl.create_program()?;
        let result = (|| {
            for (kind, source) in [
                (glow::VERTEX_SHADER, VERTEX),
                (glow::FRAGMENT_SHADER, &fragment),
            ] {
                let shader = gl.create_shader(kind)?;
                gl.shader_source(shader, source);
                gl.compile_shader(shader);
                let compiled = gl.get_shader_compile_status(shader);
                if compiled {
                    gl.attach_shader(program, shader);
                }
                let info = gl.get_shader_info_log(shader);
                gl.delete_shader(shader);
                if !compiled {
                    return Err(info);
                }
            }
            gl.link_program(program);
            if gl.get_program_link_status(program) {
                Ok(program)
            } else {
                Err(gl.get_program_info_log(program))
            }
        })();
        if result.is_err() {
            gl.delete_program(program);
        }
        result
    }
}

pub fn probe(
    gl: &glow::Context,
    atlas: (glow::Texture, u32),
    sampler: glow::Sampler,
    layers: &[[u8; 4]],
    log: &mut dyn FnMut(&str),
) -> Result<usize, String> {
    let array_program = program(gl, true)?;
    let plain_program = match program(gl, false) {
        Ok(program) => program,
        Err(error) => {
            unsafe { gl.delete_program(array_program) };
            return Err(error);
        }
    };
    let mut failures = 0;
    unsafe {
        // Save the bindings and pixel transfer state touched by the probe
        // so the HAL sees its own state on the next submission.
        let error = gl.get_error();
        let version = gl.get_parameter_string(glow::VERSION);
        let glsl = gl.get_parameter_string(glow::SHADING_LANGUAGE_VERSION);
        let max_layers = gl.get_parameter_i32(glow::MAX_ARRAY_TEXTURE_LAYERS);
        log(&format!(
            "GLES_ATLAS_GL version={version:?} glsl={glsl:?} max_layers={max_layers} prior_error={error:#x}"
        ));
        let read = gl.get_parameter_framebuffer(glow::READ_FRAMEBUFFER_BINDING);
        let draw = gl.get_parameter_framebuffer(glow::DRAW_FRAMEBUFFER_BINDING);
        let renderbuffer = gl.get_parameter_renderbuffer(glow::RENDERBUFFER_BINDING);
        let vao = gl.get_parameter_vertex_array(glow::VERTEX_ARRAY_BINDING);
        let current_program = gl.get_parameter_program(glow::CURRENT_PROGRAM);
        let mut viewport = [0; 4];
        gl.get_parameter_i32_slice(glow::VIEWPORT, &mut viewport);
        let active = gl.get_parameter_i32(glow::ACTIVE_TEXTURE) as u32;
        let bindings = [0, 1].map(|unit| {
            gl.active_texture(glow::TEXTURE0 + unit);
            (
                gl.get_parameter_texture(glow::TEXTURE_BINDING_2D),
                gl.get_parameter_texture(glow::TEXTURE_BINDING_2D_ARRAY),
                gl.get_parameter_sampler(glow::SAMPLER_BINDING),
            )
        });
        let pack = gl.get_parameter_buffer(glow::PIXEL_PACK_BUFFER_BINDING);
        let unpack = gl.get_parameter_buffer(glow::PIXEL_UNPACK_BUFFER_BINDING);
        gl.bind_buffer(glow::PIXEL_PACK_BUFFER, None);
        gl.bind_buffer(glow::PIXEL_UNPACK_BUFFER, None);
        let pixel_store = [
            (glow::PACK_ALIGNMENT, 1),
            (glow::PACK_ROW_LENGTH, 0),
            (glow::PACK_SKIP_ROWS, 0),
            (glow::PACK_SKIP_PIXELS, 0),
            (glow::UNPACK_ALIGNMENT, 1),
            (glow::UNPACK_ROW_LENGTH, 0),
            (glow::UNPACK_IMAGE_HEIGHT, 0),
            (glow::UNPACK_SKIP_IMAGES, 0),
            (glow::UNPACK_SKIP_ROWS, 0),
            (glow::UNPACK_SKIP_PIXELS, 0),
        ];
        let saved_pixel_store = pixel_store.map(|(parameter, value)| {
            let old = gl.get_parameter_i32(parameter);
            gl.pixel_store_i32(parameter, value);
            (parameter, old)
        });
        let enables = [
            glow::DEPTH_TEST,
            glow::STENCIL_TEST,
            glow::SCISSOR_TEST,
            glow::CULL_FACE,
            glow::BLEND,
        ];
        let saved_enables = enables.map(|cap| {
            let enabled = gl.is_enabled(cap);
            gl.disable(cap);
            (cap, enabled)
        });
        let out = gl.create_renderbuffer().expect("probe renderbuffer");
        let framebuffer = gl.create_framebuffer().expect("probe framebuffer");
        let empty_vao = gl.create_vertex_array().expect("probe vertex array");
        gl.bind_vertex_array(Some(empty_vao));
        gl.bind_renderbuffer(glow::RENDERBUFFER, Some(out));
        gl.renderbuffer_storage(glow::RENDERBUFFER, glow::RGBA8, 1, 1);
        gl.bind_framebuffer(glow::FRAMEBUFFER, Some(framebuffer));
        gl.framebuffer_renderbuffer(
            glow::FRAMEBUFFER,
            glow::COLOR_ATTACHMENT0,
            glow::RENDERBUFFER,
            Some(out),
        );
        let status = gl.check_framebuffer_status(glow::FRAMEBUFFER);
        gl.viewport(0, 0, 1, 1);
        let setup_error = gl.get_error();
        failures += usize::from(status != glow::FRAMEBUFFER_COMPLETE || setup_error != 0);
        log(&format!(
            "GLES_ATLAS_NATIVE_FBO status={status:#x} error={setup_error:#x}"
        ));

        // The production array stays untouched. Each control texture has a
        // distinct known color per layer and uses the same dimensions and format.
        let targets = [
            ("client", glow::TEXTURE_2D_ARRAY, layers.len()),
            ("2d", glow::TEXTURE_2D, 1),
            ("array15", glow::TEXTURE_2D_ARRAY, 15),
            ("array42", glow::TEXTURE_2D_ARRAY, 42),
            ("array42_mutable", glow::TEXTURE_2D_ARRAY, 42),
        ];
        for (name, target, depth) in targets {
            let owned = name != "client";
            if !owned && atlas.1 != target {
                log(&format!(
                    "GLES_ATLAS_NATIVE name=client skipped_target_mismatch actual={:#x} required={target:#x}",
                    atlas.1
                ));
                failures += 1;
                continue;
            }
            let texture = if owned {
                gl.create_texture().expect("probe texture")
            } else {
                atlas.0
            };
            let control_color =
                |layer: usize| [16 + layer as u8 * 5, 180 - layer as u8 * 3, 64, 255];
            gl.active_texture(glow::TEXTURE0);
            gl.bind_texture(target, Some(texture));
            let bind_error = gl.get_error();
            if bind_error != 0 {
                log(&format!(
                    "GLES_ATLAS_NATIVE name={name} bind_error={bind_error:#x}"
                ));
                failures += 1;
                if owned {
                    gl.delete_texture(texture);
                }
                continue;
            }
            if owned {
                gl.tex_parameter_i32(target, glow::TEXTURE_BASE_LEVEL, 0);
                gl.tex_parameter_i32(target, glow::TEXTURE_MAX_LEVEL, 0);
                gl.tex_parameter_i32(target, glow::TEXTURE_MIN_FILTER, glow::NEAREST as i32);
                gl.tex_parameter_i32(target, glow::TEXTURE_MAG_FILTER, glow::NEAREST as i32);
                let bytes: Vec<_> = (0..depth)
                    .flat_map(|layer| control_color(layer).repeat(16 * 16))
                    .collect();
                let pixels = glow::PixelUnpackData::Slice(Some(&bytes));
                if target == glow::TEXTURE_2D {
                    gl.tex_image_2d(
                        target,
                        0,
                        glow::RGBA8 as i32,
                        16,
                        16,
                        0,
                        glow::RGBA,
                        glow::UNSIGNED_BYTE,
                        pixels,
                    );
                } else if name == "array42_mutable" {
                    gl.tex_image_3d(
                        target,
                        0,
                        glow::RGBA8 as i32,
                        16,
                        16,
                        depth as i32,
                        0,
                        glow::RGBA,
                        glow::UNSIGNED_BYTE,
                        pixels,
                    );
                } else {
                    gl.tex_storage_3d(target, 1, glow::RGBA8, 16, 16, depth as i32);
                    gl.tex_sub_image_3d(
                        target,
                        0,
                        0,
                        0,
                        0,
                        16,
                        16,
                        depth as i32,
                        glow::RGBA,
                        glow::UNSIGNED_BYTE,
                        pixels,
                    );
                }
            }
            let storage_error = gl.get_error();
            let program = if target == glow::TEXTURE_2D {
                plain_program
            } else {
                array_program
            };
            gl.use_program(Some(program));
            let atlas_uniform = gl.get_uniform_location(program, "atlas");
            let layer_uniform = gl.get_uniform_location(program, "layer");
            let fetch_uniform = gl.get_uniform_location(program, "fetch");
            let test_layers =
                (0..depth).filter(|&i| matches!(i, 0 | 14 | 15 | 31 | 32) || i + 1 == depth);
            for (unit, sampler_object) in [(0, None), (0, Some(sampler)), (1, Some(sampler))] {
                gl.active_texture(glow::TEXTURE0 + unit);
                gl.bind_texture(target, Some(texture));
                gl.bind_sampler(unit, sampler_object);
                gl.uniform_1_i32(atlas_uniform.as_ref(), unit as i32);
                let mut checks = Vec::new();
                for layer in test_layers.clone() {
                    let expected: [u8; 4] = if owned {
                        control_color(layer)
                    } else {
                        layers[layer]
                    };
                    gl.uniform_1_i32(layer_uniform.as_ref(), layer as i32);
                    let [fetch, sample] = [1, 0].map(|mode| {
                        gl.uniform_1_i32(fetch_uniform.as_ref(), mode);
                        gl.clear_buffer_f32_slice(glow::COLOR, 0, &[1.0, 0.0, 1.0, 1.0]);
                        gl.draw_arrays(glow::TRIANGLES, 0, 3);
                        let mut pixel = [0; 4];
                        gl.read_pixels(
                            0,
                            0,
                            1,
                            1,
                            glow::RGBA,
                            glow::UNSIGNED_BYTE,
                            glow::PixelPackData::Slice(Some(&mut pixel)),
                        );
                        pixel
                    });
                    checks.push((layer, expected, fetch, sample));
                    failures += usize::from(fetch != expected || sample != expected);
                }
                let draw_error = gl.get_error();
                failures += usize::from(storage_error != 0 || draw_error != 0);
                log(&format!(
                    "GLES_ATLAS_NATIVE name={name} depth={depth} unit={unit} sampler_object={} storage_error={storage_error:#x} draw_error={draw_error:#x} checks(layer,expected,fetch,sample)={checks:?}",
                    sampler_object.is_some(),
                ));
            }
            if owned {
                gl.delete_texture(texture);
            }
        }
        gl.use_program(current_program);
        gl.bind_vertex_array(vao);
        gl.bind_framebuffer(glow::READ_FRAMEBUFFER, read);
        gl.bind_framebuffer(glow::DRAW_FRAMEBUFFER, draw);
        gl.bind_renderbuffer(glow::RENDERBUFFER, renderbuffer);
        gl.viewport(viewport[0], viewport[1], viewport[2], viewport[3]);
        for (cap, enabled) in saved_enables {
            if enabled {
                gl.enable(cap);
            }
        }
        for (parameter, value) in saved_pixel_store {
            gl.pixel_store_i32(parameter, value);
        }
        gl.bind_buffer(glow::PIXEL_PACK_BUFFER, pack);
        gl.bind_buffer(glow::PIXEL_UNPACK_BUFFER, unpack);
        for (unit, (plain, array, sampler)) in bindings.into_iter().enumerate() {
            gl.active_texture(glow::TEXTURE0 + unit as u32);
            gl.bind_texture(glow::TEXTURE_2D, plain);
            gl.bind_texture(glow::TEXTURE_2D_ARRAY, array);
            gl.bind_sampler(unit as u32, sampler);
        }
        gl.active_texture(active);
        gl.delete_vertex_array(empty_vao);
        gl.delete_framebuffer(framebuffer);
        gl.delete_renderbuffer(out);
        gl.delete_program(array_program);
        gl.delete_program(plain_program);
        let error = gl.get_error();
        failures += usize::from(error != 0);
        log(&format!(
            "GLES_ATLAS_NATIVE_RESTORE error={error:#x} failures={failures}"
        ));
    }
    Ok(failures)
}
