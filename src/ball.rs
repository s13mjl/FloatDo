//! 悬浮球窗口 —— 纯 Win32 分层窗口，逐像素自绘。

use crate::gdi;
use crate::taskwin;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::w;

pub const BALL_SIZE: i32 = 100;
const TIMER_BREATH: usize = 1;
const TIMER_RIPPLE: usize = 2;

/// 显示悬浮球（从托盘菜单调用）。
pub unsafe fn show_ball() {
    if !BALL.is_null() {
        let st = &*BALL;
        ShowWindow(st.hwnd, SW_SHOW);
        SetWindowPos(st.hwnd, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
    }
}

pub struct BallState {
    pub hwnd: HWND,
    pub dragging: bool,
    pub offset: POINT,
    pub start_pos: POINT,
    pub breath: f32,
    pub breath_dir: f32,
    pub anim_scale: f32,
    pub anim_target: f32,
}

static mut BALL: *mut BallState = std::ptr::null_mut();

impl BallState {
    pub fn create() -> &'static mut BallState {
        let class = w!("FloatDoBallClass");
        unsafe {
            let wc = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                style: CS_HREDRAW | CS_VREDRAW,
                lpfnWndProc: Some(ball_wndproc),
                hInstance: GetModuleHandleW(None).unwrap().into(),
                hCursor: LoadCursorW(None, IDC_HAND).unwrap(),
                hbrBackground: HBRUSH(std::ptr::null_mut()),
                lpszClassName: class,
                ..Default::default()
            };
            RegisterClassExW(&wc);

            let hwnd = CreateWindowExW(
                WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
                class,
                w!("FloatDo"),
                WS_POPUP,
                100, 100, BALL_SIZE, BALL_SIZE,
                None, None,
                GetModuleHandleW(None).unwrap(),
                None,
            ).unwrap();

            let st = Box::leak(Box::new(BallState {
                hwnd,
                dragging: false,
                offset: POINT::default(),
                start_pos: POINT::default(),
                breath: 0.0,
                breath_dir: 1.0,
                anim_scale: 1.0,
                anim_target: 1.0,
            }));
            BALL = st;
            SetTimer(hwnd, TIMER_BREATH, 50, None);
            SetTimer(hwnd, TIMER_RIPPLE, 16, None);
            ShowWindow(hwnd, SW_SHOW);
            st
        }
    }
}

unsafe extern "system" fn ball_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_TIMER => {
            let id = wparam.0;
            let st = &mut *BALL;
            if id == TIMER_BREATH {
                st.breath += 0.05 * st.breath_dir;
                if st.breath >= 1.0 { st.breath = 1.0; st.breath_dir = -1.0; }
                else if st.breath <= 0.0 { st.breath = 0.0; st.breath_dir = 1.0; }
                let d = st.anim_target - st.anim_scale;
                if d.abs() > 0.01 {
                    st.anim_scale += d * 0.3;
                } else {
                    st.anim_scale = st.anim_target;
                }
                let px = gdi::render_ball(BALL_SIZE, st.breath, st.anim_scale);
                gdi::push_layered(hwnd, &px, BALL_SIZE, BALL_SIZE);
            }
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            let st = &mut *BALL;
            st.dragging = true;
            st.offset = point_from_lparam(lparam);
            st.start_pos = cursor_pos();
            st.anim_target = 0.9;
            SetCapture(hwnd);
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            let st = &mut *BALL;
            if st.dragging {
                let gp = cursor_pos();
                let nx = gp.x - st.offset.x;
                let ny = gp.y - st.offset.y;
                SetWindowPos(hwnd, None, nx, ny, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
            }
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            let st = &mut *BALL;
            st.dragging = false;
            ReleaseCapture();
            let end = cursor_pos();
            let dist = (end.x - st.start_pos.x).abs() + (end.y - st.start_pos.y).abs();
            if dist < 5 {
                st.anim_target = 1.1;
                taskwin::show_current();
            } else {
                st.anim_target = 1.0;
                snap_to_edge(hwnd);
            }
            LRESULT(0)
        }
        WM_RBUTTONUP => {
            let pt = cursor_pos();
            show_ball_menu(hwnd, pt);
            LRESULT(0)
        }
        WM_ERASEBKGND => LRESULT(1),
        WM_PAINT => LRESULT(0),
        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

fn point_from_lparam(lp: LPARAM) -> POINT {
    let v = lp.0;
    POINT {
        x: (v & 0xFFFF) as i16 as i32,
        y: ((v >> 16) & 0xFFFF) as i16 as i32,
    }
}

unsafe fn cursor_pos() -> POINT {
    let mut p = POINT::default();
    let _ = GetCursorPos(&mut p);
    p
}

unsafe fn snap_to_edge(hwnd: HWND) {
    let mon = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
    let mut mi = MONITORINFO::default();
    mi.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
    GetMonitorInfoW(mon, &mut mi);
    let work = mi.rcWork;

    let mut rc = RECT::default();
    let _ = GetWindowRect(hwnd, &mut rc);
    let w = rc.right - rc.left;
    let h = rc.bottom - rc.top;
    let x = rc.left;
    let y = rc.top;

    let dist_left = x - work.left;
    let dist_right = work.right - (x + w);
    let target_x = if dist_left < dist_right { work.left } else { work.right - w };
    let target_y = y.max(work.top).min(work.bottom - h);
    SetWindowPos(hwnd, None, target_x, target_y, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
}

unsafe fn show_ball_menu(hwnd: HWND, pt: POINT) {
    let menu = CreatePopupMenu().unwrap();
    AppendMenuW(menu, MF_STRING, 1, w!("隐藏悬浮球"));
    AppendMenuW(menu, MF_SEPARATOR, 0, None);
    AppendMenuW(menu, MF_STRING, 2, w!("退出程序"));
    let cmd = TrackPopupMenu(
        menu,
        TPM_RETURNCMD | TPM_NONOTIFY,
        pt.x, pt.y, 0, hwnd, None,
    ).0 as u32;
    DestroyMenu(menu);
    match cmd {
        1 => { ShowWindow(hwnd, SW_HIDE); }
        2 => { PostQuitMessage(0); }
        _ => {}
    }
}
