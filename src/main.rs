// #![windows_subsystem = "windows"]

use anyhow::{ensure, Error, Result};
use image::{self, imageops, GenericImageView};
use std::char::{decode_utf16, REPLACEMENT_CHARACTER};
use std::env;
use std::mem;
use std::ptr;
use std::slice;
use winapi::{
    ctypes::c_void,
    shared::{
        minwindef::{HIWORD, LOWORD, LPARAM, LRESULT, TRUE, UINT, WPARAM},
        windef::{HFONT, HMENU, HWND, RECT},
    },
    um::{
        commdlg::{GetOpenFileNameW, OFN_FILEMUSTEXIST, OPENFILENAMEW},
        wingdi::{
            BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, CreateFontW, DeleteDC,
            DeleteObject, SelectObject, SetDIBits, BITMAPINFO, BITMAPINFOHEADER, BI_RGB,
            CLIP_DEFAULT_PRECIS, DEFAULT_CHARSET, DEFAULT_PITCH, DEFAULT_QUALITY, DIB_RGB_COLORS,
            FF_DONTCARE, OUT_DEFAULT_PRECIS, SRCCOPY,
        },
        winuser::{
            BeginPaint, CreateWindowExW, DefWindowProcW, DispatchMessageW, EndPaint, GetMessageW,
            GetSysColorBrush, InvalidateRect, LoadCursorW, LoadIconW, MessageBoxW, PostQuitMessage,
            RegisterClassW, SendMessageW, ShowWindow, TranslateMessage, UpdateWindow, BN_CLICKED,
            BS_PUSHBUTTON, COLOR_MENUBAR, CW_USEDEFAULT, IDI_APPLICATION, MB_OK, MSG, PAINTSTRUCT,
            SW_SHOW, WM_COMMAND, WM_CREATE, WM_DESTROY, WM_PAINT, WM_SETFONT, WNDCLASSW,
            WS_CAPTION, WS_CHILD, WS_OVERLAPPED, WS_SYSMENU, WS_VISIBLE,
        },
    },
};

static mut H_WINDOW: HWND = ptr::null_mut();
static mut H_FONT: HFONT = ptr::null_mut();
static mut BUF: Vec<u8> = Vec::new();
static mut BUF_LEN: usize = 0;
static mut WIDTH: i32 = 0;
static mut HEIGHT: i32 = 0;

const ID_OPEN_BUTTON: i32 = 2100;

fn main() -> Result<()> {
    unsafe {
        let class_name = l("pinion_window_class");
        let wnd_class = WNDCLASSW {
            style: 0,
            lpfnWndProc: Some(window_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: ptr::null_mut(),
            hIcon: LoadIconW(ptr::null_mut(), IDI_APPLICATION),
            hCursor: LoadCursorW(ptr::null_mut(), IDI_APPLICATION),
            hbrBackground: GetSysColorBrush(COLOR_MENUBAR),
            lpszMenuName: ptr::null_mut(),
            lpszClassName: class_name.as_ptr(),
        };
        RegisterClassW(&wnd_class);

        let title = format!("{} v{}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));
        H_WINDOW = CreateWindowExW(
            0,
            class_name.as_ptr(),
            l(&title).as_ptr(),
            WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_VISIBLE,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            656,
            551,
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
        );
        ensure!(!H_WINDOW.is_null(), "CreateWindowExW failed.");

        BUF.reserve(640 * 480 * 3);

        ShowWindow(H_WINDOW, SW_SHOW);
        UpdateWindow(H_WINDOW);
        let mut msg = init::<MSG>();
        loop {
            if GetMessageW(&mut msg, ptr::null_mut(), 0, 0) == 0 {
                break;
            }
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    Ok(())
}

unsafe extern "system" fn window_proc(
    h_wnd: HWND,
    msg: UINT,
    w_param: WPARAM,
    l_param: LPARAM,
) -> LRESULT {
    match msg {
        WM_CREATE => create(h_wnd),
        WM_COMMAND => command(h_wnd, w_param),
        WM_PAINT => {
            if BUF_LEN > 0 {
                paint(h_wnd)
            } else {
                return DefWindowProcW(h_wnd, msg, w_param, l_param);
            }
        }
        WM_DESTROY => {
            DeleteObject(H_FONT as *mut c_void);
            PostQuitMessage(0);
            Ok(())
        }
        _ => return DefWindowProcW(h_wnd, msg, w_param, l_param),
    }
    .map_err(msg_box)
    .ok();
    0
}

unsafe fn create(h_wnd: HWND) -> Result<()> {
    create_font()?;
    create_button(h_wnd)?;
    Ok(())
}

unsafe fn create_font() -> Result<()> {
    H_FONT = CreateFontW(
        18,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        DEFAULT_CHARSET,
        OUT_DEFAULT_PRECIS,
        CLIP_DEFAULT_PRECIS,
        DEFAULT_QUALITY,
        DEFAULT_PITCH | FF_DONTCARE,
        l("メイリオ").as_ptr(),
    );
    ensure!(!H_FONT.is_null(), "CreateFontW failed.");
    Ok(())
}

unsafe fn create_button(h_wnd: HWND) -> Result<()> {
    let h_button = CreateWindowExW(
        0,
        l("BUTTON").as_ptr(),
        l("Open").as_ptr(),
        WS_CHILD | WS_VISIBLE | BS_PUSHBUTTON,
        4,
        4,
        80,
        24,
        h_wnd,
        ID_OPEN_BUTTON as HMENU,
        ptr::null_mut(),
        ptr::null_mut(),
    );
    ensure!(!h_button.is_null(), "CreateWindowExW BUTTON failed.",);
    SendMessageW(h_button, WM_SETFONT, H_FONT as WPARAM, 0);
    Ok(())
}

unsafe fn command(h_wnd: HWND, w_param: WPARAM) -> Result<()> {
    let msg = HIWORD(w_param as u32);
    let id = LOWORD(w_param as u32) as i32;
    if msg == BN_CLICKED {
        if id == ID_OPEN_BUTTON {
            let file_path = open_dialog(h_wnd)?;
            read_image(&file_path)?;
        }
    }
    Ok(())
}

unsafe fn read_image(file_path: &str) -> Result<()> {
    let img = image::open(file_path)?;
    let img = if img.width() > 640 || img.height() > 480 {
        let new_size = if img.width() as f32 / img.height() as f32 > 1.333 {
            640
        } else {
            if img.width() > img.height() {
                (480.0 / img.height() as f32 * img.width() as f32) as u32
            } else {
                480
            }
        };
        img.resize(new_size, new_size, imageops::Lanczos3)
    } else {
        img
    };
    WIDTH = img.width() as i32;
    HEIGHT = img.height() as i32;

    let bgr = img.into_bgr();
    ensure!(bgr.len() <= 640 * 480 * 3, "Invalid data length.");

    let remain = (3 * WIDTH as usize) % 4;
    if remain > 0 {
        let chunk_size = 3 * WIDTH as usize;
        let line_bytes_len = chunk_size + 4 - remain;
        BUF_LEN = line_bytes_len * HEIGHT as usize;
        let mut p = BUF.as_mut_ptr();
        bgr.chunks(chunk_size).for_each(|c| {
            ptr::copy_nonoverlapping(c.as_ptr(), p, chunk_size);
            p = p.add(line_bytes_len);
        });
    } else {
        BUF_LEN = (WIDTH * HEIGHT * 3) as usize;
        ptr::copy_nonoverlapping(bgr.as_ptr(), BUF.as_mut_ptr(), BUF_LEN);
    };

    let rc = RECT {
        top: 32,
        left: 0,
        right: 640,
        bottom: 512,
    };
    InvalidateRect(H_WINDOW, &rc, TRUE);
    Ok(())
}

unsafe fn open_dialog(h_wnd: HWND) -> Result<String> {
    const MAX_PATH: u32 = 260;
    let mut buf = [0u16; MAX_PATH as usize];

    let filter = l("Image file\0*.jpg;*.png;*.gif;*.bmp\0");
    let title = l("Choose a image file");

    let mut ofn = zeroed::<OPENFILENAMEW>();
    ofn.lStructSize = mem::size_of::<OPENFILENAMEW>() as u32;
    ofn.lpstrFilter = filter.as_ptr();
    ofn.lpstrTitle = title.as_ptr();
    ofn.lpstrFile = buf.as_mut_ptr();
    ofn.nMaxFile = MAX_PATH;
    ofn.Flags = OFN_FILEMUSTEXIST;
    ofn.hwndOwner = h_wnd;

    ensure!(GetOpenFileNameW(&mut ofn) != 0, "Cannot get file path.");

    let slice = slice::from_raw_parts(ofn.lpstrFile, MAX_PATH as usize);
    Ok(decode(slice))
}

unsafe fn paint(h_wnd: HWND) -> Result<()> {
    let mut ps = init::<PAINTSTRUCT>();
    let hdc = BeginPaint(h_wnd, &mut ps);

    let mut bi = zeroed::<BITMAPINFO>();
    bi.bmiHeader = zeroed::<BITMAPINFOHEADER>();
    bi.bmiHeader.biSize = mem::size_of::<BITMAPINFOHEADER>() as u32;
    bi.bmiHeader.biWidth = WIDTH;
    bi.bmiHeader.biHeight = -HEIGHT;
    bi.bmiHeader.biPlanes = 1;
    bi.bmiHeader.biBitCount = 24;
    bi.bmiHeader.biSizeImage = BUF_LEN as u32;
    bi.bmiHeader.biCompression = BI_RGB;

    let h_bmp = CreateCompatibleBitmap(hdc, WIDTH, HEIGHT);

    SetDIBits(
        hdc,
        h_bmp,
        0,
        HEIGHT as u32,
        BUF.as_ptr() as *const c_void,
        &bi,
        DIB_RGB_COLORS,
    );
    let h_mdc = CreateCompatibleDC(hdc);
    SelectObject(h_mdc, h_bmp as *mut c_void);

    let padding_left = (640 - WIDTH) / 2;
    let padding_top = (480 - HEIGHT) / 2;
    BitBlt(
        hdc,
        padding_left,
        padding_top + 32,
        WIDTH,
        HEIGHT,
        h_mdc,
        0,
        0,
        SRCCOPY,
    );
    DeleteDC(h_mdc);
    DeleteObject(h_bmp as *mut c_void);
    EndPaint(h_wnd, &ps);
    Ok(())
}

fn msg_box(e: Error) {
    unsafe {
        MessageBoxW(
            H_WINDOW,
            l(&e.to_string()).as_ptr(),
            l("Error").as_ptr(),
            MB_OK,
        )
    };
}

fn l(source: &str) -> Vec<u16> {
    source.encode_utf16().chain(Some(0)).collect()
}

fn decode(source: &[u16]) -> String {
    decode_utf16(source.iter().take_while(|&n| n != &0).cloned())
        .map(|r| r.unwrap_or(REPLACEMENT_CHARACTER))
        .collect()
}

unsafe fn init<T>() -> T {
    mem::MaybeUninit::<T>::uninit().assume_init()
}

unsafe fn zeroed<T>() -> T {
    mem::MaybeUninit::<T>::zeroed().assume_init()
}
