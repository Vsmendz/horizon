use glow::HasContext;

#[derive(Debug)]
pub struct FrameProbe {
    pub size: [i32; 2],
    pub framebuffer: i32,
    pub attachment_status: u32,
    pub read_buffer: i32,
    pub prior_error: u32,
    pub read_error: u32,
    pub samples: [([i32; 2], [u8; 4]); 9],
}

pub fn sample_frame(
    gl: &glow::Context,
    framebuffer: Option<glow::Framebuffer>,
    size: [i32; 2],
) -> FrameProbe {
    unsafe {
        let prior_error = gl.get_error();
        let previous_fbo = gl.get_parameter_framebuffer(glow::READ_FRAMEBUFFER_BINDING);
        let previous_pack = gl.get_parameter_buffer(glow::PIXEL_PACK_BUFFER_BINDING);
        gl.bind_framebuffer(glow::READ_FRAMEBUFFER, framebuffer);
        let previous_read = gl.get_parameter_i32(glow::READ_BUFFER);
        let source = if framebuffer.is_some() {
            glow::COLOR_ATTACHMENT0
        } else {
            glow::BACK
        };
        gl.read_buffer(source);
        gl.bind_buffer(glow::PIXEL_PACK_BUFFER, None);
        let pack = [
            glow::PACK_ALIGNMENT,
            glow::PACK_ROW_LENGTH,
            glow::PACK_SKIP_ROWS,
            glow::PACK_SKIP_PIXELS,
        ];
        let old_pack = pack.map(|parameter| gl.get_parameter_i32(parameter));
        for (parameter, value) in pack.into_iter().zip([1, 0, 0, 0]) {
            gl.pixel_store_i32(parameter, value);
        }
        let status = gl.check_framebuffer_status(glow::READ_FRAMEBUFFER);
        let id = gl.get_parameter_i32(glow::READ_FRAMEBUFFER_BINDING);
        let read_buffer = gl.get_parameter_i32(glow::READ_BUFFER);
        gl.finish();
        // Coordinates are fractions from the top-left of the presented image.
        let samples = [
            (5, 5),
            (95, 95),
            (17, 33),
            (50, 33),
            (83, 33),
            (17, 67),
            (50, 67),
            (50, 50),
            (50, 94),
        ]
        .map(|(u, v)| {
            let [w, h] = size;
            let x = w * u / 100;
            let row = h * v / 100;
            let y = if framebuffer.is_some() {
                row
            } else {
                h - 1 - row
            };
            let mut rgba = [0; 4];
            if status == glow::FRAMEBUFFER_COMPLETE {
                gl.read_pixels(
                    x,
                    y,
                    1,
                    1,
                    glow::RGBA,
                    glow::UNSIGNED_BYTE,
                    glow::PixelPackData::Slice(Some(&mut rgba)),
                );
            }
            ([x, y], rgba)
        });
        let read_error = gl.get_error();
        for (parameter, value) in pack.into_iter().zip(old_pack) {
            gl.pixel_store_i32(parameter, value);
        }
        gl.bind_buffer(glow::PIXEL_PACK_BUFFER, previous_pack);
        gl.read_buffer(previous_read as u32);
        gl.bind_framebuffer(glow::READ_FRAMEBUFFER, previous_fbo);
        FrameProbe {
            size,
            framebuffer: id,
            attachment_status: status,
            read_buffer,
            prior_error,
            read_error,
            samples,
        }
    }
}
