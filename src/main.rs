#![windows_subsystem = "windows"]

use anyhow::{ensure, Context, Error, Result};
use image::{self, imageops, DynamicImage};
use std::env;
use std::ffi::c_void;
use std::mem;
use std::path::Path;
use std::ptr;
use windows::{
    core::{PCWSTR, PWSTR},
    w,
    Win32::{
        Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM},
        Graphics::Gdi::{
            BeginPaint, BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, CreateFontW, DeleteDC,
            DeleteObject, EndPaint, GetSysColorBrush, InvalidateRect, SelectObject, SetDIBits,
            UpdateWindow, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, CLIP_DEFAULT_PRECIS, COLOR_MENUBAR,
            DEFAULT_CHARSET, DEFAULT_PITCH, DEFAULT_QUALITY, DIB_RGB_COLORS, FF_DONTCARE, HFONT,
            OUT_DEFAULT_PRECIS, PAINTSTRUCT, SRCCOPY,
        },
        UI::{
            Controls::Dialogs::{GetOpenFileNameW, OFN_FILEMUSTEXIST, OPENFILENAMEW},
            WindowsAndMessaging::{
                CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, LoadCursorW,
                MessageBoxW, PostQuitMessage, RegisterClassW, SendMessageW, SetWindowTextW,
                ShowWindow, TranslateMessage, BN_CLICKED, BS_PUSHBUTTON, CW_USEDEFAULT, HMENU,
                IDI_APPLICATION, MB_OK, MSG, SW_SHOW, WINDOW_EX_STYLE, WINDOW_STYLE, WM_COMMAND,
                WM_CREATE, WM_DESTROY, WM_PAINT, WM_SETFONT, WNDCLASSW, WS_CAPTION, WS_CHILD,
                WS_OVERLAPPED, WS_SYSMENU, WS_VISIBLE,
            },
        },
    },
};

mod lz4i_decoder;
use lz4i_decoder::read_lz4i;

const CLASS_NAME: PCWSTR = w!("pinion_window_class");

static mut H_WINDOW: Option<HWND> = None;
static mut H_FONT: Option<HFONT> = None;
static mut BUF: Vec<u8> = Vec::new();
static mut DATA_LEN: usize = 0;
static mut WIDTH: i32 = 0;
static mut HEIGHT: i32 = 0;

const ID_OPEN_BUTTON: i32 = 2100;

fn main() -> Result<()> {
    let wnd_class = WNDCLASSW {
        lpszClassName: CLASS_NAME,
        lpfnWndProc: Some(window_proc),
        hCursor: unsafe { LoadCursorW(None, IDI_APPLICATION)? },
        hbrBackground: unsafe { GetSysColorBrush(COLOR_MENUBAR) },
        ..Default::default()
    };
    unsafe { RegisterClassW(&wnd_class) };

    let title = format!("{} v{}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));
    let hwnd = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            CLASS_NAME,
            PCWSTR::from_raw(l(&title).as_ptr()),
            WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_VISIBLE,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            656,
            551,
            None,
            None,
            None,
            None,
        )
    };
    ensure!(hwnd.0 != 0, "failed to create window.");

    unsafe { BUF.reserve(640 * 480 * 3) };

    unsafe {
        ShowWindow(hwnd, SW_SHOW);
        UpdateWindow(hwnd);
    }

    unsafe { H_WINDOW = Some(hwnd) };

    let mut msg = MSG::default();
    loop {
        if unsafe { !GetMessageW(&mut msg, None, 0, 0).as_bool() } {
            break;
        }
        unsafe {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    Ok(())
}

unsafe extern "system" fn window_proc(
    h_wnd: HWND,
    msg: u32,
    w_param: WPARAM,
    l_param: LPARAM,
) -> LRESULT {
    match msg {
        WM_CREATE => create(h_wnd),
        WM_COMMAND => command(h_wnd, w_param),
        WM_PAINT => {
            if DATA_LEN > 0 {
                paint(h_wnd)
            } else {
                return DefWindowProcW(h_wnd, msg, w_param, l_param);
            }
        }
        WM_DESTROY => {
            if let Some(font) = H_FONT {
                DeleteObject(font);
            }
            PostQuitMessage(0);
            Ok(())
        }
        _ => return DefWindowProcW(h_wnd, msg, w_param, l_param),
    }
    .map_err(msg_box)
    .ok();

    LRESULT::default()
}

fn create(h_wnd: HWND) -> Result<()> {
    create_font()?;
    create_button(h_wnd)?;
    Ok(())
}

fn create_font() -> Result<()> {
    let font = unsafe {
        CreateFontW(
            18,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            DEFAULT_CHARSET.0 as u32,
            OUT_DEFAULT_PRECIS.0 as u32,
            CLIP_DEFAULT_PRECIS.0 as u32,
            DEFAULT_QUALITY.0 as u32,
            DEFAULT_PITCH.0 as u32 | FF_DONTCARE.0 as u32,
            w!("メイリオ"),
        )
    };
    ensure!(!font.is_invalid(), "CreateFontW failed.");
    unsafe { H_FONT = Some(font) };
    Ok(())
}

fn create_button(h_wnd: HWND) -> Result<()> {
    let h_button = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            w!("BUTTON"),
            w!("Open"),
            WS_CHILD | WS_VISIBLE | WINDOW_STYLE(BS_PUSHBUTTON as u32),
            4,
            4,
            80,
            24,
            h_wnd,
            HMENU(ID_OPEN_BUTTON as isize),
            None,
            None,
        )
    };
    unsafe {
        SendMessageW(
            h_button,
            WM_SETFONT,
            WPARAM(H_FONT.context("no font")?.0 as usize),
            LPARAM::default(),
        )
    };
    Ok(())
}

fn command(h_wnd: HWND, w_param: WPARAM) -> Result<()> {
    let msg = (w_param.0 as u32) >> 16;
    let id = ((w_param.0 as u32) & 0xffff) as i32;
    if msg == BN_CLICKED && id == ID_OPEN_BUTTON {
        let file_path = open_dialog(h_wnd)?;
        read_image(&file_path)?;
    }
    Ok(())
}

fn open_image(file_path: &str) -> Result<DynamicImage> {
    let path = Path::new(file_path);
    if path.extension().context("no extension")?.eq("lz4i") {
        read_lz4i(file_path)
    } else {
        Ok(image::open(file_path)?)
    }
}

fn read_image(file_path: &str) -> Result<()> {
    let img = open_image(file_path)?;
    let width = img.width();
    let height = img.height();

    let img = if width > 640 || height > 480 {
        let new_size = if width as f32 / height as f32 > 1.333 {
            640
        } else if width > height {
            (480.0 / height as f32 * width as f32) as u32
        } else {
            480
        };
        img.resize(new_size, new_size, imageops::Lanczos3)
    } else {
        img
    };

    let width = img.width();
    let height = img.height();
    let mut rgb = img.into_rgb8();
    ensure!(rgb.len() <= 640 * 480 * 3, "Invalid data length.");

    // change from RGB to BGR.
    rgb.chunks_mut(3).for_each(|c| c.swap(0, 2));

    let remain = (3 * width as usize) % 4;

    if remain > 0 {
        let scan_line = 3 * width as usize;
        let scan_line_with_padding = scan_line + 4 - remain;
        let data_len = scan_line_with_padding * height as usize;
        let mut p = unsafe { BUF.as_mut_ptr() };
        rgb.chunks(scan_line).for_each(|c| unsafe {
            ptr::copy_nonoverlapping(c.as_ptr(), p, scan_line);
            p = p.add(scan_line_with_padding);
        });
        unsafe { DATA_LEN = data_len };
    } else {
        let data_len = (width * height * 3) as usize;
        unsafe {
            DATA_LEN = data_len;
            ptr::copy_nonoverlapping(rgb.as_ptr(), BUF.as_mut_ptr(), data_len);
        }
    };

    let rc = RECT {
        top: 32,
        left: 0,
        right: 640,
        bottom: 512,
    };
    unsafe {
        let win = H_WINDOW.context("no window")?;
        InvalidateRect(win, Some(&rc), true);
        SetWindowTextW(win, PCWSTR::from_raw(l(file_path).as_ptr()));
        WIDTH = width as i32;
        HEIGHT = height as i32;
    }
    Ok(())
}

fn open_dialog(h_wnd: HWND) -> Result<String> {
    const MAX_PATH: u32 = 260;
    let mut buf = [0u16; MAX_PATH as usize];

    let filter = w!("Image file (jpg, png, gif, bmp, lz4i)\0*.jpg;*.png;*.gif;*.bmp;*.lz4i\0");
    let title = w!("Choose a image file");

    let mut ofn = OPENFILENAMEW {
        lStructSize: mem::size_of::<OPENFILENAMEW>() as u32,
        lpstrFilter: filter,
        lpstrTitle: title,
        lpstrFile: PWSTR::from_raw(buf.as_mut_ptr()),
        nMaxFile: MAX_PATH,
        Flags: OFN_FILEMUSTEXIST,
        hwndOwner: h_wnd,
        ..Default::default()
    };

    ensure!(
        unsafe { GetOpenFileNameW(&mut ofn).as_bool() },
        "Cannot get file path."
    );

    let result = unsafe { ofn.lpstrFile.to_string()? };
    Ok(result)
}

fn paint(h_wnd: HWND) -> Result<()> {
    let mut ps = PAINTSTRUCT::default();
    let hdc = unsafe { BeginPaint(h_wnd, &mut ps) };

    let bi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: unsafe { WIDTH },
            biHeight: unsafe { -HEIGHT },
            biPlanes: 1,
            biBitCount: 24,
            biCompression: BI_RGB.0 as u32,
            biSizeImage: unsafe { DATA_LEN as u32 },
            ..Default::default()
        },
        ..Default::default()
    };

    let h_bmp = unsafe { CreateCompatibleBitmap(hdc, WIDTH, HEIGHT) };

    unsafe {
        SetDIBits(
            hdc,
            h_bmp,
            0,
            HEIGHT as u32,
            BUF.as_ptr() as *const c_void,
            &bi,
            DIB_RGB_COLORS,
        )
    };
    let h_mdc = unsafe { CreateCompatibleDC(hdc) };
    unsafe { SelectObject(h_mdc, h_bmp) };

    unsafe {
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
        DeleteObject(h_bmp);
        EndPaint(h_wnd, &ps);
    }
    Ok(())
}

fn msg_box(e: Error) -> Result<()> {
    unsafe {
        MessageBoxW(
            H_WINDOW.context("no window")?,
            PCWSTR::from_raw(l(&e.to_string()).as_ptr()),
            w!("Error"),
            MB_OK,
        )
    };
    Ok(())
}

fn l(source: &str) -> Vec<u16> {
    source.encode_utf16().chain(Some(0)).collect()
}
