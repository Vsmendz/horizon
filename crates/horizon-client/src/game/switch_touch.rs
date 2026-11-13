//! Turn the primary touchscreen contact into the original GUI pointer events.
#[derive(Default)]
pub(super) struct Touch(Option<u32>);

#[derive(Debug, PartialEq)]
enum Event {
    Move(f64, f64),
    Press,
    Release,
}

impl Touch {
    fn update(
        &mut self,
        sample: Option<(u32, u32, u32)>,
        size: [u32; 2],
        mut emit: impl FnMut(Event),
    ) {
        if self.0.is_some() && self.0 != sample.map(|s| s.0) {
            self.0 = None;
            emit(Event::Release);
        }
        if let Some((id, x, y)) = sample {
            let scale = |value: u32, source: u32, target: u32| {
                f64::from(value.min(source - 1)) * f64::from(target.saturating_sub(1))
                    / f64::from(source - 1)
            };
            emit(Event::Move(scale(x, 1280, size[0]), scale(y, 720, size[1])));
            if self.0.replace(id).is_none() {
                emit(Event::Press);
            }
        }
    }

    #[cfg(target_os = "horizon")]
    pub(super) fn poll(
        &mut self,
        enabled: bool,
        size: [u32; 2],
        mut emit: impl FnMut(winit::event::WindowEvent),
    ) {
        use winit::{
            dpi::PhysicalPosition,
            event::{DeviceId, ElementState, MouseButton, WindowEvent},
        };
        let sample = if enabled {
            horizon_nx::splash::touch_sample()
        } else {
            None
        };
        // A closed GUI cannot receive a release: its mouse-up handler may now be
        // gameplay. Clear ownership before allowing another touch to act.
        if !enabled {
            self.0 = None;
            return;
        }
        let device_id = DeviceId::dummy();
        self.update(sample, size, |event| {
            emit(match event {
                Event::Move(x, y) => WindowEvent::CursorMoved {
                    device_id,
                    position: PhysicalPosition::new(x, y),
                },
                Event::Press => WindowEvent::MouseInput {
                    device_id,
                    state: ElementState::Pressed,
                    button: MouseButton::Left,
                },
                Event::Release => WindowEvent::MouseInput {
                    device_id,
                    state: ElementState::Released,
                    button: MouseButton::Left,
                },
            })
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scales_clamps_and_preserves_drag_and_release_order() {
        let mut touch = Touch::default();
        let mut events = Vec::new();
        touch.update(Some((1, 1279, 900)), [640, 360], |e| events.push(e));
        touch.update(Some((1, 0, 0)), [640, 360], |e| events.push(e));
        touch.update(None, [640, 360], |e| events.push(e));
        assert_eq!(
            events,
            [
                Event::Move(639., 359.),
                Event::Press,
                Event::Move(0., 0.),
                Event::Release
            ]
        );
    }

    #[test]
    fn changing_fingers_ends_the_previous_contact() {
        let mut touch = Touch::default();
        touch.update(Some((1, 0, 0)), [1280, 720], |_| {});
        let mut events = Vec::new();
        touch.update(Some((2, 1279, 719)), [1280, 720], |e| events.push(e));
        assert_eq!(
            events,
            [Event::Release, Event::Move(1279., 719.), Event::Press]
        );
    }
}
