#![windows_subsystem = "windows"]

use anyhow::{anyhow, ensure, Error, Result};
use image::{self, imageops};
use std::cell::{Ref, RefCell, RefMut};
use std::env;
use std::ffi::c_void;
use std::mem;
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

const CLASS_NAME: PCWSTR = w!("pinion_window_class");

static H_WINDOW: Global<HWND> = Global::new();
static H_FONT: Global<HFONT> = Global::new();
static BUF: Global<Vec<u8>> = Global::new();
static DATA_LEN: Global<usize> = Global::new();
static WIDTH: Global<i32> = Global::new();
static HEIGHT: Global<i32> = Global::new();

struct Global<T>(RefCell<Option<T>>);
unsafe impl<T> Sync for Global<T> {}

impl<T> Global<T> {
    const fn new() -> Self {
        Self(RefCell::new(None))
    }

    fn borrow(&self) -> Ref<Option<T>> {
        self.0.borrow()
    }

    fn borrow_mut(&self) -> RefMut<Option<T>> {
        self.0.borrow_mut()
    }
}

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

    let mut v = Vec::new();
    v.reserve(640 * 480 * 3);
    *BUF.borrow_mut() = Some(v);

    unsafe {
        ShowWindow(hwnd, SW_SHOW);
        UpdateWindow(hwnd);
    }

    *H_WINDOW.borrow_mut() = Some(hwnd);

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
            if DATA_LEN.borrow().gt(&Some(0)) {
                paint(h_wnd)
            } else {
                return DefWindowProcW(h_wnd, msg, w_param, l_param);
            }
        }
        WM_DESTROY => {
            if let Some(font) = *H_FONT.borrow() {
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
    *H_FONT.borrow_mut() = Some(font);
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
    let Some(font) = *H_FONT.borrow() else { return  Err(anyhow!("no font.")) };
    unsafe {
        SendMessageW(
            h_button,
            WM_SETFONT,
            WPARAM(font.0 as usize),
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

fn read_image(file_path: &str) -> Result<()> {
    let img = image::open(file_path)?;
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
    let mut buf = BUF.borrow_mut();
    let Some(buf) = buf.as_mut() else { return Err(anyhow!("no buffer.")) };

    if remain > 0 {
        let scan_line = 3 * width as usize;
        let scan_line_with_padding = scan_line + 4 - remain;
        let data_len = scan_line_with_padding * height as usize;
        let mut p = buf.as_mut_ptr();
        rgb.chunks(scan_line).for_each(|c| unsafe {
            ptr::copy_nonoverlapping(c.as_ptr(), p, scan_line);
            p = p.add(scan_line_with_padding);
        });
        *DATA_LEN.borrow_mut() = Some(data_len);
    } else {
        let data_len = (width * height * 3) as usize;
        *DATA_LEN.borrow_mut() = Some(data_len);
        unsafe {
            ptr::copy_nonoverlapping(rgb.as_ptr(), buf.as_mut_ptr(), data_len);
        }
    };

    let rc = RECT {
        top: 32,
        left: 0,
        right: 640,
        bottom: 512,
    };
    let hwnd = *H_WINDOW.borrow();
    unsafe {
        InvalidateRect(hwnd, Some(&rc), true);
        SetWindowTextW(hwnd, PCWSTR::from_raw(l(file_path).as_ptr()));
    }
    *WIDTH.borrow_mut() = Some(width as i32);
    *HEIGHT.borrow_mut() = Some(height as i32);
    Ok(())
}

fn open_dialog(h_wnd: HWND) -> Result<String> {
    const MAX_PATH: u32 = 260;
    let mut buf = [0u16; MAX_PATH as usize];

    let filter = w!("Image file\0*.jpg;*.png;*.gif;*.bmp\0");
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

    let Some(data_len) = *DATA_LEN.borrow() else { return Err(anyhow!("no data_len")) };
    let Some(width) = *WIDTH.borrow() else { return Err(anyhow!("no width.")) };
    let Some(height) = *HEIGHT.borrow() else { return Err(anyhow!("no height.")) };

    let bi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width,
            biHeight: -height,
            biPlanes: 1,
            biBitCount: 24,
            biCompression: BI_RGB,
            biSizeImage: data_len as u32,
            ..Default::default()
        },
        ..Default::default()
    };

    let h_bmp = unsafe { CreateCompatibleBitmap(hdc, width, height) };

    let Some(ref buf) = *BUF.borrow() else { return Err(anyhow!("no buffer.")) };
    unsafe {
        SetDIBits(
            hdc,
            h_bmp,
            0,
            height as u32,
            buf.as_ptr() as *const c_void,
            &bi,
            DIB_RGB_COLORS,
        )
    };
    let h_mdc = unsafe { CreateCompatibleDC(hdc) };
    unsafe { SelectObject(h_mdc, h_bmp) };

    let padding_left = (640 - width) / 2;
    let padding_top = (480 - height) / 2;
    unsafe {
        BitBlt(
            hdc,
            padding_left,
            padding_top + 32,
            width,
            height,
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
    let hwnd = *H_WINDOW.borrow();
    unsafe {
        MessageBoxW(
            hwnd,
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
