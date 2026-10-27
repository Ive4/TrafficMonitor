//! DirectWrite text with explicit color-font opt-in and a caller-controlled GDI fallback.
//! https://learn.microsoft.com/windows/win32/directwrite/color-fonts
use std::cell::RefCell;
use windows::{
    Win32::{
        Foundation::RECT,
        Graphics::{
            Direct2D::{Common::*, *},
            DirectWrite::*,
            Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM,
            Gdi::HDC,
        },
    },
    core::PCWSTR,
};
struct Renderer {
    target: ID2D1DCRenderTarget,
    write: IDWriteFactory,
}
impl Renderer {
    fn new() -> windows::core::Result<Self> {
        unsafe {
            let factory: ID2D1Factory = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
            let target = factory.CreateDCRenderTarget(&D2D1_RENDER_TARGET_PROPERTIES {
                r#type: D2D1_RENDER_TARGET_TYPE_SOFTWARE,
                pixelFormat: D2D1_PIXEL_FORMAT {
                    format: DXGI_FORMAT_B8G8R8A8_UNORM,
                    alphaMode: D2D1_ALPHA_MODE_IGNORE,
                },
                dpiX: 96.,
                dpiY: 96.,
                usage: D2D1_RENDER_TARGET_USAGE_GDI_COMPATIBLE,
                minLevel: D2D1_FEATURE_LEVEL_DEFAULT,
            })?;
            target.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
            Ok(Self {
                target,
                write: DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?,
            })
        }
    }
    #[allow(clippy::too_many_arguments)]
    fn draw(
        &self,
        hdc: usize,
        text: &str,
        rect: [i32; 4],
        font: &str,
        points: i32,
        style: u8,
        color: u32,
        alignment: u8,
        colorful: bool,
        dpi: u32,
    ) -> windows::core::Result<()> {
        unsafe {
            let bounds = RECT {
                left: rect[0],
                top: rect[1],
                right: rect[2],
                bottom: rect[3],
            };
            self.target.BindDC(HDC(hdc as _), &bounds)?;
            let font: Vec<u16> = font.encode_utf16().chain(Some(0)).collect();
            let format = self.write.CreateTextFormat(
                PCWSTR(font.as_ptr()),
                None,
                if style & 1 != 0 {
                    DWRITE_FONT_WEIGHT_BOLD
                } else {
                    DWRITE_FONT_WEIGHT_NORMAL
                },
                if style & 2 != 0 {
                    DWRITE_FONT_STYLE_ITALIC
                } else {
                    DWRITE_FONT_STYLE_NORMAL
                },
                DWRITE_FONT_STRETCH_NORMAL,
                points as f32 * dpi as f32 / 72.,
                windows::core::w!(""),
            )?;
            format.SetTextAlignment(match alignment {
                1 => DWRITE_TEXT_ALIGNMENT_TRAILING,
                2 => DWRITE_TEXT_ALIGNMENT_CENTER,
                _ => DWRITE_TEXT_ALIGNMENT_LEADING,
            })?;
            format.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;
            format.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)?;
            let text: Vec<u16> = text.encode_utf16().collect();
            let layout = self.write.CreateTextLayout(
                &text,
                &format,
                (rect[2] - rect[0]) as f32,
                (rect[3] - rect[1]) as f32,
            )?;
            let range = DWRITE_TEXT_RANGE {
                startPosition: 0,
                length: text.len() as u32,
            };
            if style & 4 != 0 {
                layout.SetUnderline(true, range)?;
            }
            if style & 8 != 0 {
                layout.SetStrikethrough(true, range)?;
            }
            let brush = self.target.CreateSolidColorBrush(
                &D2D1_COLOR_F {
                    r: (color & 255) as f32 / 255.,
                    g: ((color >> 8) & 255) as f32 / 255.,
                    b: ((color >> 16) & 255) as f32 / 255.,
                    a: 1.,
                },
                None,
            )?;
            let options = if colorful {
                D2D1_DRAW_TEXT_OPTIONS_CLIP | D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT
            } else {
                D2D1_DRAW_TEXT_OPTIONS_CLIP
            };
            self.target.BeginDraw();
            self.target
                .DrawTextLayout(Default::default(), &layout, &brush, options);
            self.target.EndDraw(None, None)
        }
    }
}
thread_local! {static RENDERER:RefCell<Option<Renderer>>=const{RefCell::new(None)};}
#[allow(clippy::too_many_arguments)]
pub(crate) fn draw(
    hdc: usize,
    text: &str,
    rect: [i32; 4],
    font: &str,
    points: i32,
    style: u8,
    color: u32,
    alignment: u8,
    colorful: bool,
    dpi: u32,
) -> Result<(), String> {
    if hdc == 0
        || rect[2] <= rect[0]
        || rect[3] <= rect[1]
        || !(5..=72).contains(&points)
        || !(48..=960).contains(&dpi)
    {
        return Err("Invalid DirectWrite drawing dimensions".into());
    }
    RENDERER.with(|cache| {
        let mut cache = cache
            .try_borrow_mut()
            .map_err(|_| "Reentrant DirectWrite drawing".to_owned())?;
        if cache.is_none() {
            *cache = Some(Renderer::new().map_err(|e| e.to_string())?);
        }
        let result = cache
            .as_ref()
            .unwrap()
            .draw(
                hdc, text, rect, font, points, style, color, alignment, colorful, dpi,
            )
            .map_err(|e| e.to_string());
        if result.is_err() {
            *cache = None;
        }
        result
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    use windows_sys::Win32::Graphics::Gdi::*;
    #[test]
    fn real_color_emoji_pixels_and_monochrome_switch() {
        unsafe {
            let dc = CreateCompatibleDC(std::ptr::null_mut());
            assert!(!dc.is_null());
            let info = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: 160,
                    biHeight: -64,
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut bits = std::ptr::null_mut();
            let bitmap = CreateDIBSection(
                dc,
                &info,
                DIB_RGB_COLORS,
                &mut bits,
                std::ptr::null_mut(),
                0,
            );
            assert!(!bitmap.is_null());
            let old = SelectObject(dc, bitmap);
            let pixels = std::slice::from_raw_parts_mut(bits.cast::<u32>(), 160 * 64);
            let colorful = |pixels: &[u32]| {
                pixels
                    .iter()
                    .filter(|p| {
                        let r = **p & 255;
                        let g = (**p >> 8) & 255;
                        let b = (**p >> 16) & 255;
                        r != g || g != b
                    })
                    .count()
            };
            pixels.fill(0xffffff);
            draw(
                dc as usize,
                "😀",
                [0, 0, 160, 64],
                "Segoe UI Emoji",
                24,
                0,
                0,
                0,
                false,
                96,
            )
            .unwrap();
            GdiFlush();
            assert_eq!(
                colorful(pixels),
                0,
                "monochrome uses grayscale antialiasing"
            );
            assert!(pixels.iter().any(|p| p & 0xffffff != 0xffffff));
            assert_eq!(
                pixels[160 * 63 + 159] & 0xffffff,
                0xffffff,
                "background outside glyph must remain intact"
            );
            pixels.fill(0xffffff);
            draw(
                dc as usize,
                "😀",
                [0, 0, 160, 64],
                "Segoe UI Emoji",
                24,
                0,
                0,
                0,
                true,
                96,
            )
            .unwrap();
            GdiFlush();
            assert!(
                colorful(pixels) > 20,
                "color glyphs must contain actual chromatic pixels"
            );
            SelectObject(dc, old);
            DeleteObject(bitmap);
            DeleteDC(dc);
        }
    }
}
