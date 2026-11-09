#![cfg(target_os = "horizon")]

use std::cell::Cell;
use std::collections::VecDeque;
use std::fmt::{self, Display, Formatter};
use std::marker::PhantomData;
use std::sync::mpsc;

use smol_str::SmolStr;

use crate::cursor::Cursor;
use crate::dpi::{PhysicalPosition, PhysicalSize, Position, Size};
use crate::error::{self, EventLoopError};
use crate::event::{self, StartCause};
use crate::event_loop::{self, ControlFlow, DeviceEvents};
use crate::keyboard::Key;
use crate::platform_impl::Fullscreen;
use crate::window::{
    self, CustomCursor as RootCustomCursor, CustomCursorSource, ImePurpose, Theme,
};

#[derive(Default, Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub(crate) struct PlatformSpecificEventLoopAttributes {}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct WindowId(u64);

impl WindowId {
    pub const fn dummy() -> Self {
        WindowId(u64::MAX)
    }
}

impl From<WindowId> for u64 {
    fn from(id: WindowId) -> Self {
        id.0
    }
}

impl From<u64> for WindowId {
    fn from(id: u64) -> Self {
        Self(id)
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct DeviceId;

impl DeviceId {
    pub const fn dummy() -> Self {
        DeviceId
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PlatformSpecificWindowAttributes;

#[derive(Clone, Debug)]
pub struct OsError(&'static str);

impl Display for OsError {
    fn fmt(&self, fmt: &mut Formatter<'_>) -> Result<(), fmt::Error> {
        fmt.write_str(self.0)
    }
}

pub(crate) use crate::cursor::{
    NoCustomCursor as PlatformCustomCursor, NoCustomCursor as PlatformCustomCursorSource,
};
pub(crate) use crate::icon::NoIcon as PlatformIcon;

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct MonitorHandle;

impl MonitorHandle {
    pub fn name(&self) -> Option<String> {
        Some("Nintendo Switch Display".to_owned())
    }

    pub fn size(&self) -> PhysicalSize<u32> {
        PhysicalSize::new(1280, 720)
    }

    pub fn position(&self) -> PhysicalPosition<i32> {
        PhysicalPosition::new(0, 0)
    }

    pub fn scale_factor(&self) -> f64 {
        1.0
    }

    pub fn refresh_rate_millihertz(&self) -> Option<u32> {
        Some(60_000)
    }

    pub fn video_modes(&self) -> impl Iterator<Item = VideoModeHandle> {
        std::iter::once(VideoModeHandle {
            size: (1280, 720),
            bit_depth: 24,
            refresh_rate_millihertz: 60_000,
            monitor: self.clone(),
        })
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct VideoModeHandle {
    size: (u32, u32),
    bit_depth: u16,
    refresh_rate_millihertz: u32,
    monitor: MonitorHandle,
}

impl VideoModeHandle {
    pub fn size(&self) -> PhysicalSize<u32> {
        self.size.into()
    }

    pub fn bit_depth(&self) -> u16 {
        self.bit_depth
    }

    pub fn refresh_rate_millihertz(&self) -> u32 {
        self.refresh_rate_millihertz
    }

    pub fn monitor(&self) -> MonitorHandle {
        self.monitor.clone()
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct KeyEventExtra {
    pub key_without_modifiers: Key,
    pub text_with_all_modifiers: Option<SmolStr>,
}

pub struct Window {
    id: WindowId,
}

impl Window {
    pub(crate) fn new(
        _el: &ActiveEventLoop,
        _attrs: window::WindowAttributes,
    ) -> Result<Self, error::OsError> {
        Ok(Self { id: WindowId(1) })
    }

    pub(crate) fn maybe_queue_on_main(&self, f: impl FnOnce(&Self) + Send + 'static) {
        f(self)
    }

    pub(crate) fn maybe_wait_on_main<R: Send>(&self, f: impl FnOnce(&Self) -> R + Send) -> R {
        f(self)
    }

    #[inline]
    pub fn id(&self) -> WindowId {
        self.id
    }

    #[inline]
    pub fn primary_monitor(&self) -> Option<MonitorHandle> {
        Some(MonitorHandle)
    }

    #[inline]
    pub fn available_monitors(&self) -> VecDeque<MonitorHandle> {
        let mut v = VecDeque::with_capacity(1);
        v.push_back(MonitorHandle);
        v
    }

    #[inline]
    pub fn current_monitor(&self) -> Option<MonitorHandle> {
        Some(MonitorHandle)
    }

    #[inline]
    pub fn scale_factor(&self) -> f64 {
        1.0
    }

    #[inline]
    pub fn request_redraw(&self) {}

    #[inline]
    pub fn pre_present_notify(&self) {}

    #[inline]
    pub fn reset_dead_keys(&self) {}

    #[inline]
    pub fn inner_position(&self) -> Result<PhysicalPosition<i32>, error::NotSupportedError> {
        Ok(PhysicalPosition::new(0, 0))
    }

    #[inline]
    pub fn outer_position(&self) -> Result<PhysicalPosition<i32>, error::NotSupportedError> {
        Ok(PhysicalPosition::new(0, 0))
    }

    #[inline]
    pub fn set_outer_position(&self, _position: Position) {}

    #[inline]
    pub fn inner_size(&self) -> PhysicalSize<u32> {
        PhysicalSize::new(1280, 720)
    }

    #[inline]
    pub fn request_inner_size(&self, _size: Size) -> Option<PhysicalSize<u32>> {
        Some(PhysicalSize::new(1280, 720))
    }

    #[inline]
    pub fn outer_size(&self) -> PhysicalSize<u32> {
        PhysicalSize::new(1280, 720)
    }

    #[inline]
    pub fn set_min_inner_size(&self, _: Option<Size>) {}

    #[inline]
    pub fn set_max_inner_size(&self, _: Option<Size>) {}

    #[inline]
    pub fn title(&self) -> String {
        String::from("Horizon")
    }

    #[inline]
    pub fn set_title(&self, _title: &str) {}

    #[inline]
    pub fn set_transparent(&self, _transparent: bool) {}

    #[inline]
    pub fn set_blur(&self, _blur: bool) {}

    #[inline]
    pub fn set_visible(&self, _visible: bool) {}

    #[inline]
    pub fn is_visible(&self) -> Option<bool> {
        Some(true)
    }

    #[inline]
    pub fn resize_increments(&self) -> Option<PhysicalSize<u32>> {
        None
    }

    #[inline]
    pub fn set_resize_increments(&self, _increments: Option<Size>) {}

    #[inline]
    pub fn set_resizable(&self, _resizable: bool) {}

    #[inline]
    pub fn is_resizable(&self) -> bool {
        false
    }

    #[inline]
    pub fn set_minimized(&self, _minimized: bool) {}

    #[inline]
    pub fn is_minimized(&self) -> Option<bool> {
        Some(false)
    }

    #[inline]
    pub fn set_maximized(&self, _maximized: bool) {}

    #[inline]
    pub fn is_maximized(&self) -> bool {
        true
    }

    #[inline]
    pub(crate) fn set_fullscreen(&self, _monitor: Option<Fullscreen>) {}

    #[inline]
    pub(crate) fn fullscreen(&self) -> Option<Fullscreen> {
        Some(Fullscreen::Borderless(Some(MonitorHandle)))
    }

    #[inline]
    pub fn set_decorations(&self, _decorations: bool) {}

    #[inline]
    pub fn is_decorated(&self) -> bool {
        false
    }

    #[inline]
    pub fn set_window_level(&self, _level: window::WindowLevel) {}

    #[inline]
    pub fn set_window_icon(&self, _window_icon: Option<crate::icon::Icon>) {}

    #[inline]
    pub fn set_ime_cursor_area(&self, _position: Position, _size: Size) {}

    #[inline]
    pub fn set_ime_allowed(&self, _allowed: bool) {}

    #[inline]
    pub fn set_ime_purpose(&self, _purpose: ImePurpose) {}

    #[inline]
    pub fn focus_window(&self) {}

    #[inline]
    pub fn request_user_attention(&self, _request_type: Option<window::UserAttentionType>) {}

    #[inline]
    pub fn set_cursor(&self, _: Cursor) {}

    #[inline]
    pub fn set_cursor_position(&self, _: Position) -> Result<(), error::ExternalError> {
        Ok(())
    }

    #[inline]
    pub fn set_cursor_grab(&self, _mode: window::CursorGrabMode) -> Result<(), error::ExternalError> {
        Ok(())
    }

    #[inline]
    pub fn set_cursor_visible(&self, _visible: bool) {}

    #[inline]
    pub fn drag_window(&self) -> Result<(), error::ExternalError> {
        Err(error::ExternalError::NotSupported(error::NotSupportedError::new()))
    }

    #[inline]
    pub fn drag_resize_window(
        &self,
        _direction: window::ResizeDirection,
    ) -> Result<(), error::ExternalError> {
        Err(error::ExternalError::NotSupported(error::NotSupportedError::new()))
    }

    #[inline]
    pub fn show_window_menu(&self, _position: Position) {}

    #[inline]
    pub fn set_cursor_hittest(&self, _hittest: bool) -> Result<(), error::ExternalError> {
        Ok(())
    }

    #[cfg(feature = "rwh_04")]
    #[inline]
    pub fn raw_window_handle_rwh_04(&self) -> rwh_04::RawWindowHandle {
        let mut handle = rwh_04::OrbitalHandle::empty();
        handle.window = 1usize as *mut _;
        rwh_04::RawWindowHandle::Orbital(handle)
    }

    #[cfg(feature = "rwh_05")]
    #[inline]
    pub fn raw_window_handle_rwh_05(&self) -> rwh_05::RawWindowHandle {
        let mut handle = rwh_05::OrbitalWindowHandle::empty();
        handle.window = 1usize as *mut _;
        rwh_05::RawWindowHandle::Orbital(handle)
    }

    #[cfg(feature = "rwh_05")]
    #[inline]
    pub fn raw_display_handle_rwh_05(&self) -> rwh_05::RawDisplayHandle {
        rwh_05::RawDisplayHandle::Orbital(rwh_05::OrbitalDisplayHandle::empty())
    }

    #[cfg(feature = "rwh_06")]
    #[inline]
    pub fn raw_window_handle_rwh_06(&self) -> Result<rwh_06::RawWindowHandle, rwh_06::HandleError> {
        let handle = rwh_06::OrbitalWindowHandle::new(
            std::ptr::NonNull::new(1usize as *mut _).unwrap(),
        );
        Ok(rwh_06::RawWindowHandle::Orbital(handle))
    }

    #[cfg(feature = "rwh_06")]
    #[inline]
    pub fn raw_display_handle_rwh_06(
        &self,
    ) -> Result<rwh_06::RawDisplayHandle, rwh_06::HandleError> {
        Ok(rwh_06::RawDisplayHandle::Orbital(rwh_06::OrbitalDisplayHandle::new()))
    }

    #[inline]
    pub fn set_enabled_buttons(&self, _buttons: window::WindowButtons) {}

    #[inline]
    pub fn enabled_buttons(&self) -> window::WindowButtons {
        window::WindowButtons::all()
    }

    #[inline]
    pub fn theme(&self) -> Option<window::Theme> {
        None
    }

    #[inline]
    pub fn has_focus(&self) -> bool {
        true
    }

    #[inline]
    pub fn set_theme(&self, _theme: Option<window::Theme>) {}

    pub fn set_content_protected(&self, _protected: bool) {}
}

pub struct EventLoop<T: 'static> {
    window_target: event_loop::ActiveEventLoop,
    user_events_sender: mpsc::Sender<T>,
    user_events_receiver: mpsc::Receiver<T>,
}

impl<T: 'static> EventLoop<T> {
    pub(crate) fn new(_: &PlatformSpecificEventLoopAttributes) -> Result<Self, EventLoopError> {
        let (user_events_sender, user_events_receiver) = mpsc::channel();
        Ok(Self {
            window_target: event_loop::ActiveEventLoop {
                p: ActiveEventLoop {
                    control_flow: Cell::new(ControlFlow::default()),
                    exit: Cell::new(false),
                },
                _marker: PhantomData,
            },
            user_events_sender,
            user_events_receiver,
        })
    }

    pub fn run<F>(self, mut event_handler: F) -> Result<(), EventLoopError>
    where
        F: FnMut(event::Event<T>, &event_loop::ActiveEventLoop),
    {
        event_handler(event::Event::NewEvents(StartCause::Init), &self.window_target);
        event_handler(event::Event::Resumed, &self.window_target);
        while let Ok(user_event) = self.user_events_receiver.try_recv() {
            event_handler(event::Event::UserEvent(user_event), &self.window_target);
        }
        event_handler(event::Event::AboutToWait, &self.window_target);
        event_handler(event::Event::LoopExiting, &self.window_target);
        Ok(())
    }

    pub fn window_target(&self) -> &event_loop::ActiveEventLoop {
        &self.window_target
    }

    pub fn create_proxy(&self) -> EventLoopProxy<T> {
        EventLoopProxy {
            user_events_sender: self.user_events_sender.clone(),
        }
    }
}

pub struct EventLoopProxy<T: 'static> {
    user_events_sender: mpsc::Sender<T>,
}

impl<T: 'static> EventLoopProxy<T> {
    pub fn send_event(&self, event: T) -> Result<(), event_loop::EventLoopClosed<T>> {
        self.user_events_sender
            .send(event)
            .map_err(|mpsc::SendError(x)| event_loop::EventLoopClosed(x))
    }
}

impl<T: 'static> Clone for EventLoopProxy<T> {
    fn clone(&self) -> Self {
        Self {
            user_events_sender: self.user_events_sender.clone(),
        }
    }
}

impl<T: 'static> Unpin for EventLoopProxy<T> {}

pub struct ActiveEventLoop {
    control_flow: Cell<ControlFlow>,
    exit: Cell<bool>,
}

impl ActiveEventLoop {
    pub fn create_custom_cursor(&self, source: CustomCursorSource) -> RootCustomCursor {
        let _ = source.inner;
        RootCustomCursor { inner: PlatformCustomCursor }
    }

    pub fn primary_monitor(&self) -> Option<MonitorHandle> {
        Some(MonitorHandle)
    }

    pub fn available_monitors(&self) -> VecDeque<MonitorHandle> {
        let mut v = VecDeque::with_capacity(1);
        v.push_back(MonitorHandle);
        v
    }

    #[inline]
    pub fn listen_device_events(&self, _allowed: DeviceEvents) {}

    #[cfg(feature = "rwh_05")]
    #[inline]
    pub fn raw_display_handle_rwh_05(&self) -> rwh_05::RawDisplayHandle {
        rwh_05::RawDisplayHandle::Orbital(rwh_05::OrbitalDisplayHandle::empty())
    }

    #[inline]
    pub fn system_theme(&self) -> Option<Theme> {
        None
    }

    #[cfg(feature = "rwh_06")]
    #[inline]
    pub fn raw_display_handle_rwh_06(
        &self,
    ) -> Result<rwh_06::RawDisplayHandle, rwh_06::HandleError> {
        Ok(rwh_06::RawDisplayHandle::Orbital(rwh_06::OrbitalDisplayHandle::new()))
    }

    pub fn set_control_flow(&self, control_flow: ControlFlow) {
        self.control_flow.set(control_flow)
    }

    pub fn control_flow(&self) -> ControlFlow {
        self.control_flow.get()
    }

    pub(crate) fn exit(&self) {
        self.exit.set(true);
    }

    pub(crate) fn exiting(&self) -> bool {
        self.exit.get()
    }

    pub(crate) fn owned_display_handle(&self) -> OwnedDisplayHandle {
        OwnedDisplayHandle
    }
}

#[derive(Clone)]
pub(crate) struct OwnedDisplayHandle;

impl OwnedDisplayHandle {
    #[cfg(feature = "rwh_05")]
    #[inline]
    pub fn raw_display_handle_rwh_05(&self) -> rwh_05::RawDisplayHandle {
        rwh_05::OrbitalDisplayHandle::empty().into()
    }

    #[cfg(feature = "rwh_06")]
    #[inline]
    pub fn raw_display_handle_rwh_06(
        &self,
    ) -> Result<rwh_06::RawDisplayHandle, rwh_06::HandleError> {
        Ok(rwh_06::OrbitalDisplayHandle::new().into())
    }
}
