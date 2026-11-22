use super::*;

/// Theme-synchronized color palette matching `dashboard.css` (`:root` and `html.dark`).
#[derive(Debug, Clone, Copy)]
pub(crate) struct ThemePalette {
    pub surface: (u8, u8, u8),
    pub surface_border: (u8, u8, u8),
    pub row_card: (u8, u8, u8),
    pub row_border: (u8, u8, u8),
    pub row_hover: (u8, u8, u8),
    pub row_hover_border: (u8, u8, u8),
    pub focus_ring: (u8, u8, u8),
    pub seg_track: (u8, u8, u8),
    pub seg_thumb: (u8, u8, u8),
    pub seg_thumb_border: (u8, u8, u8),
    pub chip_bg: (u8, u8, u8),
    pub chip_border: (u8, u8, u8),
    pub chip_fg: (u8, u8, u8),
    pub text: (u8, u8, u8),
    pub sub: (u8, u8, u8),
    pub chevron: (u8, u8, u8),
    pub accent: (u8, u8, u8),
    pub hairline: (u8, u8, u8),
    pub direct_bg: (u8, u8, u8),
    pub direct_fg: (u8, u8, u8),
    pub direct_dot: (u8, u8, u8),
    pub routed_bg: (u8, u8, u8),
    pub routed_fg: (u8, u8, u8),
    pub routed_dot: (u8, u8, u8),
    pub loading_bg: (u8, u8, u8),
    pub loading_fg: (u8, u8, u8),
    pub loading_dot: (u8, u8, u8),
    pub btn_primary_bg: (u8, u8, u8),
    pub btn_primary_hover: (u8, u8, u8),
    pub btn_primary_border: (u8, u8, u8),
    pub btn_primary_fg: (u8, u8, u8),
    pub btn_sec_bg: (u8, u8, u8),
    pub btn_sec_hover: (u8, u8, u8),
    pub btn_sec_border: (u8, u8, u8),
    pub btn_sec_fg: (u8, u8, u8),
    pub btn_sec_hover_fg: (u8, u8, u8),
}

impl ThemePalette {
    pub fn for_theme(dark: bool) -> Self {
        if dark {
            Self {
                surface: (0x1a, 0x1a, 0x1e),
                surface_border: (0x33, 0x33, 0x39),
                row_card: (0x23, 0x23, 0x28),
                row_border: (0x2f, 0x2f, 0x37),
                row_hover: (0x2b, 0x2c, 0x3d),
                row_hover_border: (0x48, 0x4b, 0x74),
                focus_ring: (0x81, 0x8c, 0xf8),
                seg_track: (0x13, 0x13, 0x16),
                seg_thumb: (0x2c, 0x2c, 0x35),
                seg_thumb_border: (0x3f, 0x3f, 0x4a),
                chip_bg: (0x2c, 0x2c, 0x35),
                chip_border: (0x3d, 0x3d, 0x48),
                chip_fg: (0xd4, 0xd4, 0xde),
                text: (0xed, 0xed, 0xf1),
                sub: (0x92, 0x92, 0x9e),
                chevron: (0x72, 0x72, 0x80),
                accent: (0xa5, 0xb4, 0xfc),
                hairline: (0x2c, 0x2c, 0x34),
                direct_bg: (0x1b, 0x36, 0x28),
                direct_fg: (0x6f, 0xd9, 0x9b),
                direct_dot: (0x34, 0xd3, 0x99),
                routed_bg: (0x27, 0x29, 0x46),
                routed_fg: (0xa5, 0xb4, 0xfc),
                routed_dot: (0x81, 0x8c, 0xf8),
                loading_bg: (0x3a, 0x2c, 0x16),
                loading_fg: (0xfb, 0xbf, 0x24),
                loading_dot: (0xf5, 0x9e, 0x0b),
                btn_primary_bg: (0x27, 0x29, 0x46),
                btn_primary_hover: (0x31, 0x34, 0x58),
                btn_primary_border: (0x43, 0x46, 0x75),
                btn_primary_fg: (0xc7, 0xd2, 0xfe),
                btn_sec_bg: (0x23, 0x23, 0x28),
                btn_sec_hover: (0x45, 0x27, 0x2a),
                btn_sec_border: (0x33, 0x33, 0x39),
                btn_sec_fg: (0xb9, 0xb9, 0xc1),
                btn_sec_hover_fg: (0xf2, 0x8b, 0x82),
            }
        } else {
            // Soft cool slate-gray surface matching `dashboard.css` (`--bg: #f4f5f8`) + Aozora UI
            Self {
                surface: (0xec, 0xef, 0xf6),
                surface_border: (0xd0, 0xd6, 0xe4),
                row_card: (0xf6, 0xf8, 0xfc),
                row_border: (0xdc, 0xe1, 0xee),
                row_hover: (0xe4, 0xe9, 0xfb),
                row_hover_border: (0xc2, 0xcd, 0xfc),
                focus_ring: (0x4f, 0x46, 0xe5),
                seg_track: (0xde, 0xe3, 0xef),
                seg_thumb: (0xf8, 0xfa, 0xff),
                seg_thumb_border: (0xce, 0xd4, 0xe4),
                chip_bg: (0xe8, 0xec, 0xf8),
                chip_border: (0xcf, 0xd6, 0xe8),
                chip_fg: (0x33, 0x38, 0x49),
                text: (0x1c, 0x1e, 0x26),
                sub: (0x5e, 0x64, 0x74),
                chevron: (0x85, 0x8b, 0x9c),
                accent: (0x4f, 0x46, 0xe5),
                hairline: (0xd8, 0xde, 0xec),
                direct_bg: (0xdc, 0xf3, 0xe6),
                direct_fg: (0x15, 0x7a, 0x3c),
                direct_dot: (0x18, 0x9a, 0x58),
                routed_bg: (0xe2, 0xe6, 0xfc),
                routed_fg: (0x37, 0x30, 0xa3),
                routed_dot: (0x4f, 0x46, 0xe5),
                loading_bg: (0xfe, 0xf3, 0xc7),
                loading_fg: (0xb4, 0x53, 0x09),
                loading_dot: (0xd9, 0x77, 0x06),
                btn_primary_bg: (0xe2, 0xe6, 0xfc),
                btn_primary_hover: (0xd6, 0xdc, 0xfb),
                btn_primary_border: (0xc2, 0xcd, 0xfc),
                btn_primary_fg: (0x37, 0x30, 0xa3),
                btn_sec_bg: (0xf6, 0xf8, 0xfc),
                btn_sec_hover: (0xfb, 0xe9, 0xe9),
                btn_sec_border: (0xd8, 0xde, 0xec),
                btn_sec_fg: (0x4b, 0x4f, 0x5c),
                btn_sec_hover_fg: (0xd6, 0x45, 0x45),
            }
        }
    }
}

// Logical card geometry (scaled by monitor DPI / 96.0)
pub(crate) const CARD_W: i32 = 316;
pub(crate) const PAD_X: i32 = 13;
pub(crate) const HEADER_H: i32 = 44;
pub(crate) const TABS_H: i32 = 34;
pub(crate) const TABS_GAP: i32 = 8;
pub(crate) const PERIOD_BAR_H: i32 = 28;
pub(crate) const PERIOD_BAR_GAP: i32 = 6;
pub(crate) const AGENT_ROW_H: i32 = 50;
pub(crate) const AGENT_ROW_GAP: i32 = 5;
pub(crate) const DETAIL_BACK_H: i32 = 30;
pub(crate) const DETAIL_HERO_H: i32 = 54;
pub(crate) const DETAIL_INFO_H: i32 = 62;
pub(crate) const DETAIL_MODELS_H: i32 = 60;
pub(crate) const DETAIL_CTA_H: i32 = 34;
pub(crate) const DETAIL_GAP: i32 = 7;
pub(crate) const FOOTER_H: i32 = 46;
pub(crate) const RADIUS: i32 = 14;
pub(crate) const SHADOW_PAD: i32 = 12;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HitTarget {
    GearSettings,
    Tab(TrayTab),
    Period(TrayUsagePeriod),
    Row(usize),
    DetailBack,
    DetailOpenConsole(usize),
    FooterMain,
    FooterExit,
}
