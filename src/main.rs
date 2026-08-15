//! FloatDo 纯 Win32 原生版 —— 内存占用极小。
//! 无 Python / Qt / 任何 GUI 框架，直接 Win32 API + GDI 自绘。

#![windows_subsystem = "windows"]
#![allow(unused_must_use)]
#![allow(unused_variables)]

mod ball;
mod emoji;
mod gdi;
mod store;
mod taskwin;
mod tray;

use std::sync::{Arc, Mutex};
use windows::Win32::Foundation::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::w;

/// 专用于接收托盘回调消息的隐藏窗口。
/// 必须和悬浮球窗口分开，否则托盘右键消息会被悬浮球的 WM_RBUTTONUP 拦截。
unsafe extern "system" fn tray_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if msg == WM_APP {
        let event = (lparam.0 & 0xFFFF) as u32;
        match event {
            x if x == WM_LBUTTONUP => {
                taskwin::show_current();
            }
            x if x == WM_RBUTTONUP || x == WM_RBUTTONDOWN || x == WM_CONTEXTMENU => {
                if tray::show_menu(hwnd) {
                    PostQuitMessage(0);
                }
            }
            _ => {}
        }
        return LRESULT(0);
    }
    DefWindowProcW(hwnd, msg, wparam, lparam)
}

fn main() {
    unsafe {
        let store = Arc::new(Mutex::new(store::TaskStore::new()));

        // 创建任务窗口 + 悬浮球
        taskwin::create(store);
        let ball = ball::BallState::create();

        // 创建独立的隐藏窗口接收托盘消息
        let tray_class = w!("FloatDoTrayClass");
        let hinst = GetModuleHandleW(None).unwrap();
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: WNDCLASS_STYLES::default(),
            lpfnWndProc: Some(tray_wndproc),
            hInstance: hinst.into(),
            lpszClassName: tray_class,
            ..Default::default()
        };
        RegisterClassExW(&wc);
        let tray_hwnd = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            tray_class,
            w!("FloatDoTray"),
            WINDOW_STYLE::default(),
            0, 0, 0, 0,
            None, None,
            hinst,
            None,
        ).unwrap();

        // 托盘图标
        let tray_icon = gdi::make_tray_icon();
        let mut tip_buf = [0u16; 128];
        let tip: Vec<u16> = "FloatDo 待办".encode_utf16().collect();
        tip_buf[..tip.len()].copy_from_slice(&tip);

        let nid = NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: tray_hwnd,
            uID: 1,
            uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP,
            uCallbackMessage: WM_APP,
            hIcon: tray_icon,
            szTip: tip_buf,
            ..Default::default()
        };
        Shell_NotifyIconW(NIM_ADD, &nid);

        // 消息循环
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        // 清理
        Shell_NotifyIconW(NIM_DELETE, &nid);
        let _ = DestroyIcon(tray_icon);
    }
}
