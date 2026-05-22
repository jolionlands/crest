//! Direct2D paint pipeline for the crest status bar.
//!
//! `Direct2DRenderer` owns a `ID2D1HwndRenderTarget` and a
//! `IDWriteFactory`/`IDWriteTextFormat`, painting all module snapshots in a
//! single `BeginDraw` … `EndDraw` block every frame.

use std::collections::HashMap;

use anyhow::{bail, Result};
use windows::core::Interface;
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_COLOR_F, D2D_RECT_F, D2D_SIZE_U,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1CreateFactory, ID2D1Factory, ID2D1HwndRenderTarget, ID2D1SolidColorBrush,
    D2D1_DRAW_TEXT_OPTIONS_NONE, D2D1_FACTORY_TYPE_SINGLE_THREADED,
    D2D1_HWND_RENDER_TARGET_PROPERTIES, D2D1_PRESENT_OPTIONS_NONE,
    D2D1_RENDER_TARGET_PROPERTIES, D2D1_RENDER_TARGET_TYPE_DEFAULT,
    D2D1_RENDER_TARGET_USAGE_NONE, D2D1_FEATURE_LEVEL_DEFAULT,
};
use windows::Win32::Graphics::DirectWrite::{
    DWriteCreateFactory, IDWriteFactory, IDWriteTextFormat,
    DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_STRETCH_NORMAL,
    DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_WEIGHT_REGULAR,
    DWRITE_MEASURING_MODE_NATURAL, DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
    DWRITE_TEXT_ALIGNMENT_LEADING,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_UNKNOWN;
use windows::Win32::UI::HiDpi::GetDpiForWindow;

use crate::config::types::StyleConfig;

// Re-export so bar/window.rs can convert without touching crate::module
pub use crate::module::{BarRegion, ModuleSnapshot};

/// Brush cache key: ARGB packed as 0xAARRGGBB.
type ColorKey = u32;

pub struct Direct2DRenderer {
    factory: ID2D1Factory,
    rt: Option<ID2D1HwndRenderTarget>,
    dwrite: IDWriteFactory,
    text_format: IDWriteTextFormat,
    brushes: HashMap<ColorKey, ID2D1SolidColorBrush>,
    width: u32,
    height: u32,
    /// DPI scale factor (dpi / 96.0)
    scale: f32,
}

impl Direct2DRenderer {
    /// Create a renderer for `hwnd` with the given physical pixel `size`.
    pub fn new(hwnd: HWND, size: (u32, u32), style: &StyleConfig) -> Result<Self> {
        let (width, height) = size;

        // DPI
        let dpi = unsafe { GetDpiForWindow(hwnd) };
        let scale = dpi as f32 / 96.0;

        // D2D factory
        let factory: ID2D1Factory = unsafe {
            D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?
        };

        // Render target
        let rt_props = D2D1_RENDER_TARGET_PROPERTIES {
            r#type: D2D1_RENDER_TARGET_TYPE_DEFAULT,
            pixelFormat: windows::Win32::Graphics::Direct2D::Common::D2D1_PIXEL_FORMAT {
                format: DXGI_FORMAT_UNKNOWN,
                alphaMode: windows::Win32::Graphics::Direct2D::Common::D2D1_ALPHA_MODE_PREMULTIPLIED,
            },
            dpiX: dpi as f32,
            dpiY: dpi as f32,
            usage: D2D1_RENDER_TARGET_USAGE_NONE,
            minLevel: D2D1_FEATURE_LEVEL_DEFAULT,
        };
        let hwnd_rt_props = D2D1_HWND_RENDER_TARGET_PROPERTIES {
            hwnd,
            pixelSize: D2D_SIZE_U { width, height },
            presentOptions: D2D1_PRESENT_OPTIONS_NONE,
        };
        let rt: ID2D1HwndRenderTarget =
            unsafe { factory.CreateHwndRenderTarget(&rt_props, &hwnd_rt_props)? };

        // DWrite factory
        let dwrite: IDWriteFactory = unsafe {
            DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?
        };

        // Text format
        let font_size_px = style.font_size_pt * scale * (4.0 / 3.0); // pt → px
        let font_family: Vec<u16> = format!("{}\0", style.font_family)
            .encode_utf16()
            .collect();
        let locale: Vec<u16> = "en-US\0".encode_utf16().collect();
        let text_format: IDWriteTextFormat = unsafe {
            dwrite.CreateTextFormat(
                windows::core::PCWSTR(font_family.as_ptr()),
                None,
                DWRITE_FONT_WEIGHT_REGULAR,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                font_size_px,
                windows::core::PCWSTR(locale.as_ptr()),
            )?
        };
        unsafe {
            text_format.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_LEADING)?;
            text_format.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;
        }

        Ok(Self {
            factory,
            rt: Some(rt),
            dwrite,
            text_format,
            brushes: HashMap::new(),
            width,
            height,
            scale,
        })
    }

    /// Recreate the render target after a resize.
    pub fn resize(&mut self, size: (u32, u32)) -> Result<()> {
        self.width = size.0;
        self.height = size.1;
        if let Some(rt) = &self.rt {
            unsafe {
                rt.Resize(&D2D_SIZE_U {
                    width: size.0,
                    height: size.1,
                })?;
            }
        }
        Ok(())
    }

    /// Paint one frame: clear, draw all module zones, present.
    pub fn paint(&mut self, modules: &[ModuleSnapshot], style: &StyleConfig) -> Result<()> {
        // Clone via COM AddRef so we can mutably borrow self for brush cache.
        let rt = match &self.rt {
            Some(rt) => rt.clone(),
            None => bail!("render target not initialised"),
        };
        let rt = &rt;

        let bg = parse_color(&style.background);
        let fg_default = parse_color(&style.foreground);
        let padding = style.padding_px as f32;
        let spacing = style.module_spacing_px as f32;
        let bar_h = self.height as f32;
        let bar_w = self.width as f32;

        unsafe {
            rt.BeginDraw();
            rt.Clear(Some(&bg));

            // Separate modules by zone
            let left_mods: Vec<&ModuleSnapshot> = modules
                .iter()
                .filter(|m| m.region == BarRegion::Left)
                .collect();
            let center_mods: Vec<&ModuleSnapshot> = modules
                .iter()
                .filter(|m| m.region == BarRegion::Center)
                .collect();
            let right_mods: Vec<&ModuleSnapshot> = modules
                .iter()
                .filter(|m| m.region == BarRegion::Right)
                .collect();

            // LEFT zone: x advances left to right from padding
            let mut x = padding;
            for module in &left_mods {
                let color = module
                    .fg
                    .as_deref()
                    .map(parse_color)
                    .unwrap_or(fg_default);
                let brush = self.get_or_create_brush(rt, color)?;
                let text_w = self.measure_text(&module.text, bar_h)?;
                let rect = D2D_RECT_F {
                    left: x,
                    top: 0.0,
                    right: x + text_w + spacing,
                    bottom: bar_h,
                };
                draw_text(rt, &self.text_format, &module.text, rect, &brush);
                x += text_w + spacing;
            }

            // RIGHT zone: measure total width, then draw right-to-left
            let mut right_texts: Vec<(String, D2D1_COLOR_F)> = Vec::new();
            let mut right_total_w = 0.0f32;
            for module in right_mods.iter().rev() {
                let color = module
                    .fg
                    .as_deref()
                    .map(parse_color)
                    .unwrap_or(fg_default);
                let w = self.measure_text(&module.text, bar_h)?;
                right_texts.push((module.text.clone(), color));
                right_total_w += w + spacing;
            }
            let mut rx = bar_w - padding;
            for (text, color) in &right_texts {
                let w = self.measure_text(text, bar_h)?;
                rx -= w + spacing;
                let brush = self.get_or_create_brush(rt, *color)?;
                let rect = D2D_RECT_F {
                    left: rx,
                    top: 0.0,
                    right: rx + w + spacing,
                    bottom: bar_h,
                };
                draw_text(rt, &self.text_format, text, rect, &brush);
            }

            // CENTER zone: measure total, center it
            let mut center_total_w = 0.0f32;
            let mut center_widths = Vec::new();
            for module in &center_mods {
                let w = self.measure_text(&module.text, bar_h)?;
                center_widths.push(w);
                center_total_w += w + spacing;
            }
            if !center_mods.is_empty() {
                center_total_w -= spacing; // no trailing gap
                let mut cx = (bar_w - center_total_w) / 2.0;
                for (module, w) in center_mods.iter().zip(center_widths.iter()) {
                    let color = module
                        .fg
                        .as_deref()
                        .map(parse_color)
                        .unwrap_or(fg_default);
                    let brush = self.get_or_create_brush(rt, color)?;
                    let rect = D2D_RECT_F {
                        left: cx,
                        top: 0.0,
                        right: cx + w + spacing,
                        bottom: bar_h,
                    };
                    draw_text(rt, &self.text_format, &module.text, rect, &brush);
                    cx += w + spacing;
                }
            }

            // EndDraw in windows 0.58 takes optional tag pointers
            let _ = rt.EndDraw(None, None);
        }

        Ok(())
    }

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    fn get_or_create_brush(
        &mut self,
        rt: &ID2D1HwndRenderTarget,
        color: D2D1_COLOR_F,
    ) -> Result<ID2D1SolidColorBrush> {
        // Return an owned clone — COM AddRef is cheap and avoids self-borrow
        // conflicts inside the paint loop.
        let key = color_to_key(color);
        if !self.brushes.contains_key(&key) {
            // ID2D1HwndRenderTarget extends ID2D1RenderTarget; cast to call
            // the parent method.
            let base: windows::Win32::Graphics::Direct2D::ID2D1RenderTarget = rt.cast()?;
            let brush = unsafe { base.CreateSolidColorBrush(&color, None)? };
            self.brushes.insert(key, brush);
        }
        Ok(self.brushes.get(&key).unwrap().clone())
    }

    /// Measure the pixel width of a string using the current text format.
    fn measure_text(&self, text: &str, max_height: f32) -> Result<f32> {
        if text.is_empty() {
            return Ok(0.0);
        }
        let wide: Vec<u16> = text.encode_utf16().collect();
        let layout = unsafe {
            self.dwrite.CreateTextLayout(
                &wide,
                &self.text_format,
                f32::MAX,
                max_height,
            )?
        };
        let mut metrics = windows::Win32::Graphics::DirectWrite::DWRITE_TEXT_METRICS::default();
        unsafe { layout.GetMetrics(&mut metrics)? };
        Ok(metrics.width)
    }
}

// ---------------------------------------------------------------------------
// Free helpers
// ---------------------------------------------------------------------------

/// Draw a single text string into `rect`, vertically centered.
unsafe fn draw_text(
    rt: &ID2D1HwndRenderTarget,
    fmt: &IDWriteTextFormat,
    text: &str,
    rect: D2D_RECT_F,
    brush: &ID2D1SolidColorBrush,
) {
    if text.is_empty() {
        return;
    }
    let wide: Vec<u16> = text.encode_utf16().collect();
    rt.DrawText(
        &wide,
        fmt,
        &rect,
        brush,
        D2D1_DRAW_TEXT_OPTIONS_NONE,
        DWRITE_MEASURING_MODE_NATURAL,
    );
}

/// Parse a hex color string like `"#rrggbb"` or `"#aarrggbb"` into `D2D1_COLOR_F`.
/// Unknown formats return opaque black.
fn parse_color(hex: &str) -> D2D1_COLOR_F {
    let s = hex.trim().trim_start_matches('#');
    let packed: u32 = u32::from_str_radix(s, 16).unwrap_or(0xFF000000);

    let (a, r, g, b) = if s.len() == 8 {
        // AARRGGBB
        (
            ((packed >> 24) & 0xFF) as f32 / 255.0,
            ((packed >> 16) & 0xFF) as f32 / 255.0,
            ((packed >> 8) & 0xFF) as f32 / 255.0,
            (packed & 0xFF) as f32 / 255.0,
        )
    } else {
        // RRGGBB — fully opaque
        (
            1.0,
            ((packed >> 16) & 0xFF) as f32 / 255.0,
            ((packed >> 8) & 0xFF) as f32 / 255.0,
            (packed & 0xFF) as f32 / 255.0,
        )
    };

    D2D1_COLOR_F { r, g, b, a }
}

/// Pack a `D2D1_COLOR_F` into a `u32` cache key.
fn color_to_key(c: D2D1_COLOR_F) -> ColorKey {
    let a = (c.a * 255.0) as u32;
    let r = (c.r * 255.0) as u32;
    let g = (c.g * 255.0) as u32;
    let b = (c.b * 255.0) as u32;
    (a << 24) | (r << 16) | (g << 8) | b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_color_rrggbb() {
        let c = parse_color("#ffffff");
        assert!((c.r - 1.0).abs() < 0.01);
        assert!((c.g - 1.0).abs() < 0.01);
        assert!((c.b - 1.0).abs() < 0.01);
        assert!((c.a - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_parse_color_aarrggbb() {
        let c = parse_color("#ee1e1e1e");
        // alpha 0xEE / 255 ≈ 0.933
        assert!((c.a - 0xEE as f32 / 255.0).abs() < 0.01);
        assert!((c.r - 0x1e as f32 / 255.0).abs() < 0.01);
    }

    #[test]
    fn test_parse_color_default_config_bg() {
        // Background from default config: "#1e1e1eee"
        // This is an 8-hex-digit string → AARRGGBB
        let c = parse_color("#1e1e1eee");
        assert!((c.a - 0x1e as f32 / 255.0).abs() < 0.01);
        assert!((c.r - 0x1e as f32 / 255.0).abs() < 0.01);
    }
}