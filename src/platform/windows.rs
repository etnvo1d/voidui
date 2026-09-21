use winit::{event_loop::EventLoopBuilder, platform::windows::EventLoopBuilderExtWindows};

pub(super) fn configure_event_loop<T>(builder: &mut EventLoopBuilder<T>) {
    // Winit negotiates per-monitor DPI awareness before creating any HWND and
    // translates WM_DPICHANGED into scale/physical-size notifications.
    builder.with_dpi_aware(true);
}

use crate::{
    WindowControlArea as Area, WindowState,
    core::{geometry::Point, stacking::HitRegion, widget_tree::WidgetTree},
};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::Arc,
};
use windows_sys::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
    Graphics::{
        Dwm::{DwmDefWindowProc, DwmExtendFrameIntoClientArea},
        Gdi::ScreenToClient,
    },
    UI::{
        Controls::WM_MOUSELEAVE,
        HiDpi::{GetDpiForWindow, GetSystemMetricsForDpi},
        Input::KeyboardAndMouse::{TME_LEAVE, TME_NONCLIENT, TRACKMOUSEEVENT, TrackMouseEvent},
        Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
        WindowsAndMessaging::*,
    },
};
use winit::window::Window;

#[derive(Default)]
struct Snapshot {
    regions: Vec<(HitRegion, Area)>,
    fullscreen: bool,
    resizable: bool,
    minimizable: bool,
}
struct NativeState {
    snapshot: RefCell<Snapshot>,
    attached: Cell<bool>,
    dirty: Cell<bool>,
}
pub(super) struct Chrome {
    owner: Arc<Window>,
    state: Rc<NativeState>,
    registration: *const NativeState,
}
impl Chrome {
    pub fn new(owner: &Arc<Window>) -> anyhow::Result<Self> {
        let RawWindowHandle::Win32(handle) = owner.window_handle()?.as_raw() else {
            anyhow::bail!("custom Windows chrome requires an HWND");
        };
        let hwnd = handle.hwnd.get() as HWND;
        let state = Rc::new(NativeState {
            snapshot: Default::default(),
            attached: Cell::new(true),
            dirty: Cell::new(true),
        });
        let registration = Rc::into_raw(state.clone());
        // SAFETY: The extra Rc reference belongs to the subclass registration.
        // WM_NCDESTROY or successful removal releases it exactly once. The stable
        // allocation contains no WidgetTree/native-owner references or mutable aliases.
        unsafe {
            if SetWindowSubclass(
                hwnd,
                Some(chrome_proc),
                registration as usize,
                registration as usize,
            ) == 0
            {
                drop(Rc::from_raw(registration));
                return Err(std::io::Error::last_os_error().into());
            }
            let margins = windows_sys::Win32::UI::Controls::MARGINS {
                cxLeftWidth: 1,
                cxRightWidth: 1,
                cyTopHeight: 1,
                cyBottomHeight: 1,
            };
            DwmExtendFrameIntoClientArea(hwnd, &margins);
            SetWindowPos(
                hwnd,
                std::ptr::null_mut(),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
            );
        }
        Ok(Self {
            owner: owner.clone(),
            state,
            registration,
        })
    }
    pub fn publish(&self, tree: &WidgetTree, state: &WindowState, changed: bool) {
        if !changed && !self.state.dirty.replace(false) {
            return;
        }
        self.state.dirty.set(false);
        let mut snapshot = self.state.snapshot.borrow_mut();
        snapshot.regions.clear();
        snapshot.fullscreen = state.fullscreen;
        snapshot.resizable = state.resizable;
        snapshot.minimizable = state.minimizable;
        tree.visit_hit_regions(|region| {
            let area = tree.window_control_region(region.clone());
            // Include ordinary client regions: popovers/inputs must occlude the
            // titlebar beneath them, even when they have no window-control role.
            snapshot.regions.push((region, area));
            true
        });
    }
    pub fn invalidate(&self) {
        self.state.dirty.set(true);
        self.state.snapshot.borrow_mut().regions.clear();
    }
}
impl Drop for Chrome {
    fn drop(&mut self) {
        let Ok(handle) = self.owner.window_handle() else {
            return;
        };
        let RawWindowHandle::Win32(handle) = handle.as_raw() else {
            return;
        };
        // SAFETY: All registration changes run on the creating thread. A callback
        // pins its own Rc before forwarding any message that can reenter this code.
        unsafe {
            if self.state.attached.get()
                && RemoveWindowSubclass(
                    handle.hwnd.get() as HWND,
                    Some(chrome_proc),
                    self.registration as usize,
                ) != 0
            {
                self.state.attached.set(false);
                drop(Rc::from_raw(self.registration));
            }
        }
    }
}
fn screen_point(lparam: LPARAM) -> POINT {
    POINT {
        x: (lparam as u16 as i16) as i32,
        y: ((lparam >> 16) as u16 as i16) as i32,
    }
}
fn client_lparam(hwnd: HWND, lparam: LPARAM) -> LPARAM {
    let mut point = screen_point(lparam);
    // SAFETY: The HWND comes from the active native callback.
    unsafe {
        ScreenToClient(hwnd, &mut point);
    }
    ((point.y as u16 as usize) << 16 | point.x as u16 as usize) as LPARAM
}
unsafe extern "system" fn chrome_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    id: usize,
    data: usize,
) -> LRESULT {
    // SAFETY: Comctl32 supplies the pointer held by the registration. Pin it across
    // reentrant native calls, including a destroy/removal while dispatch is active.
    let state = unsafe {
        Rc::increment_strong_count(data as *const NativeState);
        Rc::from_raw(data as *const NativeState)
    };
    // No unwinding is permitted through the Windows ABI. The default window proc
    // remains the fallback if an invariant fails in our optional chrome adapter.
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
        match message {
            WM_NCDESTROY => {
                if state.attached.replace(false) {
                    RemoveWindowSubclass(hwnd, Some(chrome_proc), id);
                    drop(Rc::from_raw(data as *const NativeState));
                }
            }
            WM_NCCALCSIZE if wparam != 0 => {
                // Winit changes the style before reporting fullscreen. Consult
                // the live frame style rather than the previous rendered snapshot.
                if GetWindowLongPtrW(hwnd, GWL_STYLE) as u32 & WS_CAPTION == 0 {
                    return None;
                }
                let params = &mut *(lparam as *mut NCCALCSIZE_PARAMS);
                let top = params.rgrc[0].top;
                DefSubclassProc(hwnd, message, wparam, lparam);
                let dpi = GetDpiForWindow(hwnd);
                params.rgrc[0].top = top
                    + if IsZoomed(hwnd) != 0 {
                        GetSystemMetricsForDpi(SM_CYFRAME, dpi)
                            + GetSystemMetricsForDpi(SM_CXPADDEDBORDER, dpi)
                    } else {
                        0
                    };
                return Some(0);
            }
            WM_NCHITTEST => {
                let snapshot = state.snapshot.borrow();
                if snapshot.fullscreen {
                    return None;
                }
                let mut point = screen_point(lparam);
                ScreenToClient(hwnd, &mut point);
                let mut rect = RECT::default();
                GetClientRect(hwnd, &mut rect);
                // Let the existing frame own hits outside the client rectangle.
                if point.x < 0 || point.y < 0 || point.x >= rect.right || point.y >= rect.bottom {
                    return None;
                }
                let dpi = GetDpiForWindow(hwnd).max(96);
                if snapshot.resizable && IsZoomed(hwnd) == 0 {
                    let border = GetSystemMetricsForDpi(SM_CYFRAME, dpi)
                        + GetSystemMetricsForDpi(SM_CXPADDEDBORDER, dpi);
                    if point.y < border {
                        let hit = if point.x < border {
                            HTTOPLEFT
                        } else if point.x >= rect.right - border {
                            HTTOPRIGHT
                        } else {
                            HTTOP
                        };
                        return Some(hit as LRESULT);
                    }
                }
                let scale = dpi as f32 / 96.0;
                let p = Point::new(point.x as f32 / scale, point.y as f32 / scale);
                let area = snapshot
                    .regions
                    .iter()
                    .find(|(region, _)| region.contains(p))
                    .map(|(_, area)| *area)
                    .unwrap_or(Area::Client);
                return Some(match area {
                    Area::Drag => HTCAPTION,
                    Area::Close => HTCLOSE,
                    Area::Max if snapshot.resizable => HTMAXBUTTON,
                    Area::Min if snapshot.minimizable => HTMINBUTTON,
                    _ => HTCLIENT,
                } as LRESULT);
            }
            WM_NCMOUSEMOVE => {
                let mut tracking = TRACKMOUSEEVENT {
                    cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                    dwFlags: TME_LEAVE | TME_NONCLIENT,
                    hwndTrack: hwnd,
                    dwHoverTime: 0,
                };
                TrackMouseEvent(&mut tracking);
                // Winit only translates client motion. Feed its normal path so CSS
                // hover and the button press/release target use real coordinates.
                DefSubclassProc(hwnd, WM_MOUSEMOVE, 0, client_lparam(hwnd, lparam));
                let mut result = 0;
                DwmDefWindowProc(hwnd, message, wparam, lparam, &mut result);
                return Some(result);
            }
            WM_NCMOUSELEAVE => {
                DefSubclassProc(hwnd, WM_MOUSELEAVE, 0, 0);
            }
            WM_NCLBUTTONDOWN | WM_NCLBUTTONDBLCLK
                if [HTCLOSE, HTMINBUTTON, HTMAXBUTTON].contains(&(wparam as u32)) =>
            {
                let position = client_lparam(hwnd, lparam);
                DefSubclassProc(hwnd, WM_MOUSEMOVE, 0, position);
                // Winit's primary-button path captures the pointer. Release outside
                // the original button therefore cancels activation automatically.
                return Some(DefSubclassProc(hwnd, WM_LBUTTONDOWN, 1, position));
            }
            WM_NCLBUTTONUP if [HTCLOSE, HTMINBUTTON, HTMAXBUTTON].contains(&(wparam as u32)) => {
                return Some(DefSubclassProc(
                    hwnd,
                    WM_LBUTTONUP,
                    0,
                    client_lparam(hwnd, lparam),
                ));
            }
            WM_SIZE | WM_DPICHANGED => {
                state.dirty.set(true);
                state.snapshot.borrow_mut().regions.clear();
            }
            _ => {}
        }
        None
    }));
    match result {
        Ok(Some(result)) => result,
        _ => unsafe { DefSubclassProc(hwnd, message, wparam, lparam) },
    }
}
