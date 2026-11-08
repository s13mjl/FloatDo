//! 任务窗口 —— 纯 Win32 自绘（无 Edit/ListBox 控件，内存占用最低）。

use crate::gdi::{self, rgb};
use crate::store::{short_date, TaskStore, today};
use chrono::{Datelike, Local, NaiveDate};
use std::sync::{Arc, Mutex};
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetFocus, GetKeyState, ReleaseCapture, SetCapture, SetFocus, VK_A, VK_CONTROL, VK_DELETE,
    VK_END, VK_ESCAPE, VK_HOME, VK_LEFT, VK_RIGHT, VK_SHIFT,
};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::w;

pub const WIN_W: i32 = 380;
pub const WIN_H: i32 = 550;

const TITLE_H: i32 = 40;
const HEADER_H: i32 = 56;
const INPUT_H: i32 = 50;
const CARD_H: i32 = 60;
const CARD_GAP: i32 = 8;
const MARGIN_X: i32 = 15;
const CORNER_R: i32 = 16;
const TIMER_CARET: usize = 1;

// 输入框内的日期胶囊
const DATE_CHIP_W: i32 = 84;
const DATE_CHIP_H: i32 = 26;

// 日期选择弹层（全自绘）
const CAL_W: i32 = 262;
const CAL_PAD: i32 = 12;
const CAL_HDR_Y: i32 = 10;
const CAL_HDR_H: i32 = 34;
const CAL_WEEK_H: i32 = 24;
const CAL_CELL: i32 = 34;
const CAL_ROWS: i32 = 6;
const CAL_GRID_Y: i32 = CAL_HDR_Y + CAL_HDR_H + CAL_WEEK_H;
const CAL_GRID_H: i32 = CAL_CELL * CAL_ROWS;
const CAL_FOOT_Y: i32 = CAL_GRID_Y + CAL_GRID_H + 6;
const CAL_FOOT_H: i32 = 30;
const CAL_H: i32 = CAL_FOOT_Y + CAL_FOOT_H + 10;
const CAL_HIT_NONE: i32 = -1;
const CAL_HIT_PREV: i32 = 100;
const CAL_HIT_NEXT: i32 = 101;
const CAL_HIT_TODAY: i32 = 102;
const CAL_HIT_CLEAR: i32 = 103;

const C_PURPLE: (u8, u8, u8) = (108, 92, 231);
const C_WHITE: (u8, u8, u8) = (255, 255, 255);
const C_BORDER: (u8, u8, u8) = (222, 226, 230);
const C_TEXT: (u8, u8, u8) = (45, 52, 54);
const C_MUTED: (u8, u8, u8) = (99, 110, 114);
const C_CARD: (u8, u8, u8) = (248, 249, 250);
const C_CARD_DONE: (u8, u8, u8) = (233, 236, 239);
const C_CHK_BORDER: (u8, u8, u8) = (178, 190, 195);
const C_DEL: (u8, u8, u8) = (178, 190, 195);
const C_HOVER: (u8, u8, u8) = (238, 237, 250);
const C_WEEKEND: (u8, u8, u8) = (168, 176, 180);

pub struct TaskWinState {
    pub hwnd: HWND,
    pub store: Arc<Mutex<TaskStore>>,
    pub input_visible: bool,
    pub input_buffer: String,
    /// 新建任务时选择的日期（YYYY-MM-DD），None 表示不设定日期。
    pub input_date: Option<String>,
    /// 插入光标在 input_buffer 中的字符位置（0 表示行首）。
    pub input_caret: usize,
    /// 选区起点（字符位置）；与 input_caret 不同时表示存在选区。
    pub sel_anchor: Option<usize>,
    /// 是否正在用鼠标拖动选择文本。
    pub text_dragging: bool,
    /// 正在编辑的任务 id；None 表示输入框用于新建任务。
    pub editing_id: Option<String>,
    pub drag_off: POINT,
    pub dragging: bool,
    pub font_title: HFONT,
    pub font_header: HFONT,
    pub font_item: HFONT,
    pub font_item_done: HFONT,
    pub font_btn: HFONT,
    pub font_date: HFONT,
    pub scroll_y: i32,
    pub caret_on: bool,
}

/// 日期选择弹层状态。
struct CalState {
    hwnd: HWND,
    view_year: i32,
    view_month: u32,
    hover: i32,
    font_hdr: HFONT,
    font_day: HFONT,
    font_week: HFONT,
}

static mut TASKWIN: *mut TaskWinState = std::ptr::null_mut();
static mut CAL: *mut CalState = std::ptr::null_mut();
static mut CAL_HIDE_AT: Option<std::time::Instant> = None;

pub fn create(store: Arc<Mutex<TaskStore>>) {
    let class = w!("FloatDoTaskClass");
    unsafe {
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(task_wndproc),
            hInstance: GetModuleHandleW(None).unwrap().into(),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap(),
            hbrBackground: HBRUSH(std::ptr::null_mut()),
            lpszClassName: class,
            ..Default::default()
        };
        RegisterClassExW(&wc);

        let hwnd = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_LAYERED,
            class,
            w!("待办事项"),
            WS_POPUP,
            CW_USEDEFAULT, CW_USEDEFAULT, WIN_W, WIN_H,
            None, None,
            GetModuleHandleW(None).unwrap(),
            None,
        ).unwrap();

        let font_title = gdi::make_font(-20, true, false);
        let font_header = gdi::make_font(-13, true, false);
        let font_item = gdi::make_font(-15, false, false);
        let font_item_done = gdi::make_font(-15, false, true);
        let font_btn = gdi::make_font(-18, true, false);
        let font_date = gdi::make_font(-13, false, false);

        let st = Box::leak(Box::new(TaskWinState {
            hwnd,
            store,
            input_visible: false,
            input_buffer: String::new(),
            input_date: None,
            input_caret: 0,
            sel_anchor: None,
            text_dragging: false,
            editing_id: None,
            drag_off: POINT::default(),
            dragging: false,
            font_title,
            font_header,
            font_item,
            font_item_done,
            font_btn,
            font_date,
            scroll_y: 0,
            caret_on: false,
        }));
        TASKWIN = st;

        // 日期选择弹层：预先创建，点日期胶囊时按需显示
        let cal_class = w!("FloatDoCalClass");
        let cal_wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(cal_wndproc),
            hInstance: GetModuleHandleW(None).unwrap().into(),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap(),
            hbrBackground: HBRUSH(std::ptr::null_mut()),
            lpszClassName: cal_class,
            ..Default::default()
        };
        RegisterClassExW(&cal_wc);

        let cal_hwnd = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_LAYERED,
            cal_class,
            w!("选择日期"),
            WS_POPUP,
            0, 0, CAL_W, CAL_H,
            hwnd, None,
            GetModuleHandleW(None).unwrap(),
            None,
        ).unwrap();

        let now = Local::now();
        CAL = Box::leak(Box::new(CalState {
            hwnd: cal_hwnd,
            view_year: now.year(),
            view_month: now.month(),
            hover: CAL_HIT_NONE,
            font_hdr: gdi::make_font(-15, true, false),
            font_day: gdi::make_font(-14, false, false),
            font_week: gdi::make_font(-12, false, false),
        }));
    }
}

pub fn show_current() {
    unsafe {
        if !TASKWIN.is_null() {
            let st = &mut *TASKWIN;
            ShowWindow(st.hwnd, SW_SHOW);
            SetForegroundWindow(st.hwnd);
            SetFocus(st.hwnd);
            reset_caret(st.hwnd, st);
            InvalidateRect(st.hwnd, None, false);
            // 同步画出首帧，避免分层窗口在 UpdateLayeredWindow 之前呈现为透明
            let _ = UpdateWindow(st.hwnd);
        }
    }
}

pub fn hide_current() {
    unsafe {
        if !TASKWIN.is_null() {
            let st = &mut *TASKWIN;
            let _ = KillTimer(st.hwnd, TIMER_CARET);
            st.caret_on = false;
            hide_calendar();
            ShowWindow(st.hwnd, SW_HIDE);
        }
    }
}

unsafe fn st() -> &'static mut TaskWinState {
    &mut *TASKWIN
}

fn list_top(s: &TaskWinState) -> i32 {
    TITLE_H + HEADER_H + if s.input_visible { INPUT_H } else { 0 }
}

fn scroll_max(content_len: usize, view_top: i32) -> i32 {
    let content_h = content_len as i32 * (CARD_H + CARD_GAP);
    let view_h = WIN_H - view_top;
    (content_h - view_h).max(0)
}

/// 输入框右侧「日期」胶囊的矩形。
fn chip_rect(s: &TaskWinState) -> RECT {
    let in_y = TITLE_H + HEADER_H;
    let cy = in_y + INPUT_H / 2;
    RECT {
        left: WIN_W - 20 - 8 - DATE_CHIP_W,
        top: cy - DATE_CHIP_H / 2,
        right: WIN_W - 20 - 8,
        bottom: cy + DATE_CHIP_H / 2,
    }
}

fn in_rect(rc: RECT, p: POINT) -> bool {
    p.x >= rc.left && p.x < rc.right && p.y >= rc.top && p.y < rc.bottom
}

// ---------- 输入框文本编辑（按字符位置，兼容中文） ----------

fn buf_char_count(buf: &str) -> usize {
    buf.chars().count()
}

fn buf_byte_index(buf: &str, char_idx: usize) -> usize {
    buf.char_indices().nth(char_idx).map(|(i, _)| i).unwrap_or(buf.len())
}

fn buf_insert(buf: &mut String, char_idx: usize, ch: char) {
    let at = buf_byte_index(buf, char_idx);
    buf.insert(at, ch);
}

fn buf_remove_before(buf: &mut String, char_idx: usize) {
    if char_idx == 0 {
        return;
    }
    let start = buf_byte_index(buf, char_idx - 1);
    let end = buf_byte_index(buf, char_idx);
    buf.replace_range(start..end, "");
}

fn buf_remove_at(buf: &mut String, char_idx: usize) {
    if char_idx >= buf_char_count(buf) {
        return;
    }
    let start = buf_byte_index(buf, char_idx);
    let end = buf_byte_index(buf, char_idx + 1);
    buf.replace_range(start..end, "");
}

/// 当前选区（已归一化为 start..end）；无选区返回 None。
fn sel_range(s: &TaskWinState) -> Option<(usize, usize)> {
    let anchor = s.sel_anchor?;
    if anchor == s.input_caret {
        None
    } else {
        Some((anchor.min(s.input_caret), anchor.max(s.input_caret)))
    }
}

/// 有选区时删除选区内容（光标落到选区起点）。
fn delete_selection(s: &mut TaskWinState) -> bool {
    if let Some((a, b)) = sel_range(s) {
        let start = buf_byte_index(&s.input_buffer, a);
        let end = buf_byte_index(&s.input_buffer, b);
        s.input_buffer.replace_range(start..end, "");
        s.input_caret = a;
        s.sel_anchor = None;
        return true;
    }
    false
}

/// 把输入框内的鼠标 x 坐标换算成最接近的字符位置。
unsafe fn caret_from_x(s: &TaskWinState, x: i32) -> usize {
    let left = 20 + 12;
    let hdc = GetDC(s.hwnd);
    let old = SelectObject(hdc, s.font_item);
    let total = buf_char_count(&s.input_buffer);
    let mut prev_w = 0;
    let mut result = total;
    let mut prefix = String::new();
    let mut idx = 0;
    for ch in s.input_buffer.chars() {
        prefix.push(ch);
        idx += 1;
        let mut sz = SIZE::default();
        let wide: Vec<u16> = prefix.encode_utf16().collect();
        GetTextExtentPoint32W(hdc, &wide, &mut sz);
        let w = sz.cx;
        if x < left + prev_w + (w - prev_w) / 2 {
            result = idx - 1;
            break;
        }
        prev_w = w;
    }
    SelectObject(hdc, old);
    ReleaseDC(s.hwnd, hdc);
    result
}

/// 退出编辑/新建状态并清空输入框。
fn clear_input(s: &mut TaskWinState) {
    s.input_buffer.clear();
    s.input_date = None;
    s.input_caret = 0;
    s.sel_anchor = None;
    s.text_dragging = false;
    s.editing_id = None;
    s.input_visible = false;
}

/// 进入编辑状态：把已设定任务的标题与日期载入输入框。
unsafe fn begin_edit(task: &crate::store::Task) {
    let s = st();
    s.input_visible = true;
    s.editing_id = Some(task.id.clone());
    s.input_buffer = task.title.clone();
    s.input_date = task.due_date.clone();
    s.input_caret = buf_char_count(&s.input_buffer);
    s.sel_anchor = None;
    s.text_dragging = false;
    let hwnd = s.hwnd;
    SetFocus(hwnd);
    reset_caret(hwnd, s);
    InvalidateRect(hwnd, None, false);
}

// ---------- 日期选择弹层（全自绘） ----------

fn cal_prev_rect() -> RECT {
    RECT {
        left: CAL_PAD,
        top: CAL_HDR_Y,
        right: CAL_PAD + 28,
        bottom: CAL_HDR_Y + CAL_HDR_H,
    }
}

fn cal_next_rect() -> RECT {
    RECT {
        left: CAL_W - CAL_PAD - 28,
        top: CAL_HDR_Y,
        right: CAL_W - CAL_PAD,
        bottom: CAL_HDR_Y + CAL_HDR_H,
    }
}

fn cal_day_rect(idx: i32) -> RECT {
    let col = idx % 7;
    let row = idx / 7;
    RECT {
        left: CAL_PAD + col * CAL_CELL,
        top: CAL_GRID_Y + row * CAL_CELL,
        right: CAL_PAD + col * CAL_CELL + CAL_CELL,
        bottom: CAL_GRID_Y + row * CAL_CELL + CAL_CELL,
    }
}

fn cal_footer_rect(i: i32) -> RECT {
    let w = (CAL_W - CAL_PAD * 2 - 8) / 2;
    let left = CAL_PAD + i * (w + 8);
    RECT {
        left,
        top: CAL_FOOT_Y,
        right: left + w,
        bottom: CAL_FOOT_Y + CAL_FOOT_H,
    }
}

fn cal_first_weekday(year: i32, month: u32) -> i32 {
    NaiveDate::from_ymd_opt(year, month, 1)
        .map(|d| d.weekday().num_days_from_sunday() as i32)
        .unwrap_or(0)
}

fn cal_days_in_month(year: i32, month: u32) -> i32 {
    let (ny, nm) = if month == 12 { (year + 1, 1) } else { (year, month + 1) };
    match (
        NaiveDate::from_ymd_opt(year, month, 1),
        NaiveDate::from_ymd_opt(ny, nm, 1),
    ) {
        (Some(a), Some(b)) => (b - a).num_days() as i32,
        _ => 30,
    }
}

/// 网格位置对应的日期（不在本月的格子返回 None）。
fn cal_day_at(cal: &CalState, idx: i32) -> Option<u32> {
    let day = idx - cal_first_weekday(cal.view_year, cal.view_month) + 1;
    if (1..=cal_days_in_month(cal.view_year, cal.view_month)).contains(&day) {
        Some(day as u32)
    } else {
        None
    }
}

fn cal_hit(cal: &CalState, p: POINT) -> i32 {
    if in_rect(cal_prev_rect(), p) {
        return CAL_HIT_PREV;
    }
    if in_rect(cal_next_rect(), p) {
        return CAL_HIT_NEXT;
    }
    if in_rect(cal_footer_rect(0), p) {
        return CAL_HIT_TODAY;
    }
    if in_rect(cal_footer_rect(1), p) {
        return CAL_HIT_CLEAR;
    }
    for idx in 0..CAL_ROWS * 7 {
        if cal_day_at(cal, idx).is_some() && in_rect(cal_day_rect(idx), p) {
            return idx;
        }
    }
    CAL_HIT_NONE
}

unsafe fn hide_calendar() {
    if !CAL.is_null() {
        ShowWindow((*CAL).hwnd, SW_HIDE);
        CAL_HIDE_AT = Some(std::time::Instant::now());
    }
}

unsafe fn toggle_calendar(main: HWND) {
    if CAL.is_null() {
        return;
    }
    if IsWindowVisible((*CAL).hwnd).as_bool() {
        hide_calendar();
        return;
    }
    // 弹层刚因点击别处而关闭时，不要被同一次点击立刻重新打开
    if let Some(t) = CAL_HIDE_AT {
        if t.elapsed() < std::time::Duration::from_millis(150) {
            return;
        }
    }
    show_calendar(main);
}

/// 在日期胶囊下方（空间不足时上方）弹出日历。
unsafe fn show_calendar(main: HWND) {
    let cal = &mut *CAL;
    let s = st();
    let base = s
        .input_date
        .as_deref()
        .and_then(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok())
        .unwrap_or_else(|| Local::now().date_naive());
    cal.view_year = base.year();
    cal.view_month = base.month();
    cal.hover = CAL_HIT_NONE;

    let chip = chip_rect(s);
    let mut below = POINT { x: chip.left, y: chip.bottom + 6 };
    let mut above = POINT { x: chip.left, y: chip.top - 6 };
    let _ = ClientToScreen(main, &mut below);
    let _ = ClientToScreen(main, &mut above);

    let mon = MonitorFromWindow(main, MONITOR_DEFAULTTONEAREST);
    let mut mi = MONITORINFO::default();
    mi.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
    GetMonitorInfoW(mon, &mut mi);
    let work = mi.rcWork;

    let x = below.x.clamp(work.left, (work.right - CAL_W).max(work.left));
    let y = if below.y + CAL_H <= work.bottom {
        below.y
    } else {
        (above.y - CAL_H).max(work.top)
    };

    SetWindowPos(cal.hwnd, HWND_TOPMOST, x, y, CAL_W, CAL_H, SWP_NOACTIVATE);
    ShowWindow(cal.hwnd, SW_SHOW);
    SetForegroundWindow(cal.hwnd);
    SetFocus(cal.hwnd);
    InvalidateRect(cal.hwnd, None, false);
    // 分层窗口必须先 UpdateLayeredWindow 才有内容，强制立即画首帧
    let _ = UpdateWindow(cal.hwnd);
}

/// 选定日期（None 表示清除），关闭弹层并把焦点交回任务窗口。
unsafe fn apply_date(date: Option<String>) {
    let s = st();
    s.input_date = date;
    let main = s.hwnd;
    hide_calendar();
    InvalidateRect(main, None, false);
    SetForegroundWindow(main);
    SetFocus(main);
    reset_caret(main, s);
}

/// 鼠标是否位于某张任务卡片上（用于切换手形光标，卡片可点击编辑）。
fn over_card_action(s: &TaskWinState, x: i32, y: i32) -> bool {
    if x < MARGIN_X || x > WIN_W - MARGIN_X {
        return false;
    }
    let count = s.store.lock().unwrap().count() as i32;
    let top = list_top(s) - s.scroll_y;
    for idx in 0..count {
        let y0 = top + idx * (CARD_H + CARD_GAP);
        if y >= y0 && y <= y0 + CARD_H {
            return true;
        }
    }
    false
}

/// 输入框是否处于「可显示插入光标」状态（打开且窗口持有焦点）。
unsafe fn caret_active(hwnd: HWND, s: &TaskWinState) -> bool {
    s.input_visible && GetFocus() == hwnd
}

/// 让插入光标立即可见并重启闪烁计时（打字时保持常亮）。
unsafe fn reset_caret(hwnd: HWND, s: &mut TaskWinState) {
    if !caret_active(hwnd, s) {
        stop_caret(hwnd, s);
        return;
    }
    s.caret_on = true;
    SetTimer(hwnd, TIMER_CARET, 530, None);
    invalidate_input(hwnd);
}

/// 停止闪烁并隐藏插入光标。
unsafe fn stop_caret(hwnd: HWND, s: &mut TaskWinState) {
    let _ = KillTimer(hwnd, TIMER_CARET);
    if s.caret_on {
        s.caret_on = false;
        invalidate_input(hwnd);
    }
}

unsafe extern "system" fn task_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if TASKWIN.is_null() {
        return DefWindowProcW(hwnd, msg, wparam, lparam);
    }
    match msg {
        WM_PAINT => {
            paint(hwnd);
            LRESULT(0)
        }
        // 分层窗口只有 UpdateLayeredWindow 过才有内容，显示时必须整窗重绘一次
        WM_SHOWWINDOW => {
            if wparam.0 != 0 {
                InvalidateRect(hwnd, None, false);
            }
            LRESULT(0)
        }
        WM_SETFOCUS => {
            reset_caret(hwnd, st());
            LRESULT(0)
        }
        WM_KILLFOCUS => {
            stop_caret(hwnd, st());
            LRESULT(0)
        }
        WM_TIMER => {
            if wparam.0 == TIMER_CARET {
                let s = st();
                s.caret_on = !s.caret_on;
                invalidate_input(hwnd);
            }
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            let p = point_from_lparam(lparam);
            let s = st();
            if p.y < TITLE_H {
                if p.x <= 45 {
                    hide_current();
                    return LRESULT(0);
                }
                s.dragging = true;
                let mut gp = POINT::default();
                let _ = GetCursorPos(&mut gp);
                let mut rc = RECT::default();
                let _ = GetWindowRect(hwnd, &mut rc);
                s.drag_off = POINT { x: gp.x - rc.left, y: gp.y - rc.top };
                SetCapture(hwnd);
                return LRESULT(0);
            }
            if p.y < TITLE_H + HEADER_H && p.x > WIN_W - 60 {
                let open = !s.input_visible;
                clear_input(s);
                s.input_visible = open;
                InvalidateRect(hwnd, None, false);
                reset_caret(hwnd, s);
                return LRESULT(0);
            }
            if s.input_visible && p.y >= TITLE_H + HEADER_H && p.y < TITLE_H + HEADER_H + INPUT_H {
                if in_rect(chip_rect(s), p) {
                    toggle_calendar(hwnd);
                } else {
                    SetFocus(hwnd);
                    let caret = caret_from_x(s, p.x);
                    s.input_caret = caret;
                    s.sel_anchor = Some(caret);
                    s.text_dragging = true;
                    SetCapture(hwnd);
                    reset_caret(hwnd, s);
                }
                return LRESULT(0);
            }
            let top = list_top(s) - s.scroll_y;
            let tasks = s.store.lock().unwrap().all();
            let mut idx = 0i32;
            for task in &tasks {
                let y0 = top + idx * (CARD_H + CARD_GAP);
                let y1 = y0 + CARD_H;
                if p.y >= y0 && p.y <= y1 {
                    if p.x >= MARGIN_X + 15 && p.x <= MARGIN_X + 15 + 22 {
                        let id = task.id.clone();
                        s.store.lock().unwrap().toggle(&id);
                        InvalidateRect(hwnd, None, false);
                        return LRESULT(0);
                    }
                    if p.x >= WIN_W - MARGIN_X - 24 - 12 && p.x <= WIN_W - MARGIN_X {
                        let id = task.id.clone();
                        s.store.lock().unwrap().delete(&id);
                        if s.editing_id.as_deref() == Some(id.as_str()) {
                            clear_input(s);
                        }
                        InvalidateRect(hwnd, None, false);
                        return LRESULT(0);
                    }
                    // 卡片其余区域 → 编辑该任务的文本与日期
                    if s.editing_id.as_deref() != Some(task.id.as_str()) {
                        let target = task.clone();
                        begin_edit(&target);
                    } else {
                        SetFocus(hwnd);
                        reset_caret(hwnd, s);
                    }
                    return LRESULT(0);
                }
                idx += 1;
            }
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            let s = st();
            if s.text_dragging {
                let p = point_from_lparam(lparam);
                let caret = caret_from_x(s, p.x);
                if caret != s.input_caret {
                    s.input_caret = caret;
                    reset_caret(hwnd, s);
                }
            }
            if s.dragging {
                let mut gp = POINT::default();
                let _ = GetCursorPos(&mut gp);
                let nx = gp.x - s.drag_off.x;
                let ny = gp.y - s.drag_off.y;
                SetWindowPos(hwnd, None, nx, ny, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
            }
            LRESULT(0)
        }
        WM_SETCURSOR => {
            if (lparam.0 & 0xFFFF) as u32 == HTCLIENT {
                let s = st();
                let mut p = POINT::default();
                let _ = GetCursorPos(&mut p);
                let _ = ScreenToClient(hwnd, &mut p);
                let icon = if p.y < TITLE_H {
                    if p.x <= 45 { IDC_HAND } else { IDC_ARROW }
                } else if p.y < TITLE_H + HEADER_H {
                    if p.x > WIN_W - 60 { IDC_HAND } else { IDC_ARROW }
                } else if s.input_visible && p.y < TITLE_H + HEADER_H + INPUT_H {
                    if in_rect(chip_rect(s), p) { IDC_HAND } else { IDC_IBEAM }
                } else if over_card_action(s, p.x, p.y) {
                    IDC_HAND
                } else {
                    IDC_ARROW
                };
                SetCursor(LoadCursorW(None, icon).unwrap_or_default());
                return LRESULT(1);
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        WM_LBUTTONUP => {
            let s = st();
            if s.text_dragging {
                s.text_dragging = false;
                if s.sel_anchor == Some(s.input_caret) {
                    s.sel_anchor = None;
                }
                ReleaseCapture();
                invalidate_input(hwnd);
                return LRESULT(0);
            }
            s.dragging = false;
            ReleaseCapture();
            LRESULT(0)
        }
        WM_CAPTURECHANGED => {
            let s = st();
            s.text_dragging = false;
            s.dragging = false;
            LRESULT(0)
        }
        WM_MOUSEWHEEL => {
            let s = st();
            let delta = ((wparam.0 >> 16) as i16) as i32;
            let step = -delta / 120 * (CARD_H + CARD_GAP);
            let maxs = scroll_max(s.store.lock().unwrap().count(), list_top(s));
            s.scroll_y = (s.scroll_y + step).clamp(0, maxs);
            InvalidateRect(hwnd, None, false);
            LRESULT(0)
        }
        WM_KEYDOWN => {
            let mut handled = false;
            let s = st();
            if s.input_visible {
                let vk = wparam.0 as u16;
                let len = buf_char_count(&s.input_buffer);
                let shift = GetKeyState(VK_SHIFT.0 as i32) < 0;
                let ctrl = GetKeyState(VK_CONTROL.0 as i32) < 0;
                let old_caret = s.input_caret;
                let mut moved = false;
                if vk == VK_LEFT.0 {
                    if s.input_caret > 0 {
                        s.input_caret -= 1;
                    }
                    moved = true;
                } else if vk == VK_RIGHT.0 {
                    if s.input_caret < len {
                        s.input_caret += 1;
                    }
                    moved = true;
                } else if vk == VK_HOME.0 {
                    s.input_caret = 0;
                    moved = true;
                } else if vk == VK_END.0 {
                    s.input_caret = len;
                    moved = true;
                } else if vk == VK_DELETE.0 {
                    if !delete_selection(s) {
                        buf_remove_at(&mut s.input_buffer, s.input_caret);
                    }
                    s.sel_anchor = None;
                    handled = true;
                } else if vk == VK_A.0 && ctrl {
                    s.sel_anchor = Some(0);
                    s.input_caret = len;
                    handled = true;
                }
                if moved {
                    handled = true;
                    if shift {
                        if s.sel_anchor.is_none() {
                            s.sel_anchor = Some(old_caret);
                        }
                    } else {
                        s.sel_anchor = None;
                    }
                    if s.sel_anchor == Some(s.input_caret) {
                        s.sel_anchor = None;
                    }
                }
                if handled {
                    invalidate_input(hwnd);
                    reset_caret(hwnd, s);
                }
            }
            if handled {
                LRESULT(0)
            } else {
                DefWindowProcW(hwnd, msg, wparam, lparam)
            }
        }
        WM_CHAR => {
            let s = st();
            if s.input_visible {
                let c = wparam.0 as u32;
                match c {
                    13 => {
                        let title = s.input_buffer.trim().to_string();
                        if !title.is_empty() {
                            let date = s.input_date.clone();
                            let editing = s.editing_id.clone();
                            let mut store = s.store.lock().unwrap();
                            match editing {
                                Some(id) => {
                                    store.update(&id, &title, date.as_deref());
                                }
                                None => {
                                    store.add(&title, date.as_deref());
                                }
                            }
                        }
                        clear_input(s);
                        InvalidateRect(hwnd, None, false);
                        reset_caret(hwnd, s);
                    }
                    8 => {
                        if !delete_selection(s) && s.input_caret > 0 {
                            buf_remove_before(&mut s.input_buffer, s.input_caret);
                            s.input_caret -= 1;
                        }
                        s.sel_anchor = None;
                        invalidate_input(hwnd);
                        reset_caret(hwnd, s);
                    }
                    27 => {
                        clear_input(s);
                        InvalidateRect(hwnd, None, false);
                        reset_caret(hwnd, s);
                    }
                    _ => {
                        if let Some(ch) = char::from_u32(c) {
                            if !ch.is_control() {
                                delete_selection(s);
                                buf_insert(&mut s.input_buffer, s.input_caret, ch);
                                s.input_caret += 1;
                                s.sel_anchor = None;
                                invalidate_input(hwnd);
                                reset_caret(hwnd, s);
                            }
                        }
                    }
                }
            }
            LRESULT(0)
        }
        WM_ERASEBKGND => LRESULT(1),
        WM_DESTROY => LRESULT(0),
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

/// 打字时只需重绘输入框那一条区域，任务列表保持原样，避免整窗闪烁。
unsafe fn invalidate_input(hwnd: HWND) {
    let rc = RECT {
        left: 0,
        top: TITLE_H + HEADER_H,
        right: WIN_W,
        bottom: TITLE_H + HEADER_H + INPUT_H,
    };
    InvalidateRect(hwnd, Some(&rc), false);
}

unsafe fn paint(hwnd: HWND) {
    let s = st();
    let mut ps = PAINTSTRUCT::default();
    let _ = BeginPaint(hwnd, &mut ps);

    // 画入 32 位 DIB，再用 UpdateLayeredWindow 逐像素上屏，
    // 圆角由 alpha 渐变实现抗锯齿（窗口区域裁剪是二值的，做不到平滑边缘）
    let screen = GetDC(None);
    let mut bmi = BITMAPINFO::default();
    bmi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
    bmi.bmiHeader.biWidth = WIN_W;
    bmi.bmiHeader.biHeight = -WIN_H; // 自上而下
    bmi.bmiHeader.biPlanes = 1;
    bmi.bmiHeader.biBitCount = 32;
    bmi.bmiHeader.biCompression = BI_RGB.0 as u32;

    let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
    let bmp = CreateDIBSection(screen, &bmi, DIB_RGB_COLORS, &mut bits, None, 0).unwrap();
    let mem = CreateCompatibleDC(screen);
    let old_bmp = SelectObject(mem, bmp);

    let full = RECT { left: 0, top: 0, right: WIN_W, bottom: WIN_H };
    let bg = CreateSolidBrush(rgb(C_WHITE.0, C_WHITE.1, C_WHITE.2));
    FillRect(mem, &full, bg);
    DeleteObject(bg);

    // 标题栏
    let title_rc = RECT { left: 0, top: 0, right: WIN_W, bottom: TITLE_H };
    gdi::draw_text(mem, "待办事项", title_rc, rgb(C_MUTED.0, C_MUTED.1, C_MUTED.2),
        DT_CENTER | DT_VCENTER | DT_SINGLELINE, s.font_header);
    let close_rc = RECT { left: 0, top: 0, right: 45, bottom: TITLE_H };
    gdi::draw_text(mem, "×", close_rc, rgb(C_MUTED.0, C_MUTED.1, C_MUTED.2),
        DT_CENTER | DT_VCENTER | DT_SINGLELINE, s.font_btn);

    // header
    let hdr_y = TITLE_H;
    let hdr_title = RECT { left: 20, top: hdr_y, right: 200, bottom: hdr_y + HEADER_H };
    gdi::draw_text(mem, "我的待办", hdr_title, rgb(C_TEXT.0, C_TEXT.1, C_TEXT.2),
        DT_LEFT | DT_VCENTER | DT_SINGLELINE, s.font_title);
    let add_rc = RECT { left: WIN_W - 20 - 36, top: hdr_y + 10, right: WIN_W - 20, bottom: hdr_y + 10 + 36 };
    // 直接贴 ➕ emoji 彩色位图
    let pad = 6;
    let img_rc = RECT { left: add_rc.left + pad, top: add_rc.top + pad, right: add_rc.right - pad, bottom: add_rc.bottom - pad };
    gdi::blit_bgra(mem, img_rc, crate::emoji::PLUS_EMOJI, crate::emoji::PLUS_EMOJI_W, crate::emoji::PLUS_EMOJI_H);

    // 输入框
    if s.input_visible {
        let in_y = TITLE_H + HEADER_H;
        let in_rc = RECT { left: 20, top: in_y + 5, right: WIN_W - 20, bottom: in_y + INPUT_H - 5 };
        gdi::round_rect_aa(bits as *mut u32, WIN_W, full, in_rc, 10.0,
            rgb(C_CARD.0, C_CARD.1, C_CARD.2), rgb(C_PURPLE.0, C_PURPLE.1, C_PURPLE.2), 1.0);
        let text = if s.input_buffer.is_empty() { "输入新任务..." } else { &s.input_buffer };
        let color = if s.input_buffer.is_empty() { rgb(150, 150, 150) } else { rgb(C_TEXT.0, C_TEXT.1, C_TEXT.2) };
        let chip = chip_rect(s);
        let txt_rc = RECT { left: 20 + 12, top: in_y + 5, right: chip.left - 8, bottom: in_y + INPUT_H - 5 };

        // 选区高亮（铺在文字下方）
        if let Some((a, b)) = sel_range(s) {
            let old_font = SelectObject(mem, s.font_item);
            let mut sz_a = SIZE::default();
            let prefix_a: String = s.input_buffer.chars().take(a).collect();
            if !prefix_a.is_empty() {
                let wa: Vec<u16> = prefix_a.encode_utf16().collect();
                GetTextExtentPoint32W(mem, &wa, &mut sz_a);
            }
            let mut sz_b = SIZE::default();
            let prefix_b: String = s.input_buffer.chars().take(b).collect();
            if !prefix_b.is_empty() {
                let wb: Vec<u16> = prefix_b.encode_utf16().collect();
                GetTextExtentPoint32W(mem, &wb, &mut sz_b);
            }
            SelectObject(mem, old_font);
            let x0 = (20 + 12 + sz_a.cx).clamp(txt_rc.left, txt_rc.right);
            let x1 = (20 + 12 + sz_b.cx).clamp(txt_rc.left, txt_rc.right);
            let hl = RECT {
                left: x0.min(x1),
                top: in_y + 8,
                right: x0.max(x1),
                bottom: in_y + INPUT_H - 8,
            };
            let brush = CreateSolidBrush(rgb(214, 209, 245));
            FillRect(mem, &hl, brush);
            DeleteObject(brush);
        }

        gdi::draw_text(mem, text, txt_rc, color,
            DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS, s.font_item);

        // 日期胶囊：未选日期时显示「选择日期」，已选时显示 26-11-08
        let (chip_border, chip_text_color) = if s.input_date.is_some() {
            (rgb(C_PURPLE.0, C_PURPLE.1, C_PURPLE.2), rgb(C_PURPLE.0, C_PURPLE.1, C_PURPLE.2))
        } else {
            (rgb(C_BORDER.0, C_BORDER.1, C_BORDER.2), rgb(150, 150, 150))
        };
        gdi::round_rect_aa(bits as *mut u32, WIN_W, full, chip, 9.0,
            rgb(C_WHITE.0, C_WHITE.1, C_WHITE.2), chip_border, 1.0);
        let label = match &s.input_date {
            Some(d) => short_date(d),
            None => "选择日期".to_string(),
        };
        gdi::draw_text(mem, &label, chip, chip_text_color,
            DT_CENTER | DT_VCENTER | DT_SINGLELINE, s.font_date);

        // 自绘插入光标（1px 竖线，闪烁节奏与系统一致）
        if s.caret_on && caret_active(hwnd, s) {
            let mut sz = SIZE::default();
            let old_font = SelectObject(mem, s.font_item);
            let prefix: String = s.input_buffer.chars().take(s.input_caret).collect();
            let wide: Vec<u16> = prefix.encode_utf16().collect();
            if !wide.is_empty() {
                GetTextExtentPoint32W(mem, &wide, &mut sz);
            }
            SelectObject(mem, old_font);
            let ch = if sz.cy > 0 { sz.cy } else { 20 };
            let left = 20 + 12;
            let x = (left + sz.cx).clamp(left, txt_rc.right - 1);
            let cy = TITLE_H + HEADER_H;
            let caret_rc = RECT { left: x, top: cy + (INPUT_H - ch) / 2, right: x + 1, bottom: cy + (INPUT_H - ch) / 2 + ch };
            let brush = CreateSolidBrush(rgb(C_TEXT.0, C_TEXT.1, C_TEXT.2));
            FillRect(mem, &caret_rc, brush);
            DeleteObject(brush);
        }
    }

    // 任务列表（裁剪到列表区，支持滚轮滚动）
    let top = list_top(s);
    let tasks = s.store.lock().unwrap().all();
    let maxs = scroll_max(tasks.len(), top);
    if s.scroll_y > maxs { s.scroll_y = maxs; }
    let saved = SaveDC(mem);
    IntersectClipRect(mem, 0, top, WIN_W, WIN_H);
    let clip = RECT { left: 0, top, right: WIN_W, bottom: WIN_H };
    let mut y = top - s.scroll_y;
    for task in &tasks {
        draw_card(mem, bits as *mut u32, clip, s, task, y);
        y += CARD_H + CARD_GAP;
    }
    RestoreDC(mem, saved);

    gdi::round_corner_alpha(bits as *mut u32, WIN_W, WIN_H, CORNER_R);

    let mut rc = RECT::default();
    let _ = GetWindowRect(hwnd, &mut rc);
    let pt_dst = POINT { x: rc.left, y: rc.top };
    let pt_src = POINT { x: 0, y: 0 };
    let sz = SIZE { cx: WIN_W, cy: WIN_H };
    let blend = BLENDFUNCTION {
        BlendOp: 0,
        BlendFlags: 0,
        SourceConstantAlpha: 255,
        AlphaFormat: 1,
    };
    let _ = UpdateLayeredWindow(
        hwnd,
        None,
        Some(&pt_dst),
        Some(&sz),
        mem,
        Some(&pt_src),
        COLORREF(0),
        Some(&blend),
        ULW_ALPHA,
    );

    SelectObject(mem, old_bmp);
    DeleteObject(bmp);
    DeleteDC(mem);
    ReleaseDC(None, screen);

    EndPaint(hwnd, &ps);
}

unsafe fn draw_card(
    hdc: HDC,
    px: *mut u32,
    clip: RECT,
    s: &TaskWinState,
    task: &crate::store::Task,
    y: i32,
) {
    let bg = if task.completed { C_CARD_DONE } else { C_CARD };
    // 正在编辑的任务用紫色描边标记
    let editing = s.editing_id.as_deref() == Some(task.id.as_str());
    let border = if editing { C_PURPLE } else { C_BORDER };
    let card = RECT { left: MARGIN_X, top: y, right: WIN_W - MARGIN_X, bottom: y + CARD_H };
    gdi::round_rect_aa(px, WIN_W, clip, card, 12.0,
        rgb(bg.0, bg.1, bg.2), rgb(border.0, border.1, border.2),
        if editing { 1.5 } else { 1.0 });

    let cx = MARGIN_X + 15 + 11;
    let cy = y + CARD_H / 2;
    let chk = RECT { left: cx - 9, top: cy - 9, right: cx + 9, bottom: cy + 9 };
    if task.completed {
        gdi::fill_ellipse_aa(px, WIN_W, clip, chk, rgb(C_PURPLE.0, C_PURPLE.1, C_PURPLE.2));
    } else {
        gdi::stroke_ellipse_aa(px, WIN_W, clip, chk, 2.0, rgb(C_CHK_BORDER.0, C_CHK_BORDER.1, C_CHK_BORDER.2));
    }

    let text_color = if task.completed { C_MUTED } else { C_TEXT };
    let font = if task.completed { s.font_item_done } else { s.font_item };
    let date_txt = task.display_date();
    // 日期用 TextOut 精确右对齐（不做矩形裁剪，杜绝字形被切掉）
    let date_right = WIN_W - MARGIN_X - 24 - 12 - 6;
    let (date_w, date_h) = match &date_txt {
        Some(d) => {
            let mut sz = SIZE::default();
            let old = SelectObject(hdc, s.font_date);
            let wide: Vec<u16> = d.encode_utf16().collect();
            GetTextExtentPoint32W(hdc, &wide, &mut sz);
            SelectObject(hdc, old);
            (if sz.cx > 0 { sz.cx } else { 56 }, if sz.cy > 0 { sz.cy } else { 18 })
        }
        None => (0, 0),
    };
    let title_right = if date_txt.is_some() {
        date_right - date_w - 10
    } else {
        WIN_W - MARGIN_X - 50
    };
    let txt = RECT { left: MARGIN_X + 15 + 34, top: y, right: title_right, bottom: y + CARD_H };
    gdi::draw_text(hdc, &task.title, txt, rgb(text_color.0, text_color.1, text_color.2),
        DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS, font);

    // 设定了日期的任务，在标题右侧显示 26-11-08
    if let Some(d) = date_txt {
        let date_color = if task.completed { C_DEL } else { C_MUTED };
        gdi::text_out(hdc, &d, date_right - date_w, y + (CARD_H - date_h) / 2,
            rgb(date_color.0, date_color.1, date_color.2), s.font_date);
    }

    let del = RECT { left: WIN_W - MARGIN_X - 24 - 12, top: y, right: WIN_W - MARGIN_X, bottom: y + CARD_H };
    gdi::draw_text(hdc, "×", del, rgb(C_DEL.0, C_DEL.1, C_DEL.2),
        DT_CENTER | DT_VCENTER | DT_SINGLELINE, s.font_btn);
}

fn point_from_lparam(lp: LPARAM) -> POINT {
    let v = lp.0;
    POINT {
        x: (v & 0xFFFF) as i16 as i32,
        y: ((v >> 16) & 0xFFFF) as i16 as i32,
    }
}

/// 月份翻页。
fn shift_month(cal: &mut CalState, delta: i32) {
    let mut m = cal.view_month as i32 + delta;
    let mut y = cal.view_year;
    if m < 1 {
        m = 12;
        y -= 1;
    } else if m > 12 {
        m = 1;
        y += 1;
    }
    cal.view_month = m as u32;
    cal.view_year = y;
    cal.hover = CAL_HIT_NONE;
}

unsafe extern "system" fn cal_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if CAL.is_null() {
        return DefWindowProcW(hwnd, msg, wparam, lparam);
    }
    let cal = &mut *CAL;
    match msg {
        WM_PAINT => {
            cal_paint(hwnd);
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            let p = point_from_lparam(lparam);
            let hit = cal_hit(cal, p);
            if hit != cal.hover {
                cal.hover = hit;
                InvalidateRect(hwnd, None, false);
            }
            LRESULT(0)
        }
        WM_SETCURSOR => {
            if (lparam.0 & 0xFFFF) as u32 == HTCLIENT {
                let mut p = POINT::default();
                let _ = GetCursorPos(&mut p);
                let _ = ScreenToClient(hwnd, &mut p);
                let icon = if cal_hit(cal, p) != CAL_HIT_NONE { IDC_HAND } else { IDC_ARROW };
                SetCursor(LoadCursorW(None, icon).unwrap_or_default());
                return LRESULT(1);
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        WM_LBUTTONDOWN => {
            let p = point_from_lparam(lparam);
            let hit = cal_hit(cal, p);
            match hit {
                CAL_HIT_PREV => {
                    shift_month(cal, -1);
                    InvalidateRect(hwnd, None, false);
                }
                CAL_HIT_NEXT => {
                    shift_month(cal, 1);
                    InvalidateRect(hwnd, None, false);
                }
                CAL_HIT_TODAY => apply_date(Some(today())),
                CAL_HIT_CLEAR => apply_date(None),
                idx if idx >= 0 => {
                    if let Some(day) = cal_day_at(cal, idx) {
                        let iso = format!("{:04}-{:02}-{:02}", cal.view_year, cal.view_month, day);
                        apply_date(Some(iso));
                    }
                }
                _ => {}
            }
            LRESULT(0)
        }
        WM_KEYDOWN => {
            if wparam.0 as u16 == VK_ESCAPE.0 {
                let main = st().hwnd;
                hide_calendar();
                SetForegroundWindow(main);
                SetFocus(main);
            }
            LRESULT(0)
        }
        // 点到别处（任务窗口或其他程序）时收起弹层
        WM_ACTIVATE => {
            if (wparam.0 & 0xFFFF) as u32 == WA_INACTIVE {
                hide_calendar();
            }
            LRESULT(0)
        }
        WM_ERASEBKGND => LRESULT(1),
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

unsafe fn cal_paint(hwnd: HWND) {
    if CAL.is_null() {
        return;
    }
    let cal = &mut *CAL;
    let mut ps = PAINTSTRUCT::default();
    let _ = BeginPaint(hwnd, &mut ps);

    let screen = GetDC(None);
    let mut bmi = BITMAPINFO::default();
    bmi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
    bmi.bmiHeader.biWidth = CAL_W;
    bmi.bmiHeader.biHeight = -CAL_H; // 自上而下
    bmi.bmiHeader.biPlanes = 1;
    bmi.bmiHeader.biBitCount = 32;
    bmi.bmiHeader.biCompression = BI_RGB.0 as u32;

    let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
    let bmp = CreateDIBSection(screen, &bmi, DIB_RGB_COLORS, &mut bits, None, 0).unwrap();
    let mem = CreateCompatibleDC(screen);
    let old_bmp = SelectObject(mem, bmp);

    let full = RECT { left: 0, top: 0, right: CAL_W, bottom: CAL_H };
    let bg = CreateSolidBrush(rgb(C_WHITE.0, C_WHITE.1, C_WHITE.2));
    FillRect(mem, &full, bg);
    DeleteObject(bg);
    gdi::round_rect_aa(bits as *mut u32, CAL_W, full, full, 12.0,
        rgb(C_WHITE.0, C_WHITE.1, C_WHITE.2), rgb(C_BORDER.0, C_BORDER.1, C_BORDER.2), 1.0);

    let purple = rgb(C_PURPLE.0, C_PURPLE.1, C_PURPLE.2);
    let muted = rgb(C_MUTED.0, C_MUTED.1, C_MUTED.2);
    let text = rgb(C_TEXT.0, C_TEXT.1, C_TEXT.2);

    // 月份导航（矢量抗锯齿箭头，避免字形锯齿）
    let prev = cal_prev_rect();
    let next = cal_next_rect();
    gdi::draw_chevron_aa(bits as *mut u32, CAL_W, full, prev,
        true, 2.4, if cal.hover == CAL_HIT_PREV { purple } else { muted });
    gdi::draw_chevron_aa(bits as *mut u32, CAL_W, full, next,
        false, 2.4, if cal.hover == CAL_HIT_NEXT { purple } else { muted });
    let title = format!("{}年{}月", cal.view_year, cal.view_month);
    let trc = RECT {
        left: CAL_PAD + 30,
        top: CAL_HDR_Y,
        right: CAL_W - CAL_PAD - 30,
        bottom: CAL_HDR_Y + CAL_HDR_H,
    };
    gdi::draw_text(mem, &title, trc, text, DT_CENTER | DT_VCENTER | DT_SINGLELINE, cal.font_hdr);

    // 星期行
    let names = ["日", "一", "二", "三", "四", "五", "六"];
    for (i, n) in names.iter().enumerate() {
        let rc = RECT {
            left: CAL_PAD + i as i32 * CAL_CELL,
            top: CAL_GRID_Y - CAL_WEEK_H,
            right: CAL_PAD + (i as i32 + 1) * CAL_CELL,
            bottom: CAL_GRID_Y,
        };
        let c = if i == 0 || i == 6 { rgb(C_WEEKEND.0, C_WEEKEND.1, C_WEEKEND.2) } else { muted };
        gdi::draw_text(mem, n, rc, c, DT_CENTER | DT_VCENTER | DT_SINGLELINE, cal.font_week);
    }

    // 日期网格
    let today_d = Local::now().date_naive();
    let sel = st()
        .input_date
        .as_deref()
        .and_then(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok());
    for idx in 0..CAL_ROWS * 7 {
        let day = match cal_day_at(cal, idx) {
            Some(d) => d,
            None => continue,
        };
        let rc = cal_day_rect(idx);
        let inner = RECT { left: rc.left + 3, top: rc.top + 3, right: rc.right - 3, bottom: rc.bottom - 3 };
        if cal.hover == idx {
            gdi::round_rect_aa(bits as *mut u32, CAL_W, full, inner, 8.0,
                rgb(C_HOVER.0, C_HOVER.1, C_HOVER.2), rgb(C_HOVER.0, C_HOVER.1, C_HOVER.2), 0.0);
        }
        let is_sel = sel.map_or(false, |d| {
            d.year() == cal.view_year && d.month() == cal.view_month && d.day() == day
        });
        let is_today = today_d.year() == cal.view_year
            && today_d.month() == cal.view_month
            && today_d.day() == day;
        if is_sel {
            gdi::fill_ellipse_aa(bits as *mut u32, CAL_W, full, inner, purple);
            gdi::draw_text(mem, &day.to_string(), rc, rgb(C_WHITE.0, C_WHITE.1, C_WHITE.2),
                DT_CENTER | DT_VCENTER | DT_SINGLELINE, cal.font_day);
        } else {
            let c = if is_today {
                purple
            } else if idx % 7 == 0 || idx % 7 == 6 {
                rgb(C_WEEKEND.0, C_WEEKEND.1, C_WEEKEND.2)
            } else {
                text
            };
            gdi::draw_text(mem, &day.to_string(), rc, c,
                DT_CENTER | DT_VCENTER | DT_SINGLELINE, cal.font_day);
        }
    }

    // 底部：「今天」/「不设日期」
    for (i, label) in ["今天", "不设日期"].iter().enumerate() {
        let rc = cal_footer_rect(i as i32);
        let hovered = cal.hover == CAL_HIT_TODAY + i as i32;
        gdi::round_rect_aa(bits as *mut u32, CAL_W, full, rc, 8.0,
            rgb(C_CARD.0, C_CARD.1, C_CARD.2),
            if hovered { purple } else { rgb(C_BORDER.0, C_BORDER.1, C_BORDER.2) },
            1.0);
        gdi::draw_text(mem, label, rc, if hovered { purple } else { muted },
            DT_CENTER | DT_VCENTER | DT_SINGLELINE, cal.font_week);
    }

    gdi::round_corner_alpha(bits as *mut u32, CAL_W, CAL_H, 12);

    let mut rc = RECT::default();
    let _ = GetWindowRect(hwnd, &mut rc);
    let pt_dst = POINT { x: rc.left, y: rc.top };
    let pt_src = POINT { x: 0, y: 0 };
    let sz = SIZE { cx: CAL_W, cy: CAL_H };
    let blend = BLENDFUNCTION {
        BlendOp: 0,
        BlendFlags: 0,
        SourceConstantAlpha: 255,
        AlphaFormat: 1,
    };
    let _ = UpdateLayeredWindow(
        hwnd,
        None,
        Some(&pt_dst),
        Some(&sz),
        mem,
        Some(&pt_src),
        COLORREF(0),
        Some(&blend),
        ULW_ALPHA,
    );

    SelectObject(mem, old_bmp);
    DeleteObject(bmp);
    DeleteDC(mem);
    ReleaseDC(None, screen);
    EndPaint(hwnd, &ps);
}
