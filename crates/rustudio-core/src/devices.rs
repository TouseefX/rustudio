//! The Device Simulator's devices: Roblox Studio's built-in presets, the
//! rules Studio sizes them by, and the input each one reports.
//!
//! The table was read from Roblox Studio 0.748's built-in device table (its
//! EmulatedDevices XML) and measured in Studio's Play: the graphics quality
//! level each device picks (`settings().Rendering.QualityLevel`, the bar's
//! Q##) and `GuiService.ViewportDisplaySize`. The legacy rows Studio keeps
//! out of its device list (Default, Average Laptop) are left out.

use std::sync::OnceLock;

/// Studio's form factor (`device_form`): no form is a desktop, `vr` a headset.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Form {
    Phone,
    Tablet,
    Desktop,
    Console,
    Headset,
}

/// The simulator bar's four form buttons, in the bar's order.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum FormButton {
    Mobile,
    Desktop,
    Console,
    Headset,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Platform {
    Ios,
    Android,
    Windows,
    MacOs,
    XboxOne,
    Ps4,
    Ps5,
    MetaOs,
}

/// Studio's `tier`: the dropdown's Flagship / Mid-range / Entry level groups.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Tier {
    Flagship,
    MidRange,
    EntryLevel,
}

/// Safe-area insets in device points, in the XML's order.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct Insets {
    pub top: f32,
    pub bottom: f32,
    pub left: f32,
    pub right: f32,
}

impl Insets {
    pub const fn new(top: f32, bottom: f32, left: f32, right: f32) -> Self {
        Self { top, bottom, left, right }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Default, Debug, Hash)]
pub enum Orientation {
    #[default]
    Landscape,
    Portrait,
}

/// `UserInputService.PreferredInput`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Preferred {
    KeyboardAndMouse,
    Touch,
    Gamepad,
}

impl Preferred {
    pub fn enum_name(self) -> &'static str {
        match self {
            Self::KeyboardAndMouse => "KeyboardAndMouse",
            Self::Touch => "Touch",
            Self::Gamepad => "Gamepad",
        }
    }
}

/// What `UserInputService` and `GuiService:IsTenFootInterface()` report.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct InputFlags {
    pub touch: bool,
    pub keyboard: bool,
    pub mouse: bool,
    pub gamepad: bool,
    pub ten_foot: bool,
    pub preferred: Preferred,
}

impl InputFlags {
    /// Studio's Play with no device simulated: keyboard and mouse.
    pub const DESKTOP: Self = Self {
        touch: false,
        keyboard: true,
        mouse: true,
        gamepad: false,
        ten_foot: false,
        preferred: Preferred::KeyboardAndMouse,
    };
}

/// Where Studio's frame image puts the screen (`wireframe_screen_x/y`, in
/// image pixels), the image's scale and whether the screen has rounded
/// corners (`special_corners`).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct FrameSpec {
    pub screen_offset: [f32; 2],
    pub image_scale: f32,
    pub special_corners: bool,
}

/// `GuiService.ViewportDisplaySize`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum DisplaySize {
    Small,
    Medium,
    Large,
}

impl DisplaySize {
    pub fn enum_name(self) -> &'static str {
        match self {
            Self::Small => "Small",
            Self::Medium => "Medium",
            Self::Large => "Large",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Device {
    pub id: String,
    pub name: String,
    pub form: Form,
    pub platform: Platform,
    pub tier: Option<Tier>,
    pub memory_mb: u32,
    /// Width and height in points, landscape.
    pub size: [u32; 2],
    pub ppi: f32,
    pub resolution_scale: f32,
    pub safe_portrait: Insets,
    pub safe_landscape: Insets,
    /// On-screen keyboard height: portrait, landscape.
    pub keyboard_height: [u32; 2],
    pub frame: Option<FrameSpec>,
    /// The default graphics quality level (the bar's Q##).
    pub quality: u8,
    pub display_size: DisplaySize,
    /// Made in Manage devices, not one of Studio's presets.
    pub custom: bool,
}

struct Row {
    id: &'static str,
    name: &'static str,
    form: Form,
    platform: Platform,
    tier: Option<Tier>,
    memory_mb: u32,
    size: [u32; 2],
    ppi: f32,
    resolution_scale: f32,
    safe_portrait: [f32; 4],
    safe_landscape: [f32; 4],
    keyboard_height: [u32; 2],
    frame: Option<([f32; 2], f32, bool)>,
    quality: u8,
    display_size: DisplaySize,
}

use DisplaySize::{Large, Medium, Small};
use Form::{Console, Desktop, Headset, Phone, Tablet};
use Platform::{Android, Ios, MacOs, MetaOs, Ps4, Ps5, Windows, XboxOne};
use Tier::{EntryLevel, Flagship, MidRange};

/// Studio's presets in its table's order. The generic handhelds' XML names
/// (`Generic_Handheld_720`) are shown as Studio's device list shows them.
/// The Galaxy Tab A9 has no `wireframe_scale`: 0.5, as the other Android
/// tablets (estimate).
#[rustfmt::skip]
const ROWS: &[Row] = &[
    Row { id: "xbox", name: "Xbox One", form: Console, platform: XboxOne, tier: None, memory_mb: 8192, size: [1920, 1080], ppi: 96.0, resolution_scale: 1.0, safe_portrait: [0.0, 0.0, 0.0, 0.0], safe_landscape: [0.0, 0.0, 0.0, 0.0], keyboard_height: [0, 0], frame: None, quality: 12, display_size: Large },
    Row { id: "ps4", name: "PS4", form: Console, platform: Ps4, tier: None, memory_mb: 8192, size: [1920, 1080], ppi: 96.0, resolution_scale: 1.0, safe_portrait: [0.0, 0.0, 0.0, 0.0], safe_landscape: [0.0, 0.0, 0.0, 0.0], keyboard_height: [0, 0], frame: None, quality: 13, display_size: Large },
    Row { id: "ps5", name: "PS5", form: Console, platform: Ps5, tier: None, memory_mb: 16384, size: [1920, 1080], ppi: 96.0, resolution_scale: 1.0, safe_portrait: [0.0, 0.0, 0.0, 0.0], safe_landscape: [0.0, 0.0, 0.0, 0.0], keyboard_height: [0, 0], frame: None, quality: 21, display_size: Large },
    Row { id: "android_tv_1080", name: "Android TV 1080p", form: Tablet, platform: Android, tier: None, memory_mb: 2048, size: [1920, 1080], ppi: 44.0, resolution_scale: 1.0, safe_portrait: [0.0, 0.0, 0.0, 0.0], safe_landscape: [54.0, 54.0, 96.0, 96.0], keyboard_height: [0, 540], frame: None, quality: 1, display_size: Large },
    Row { id: "average_windows", name: "Average Windows", form: Desktop, platform: Windows, tier: None, memory_mb: 8192, size: [1366, 768], ppi: 96.0, resolution_scale: 1.0, safe_portrait: [0.0, 0.0, 0.0, 0.0], safe_landscape: [0.0, 0.0, 0.0, 0.0], keyboard_height: [0, 0], frame: None, quality: 17, display_size: Medium },
    Row { id: "average_macbook", name: "Average MacBook", form: Desktop, platform: MacOs, tier: None, memory_mb: 8192, size: [1440, 900], ppi: 96.0, resolution_scale: 1.0, safe_portrait: [0.0, 0.0, 0.0, 0.0], safe_landscape: [0.0, 0.0, 0.0, 0.0], keyboard_height: [0, 0], frame: None, quality: 21, display_size: Medium },
    Row { id: "hd_720", name: "HD 720", form: Desktop, platform: Windows, tier: None, memory_mb: 8192, size: [1280, 720], ppi: 96.0, resolution_scale: 1.0, safe_portrait: [0.0, 0.0, 0.0, 0.0], safe_landscape: [0.0, 0.0, 0.0, 0.0], keyboard_height: [0, 0], frame: None, quality: 17, display_size: Medium },
    Row { id: "hd_1080", name: "HD 1080", form: Desktop, platform: Windows, tier: None, memory_mb: 8192, size: [1920, 1080], ppi: 96.0, resolution_scale: 1.0, safe_portrait: [0.0, 0.0, 0.0, 0.0], safe_landscape: [0.0, 0.0, 0.0, 0.0], keyboard_height: [0, 0], frame: None, quality: 17, display_size: Medium },
    Row { id: "vga", name: "VGA", form: Desktop, platform: Windows, tier: None, memory_mb: 8192, size: [640, 480], ppi: 96.0, resolution_scale: 1.0, safe_portrait: [0.0, 0.0, 0.0, 0.0], safe_landscape: [0.0, 0.0, 0.0, 0.0], keyboard_height: [0, 0], frame: None, quality: 17, display_size: Small },
    Row { id: "meta_quest_2", name: "Meta Quest 2", form: Headset, platform: MetaOs, tier: None, memory_mb: 6144, size: [611, 640], ppi: 773.0, resolution_scale: 3.0, safe_portrait: [0.0, 0.0, 0.0, 0.0], safe_landscape: [0.0, 0.0, 0.0, 0.0], keyboard_height: [0, 0], frame: None, quality: 14, display_size: Medium },
    Row { id: "meta_quest_3", name: "Meta Quest 3", form: Headset, platform: MetaOs, tier: None, memory_mb: 8192, size: [688, 736], ppi: 1218.0, resolution_scale: 3.0, safe_portrait: [0.0, 0.0, 0.0, 0.0], safe_landscape: [0.0, 0.0, 0.0, 0.0], keyboard_height: [0, 0], frame: None, quality: 21, display_size: Medium },
    Row { id: "generic_handheld_720", name: "Generic Handheld HD 720", form: Console, platform: Windows, tier: None, memory_mb: 16384, size: [1280, 720], ppi: 274.0, resolution_scale: 1.5, safe_portrait: [0.0, 0.0, 0.0, 0.0], safe_landscape: [0.0, 0.0, 0.0, 0.0], keyboard_height: [0, 0], frame: None, quality: 21, display_size: Small },
    Row { id: "generic_handheld_1080", name: "Generic Handheld HD 1080", form: Console, platform: Windows, tier: None, memory_mb: 16384, size: [1920, 1080], ppi: 411.0, resolution_scale: 1.5, safe_portrait: [0.0, 0.0, 0.0, 0.0], safe_landscape: [0.0, 0.0, 0.0, 0.0], keyboard_height: [0, 0], frame: None, quality: 21, display_size: Small },
    Row { id: "iphone_6_Plus", name: "iPhone 6 Plus", form: Phone, platform: Ios, tier: Some(EntryLevel), memory_mb: 1024, size: [736, 414], ppi: 401.0, resolution_scale: 3.0, safe_portrait: [0.0, 0.0, 0.0, 0.0], safe_landscape: [0.0, 0.0, 0.0, 0.0], keyboard_height: [226, 162], frame: Some(([126.0, 34.0], 0.3333, false)), quality: 1, display_size: Small },
    Row { id: "iphone_7", name: "iPhone 7", form: Phone, platform: Ios, tier: Some(EntryLevel), memory_mb: 2048, size: [667, 375], ppi: 326.0, resolution_scale: 2.0, safe_portrait: [0.0, 0.0, 0.0, 0.0], safe_landscape: [0.0, 0.0, 0.0, 0.0], keyboard_height: [216, 162], frame: Some(([114.0, 30.0], 0.5, false)), quality: 12, display_size: Small },
    Row { id: "iphone_XR", name: "iPhone XR", form: Phone, platform: Ios, tier: Some(EntryLevel), memory_mb: 3072, size: [896, 414], ppi: 326.0, resolution_scale: 2.0, safe_portrait: [47.0, 34.0, 0.0, 0.0], safe_landscape: [0.0, 21.0, 47.0, 47.0], keyboard_height: [313, 186], frame: Some(([38.0, 46.0], 0.5, true)), quality: 21, display_size: Small },
    Row { id: "iphone_11", name: "iPhone 11", form: Phone, platform: Ios, tier: Some(EntryLevel), memory_mb: 4096, size: [896, 414], ppi: 326.0, resolution_scale: 2.0, safe_portrait: [47.0, 34.0, 0.0, 0.0], safe_landscape: [0.0, 21.0, 47.0, 47.0], keyboard_height: [313, 186], frame: Some(([38.0, 46.0], 0.5, true)), quality: 21, display_size: Small },
    Row { id: "samsung_galaxy_a06", name: "Samsung Galaxy A06", form: Phone, platform: Android, tier: Some(EntryLevel), memory_mb: 4096, size: [800, 360], ppi: 262.0, resolution_scale: 2.0, safe_portrait: [47.0, 34.0, 0.0, 0.0], safe_landscape: [0.0, 21.0, 47.0, 47.0], keyboard_height: [280, 162], frame: Some(([41.0, 27.0], 0.5, true)), quality: 1, display_size: Small },
    Row { id: "iphone_13", name: "iPhone 13", form: Phone, platform: Ios, tier: Some(MidRange), memory_mb: 4096, size: [844, 390], ppi: 460.0, resolution_scale: 3.0, safe_portrait: [47.0, 34.0, 0.0, 0.0], safe_landscape: [0.0, 21.0, 47.0, 47.0], keyboard_height: [295, 176], frame: Some(([24.0, 31.0], 0.3333, true)), quality: 21, display_size: Small },
    Row { id: "iphone_13_pro", name: "iPhone 13 Pro", form: Phone, platform: Ios, tier: Some(MidRange), memory_mb: 6144, size: [844, 390], ppi: 460.0, resolution_scale: 3.0, safe_portrait: [47.0, 34.0, 0.0, 0.0], safe_landscape: [0.0, 21.0, 47.0, 47.0], keyboard_height: [295, 176], frame: Some(([24.0, 31.0], 0.3333, true)), quality: 21, display_size: Small },
    Row { id: "iphone_13_pro_max", name: "iPhone 13 Pro Max", form: Phone, platform: Ios, tier: Some(MidRange), memory_mb: 6144, size: [926, 428], ppi: 458.0, resolution_scale: 3.0, safe_portrait: [47.0, 34.0, 0.0, 0.0], safe_landscape: [0.0, 21.0, 47.0, 47.0], keyboard_height: [324, 192], frame: Some(([24.0, 31.0], 0.3333, true)), quality: 21, display_size: Small },
    Row { id: "iphone_14", name: "iPhone 14", form: Phone, platform: Ios, tier: Some(MidRange), memory_mb: 6144, size: [844, 390], ppi: 460.0, resolution_scale: 3.0, safe_portrait: [47.0, 34.0, 0.0, 0.0], safe_landscape: [0.0, 21.0, 47.0, 47.0], keyboard_height: [295, 176], frame: Some(([24.0, 31.0], 0.3333, true)), quality: 21, display_size: Small },
    Row { id: "samsung_galaxy_a16", name: "Samsung Galaxy A16", form: Phone, platform: Android, tier: Some(MidRange), memory_mb: 6144, size: [780, 360], ppi: 385.0, resolution_scale: 3.0, safe_portrait: [47.0, 34.0, 0.0, 0.0], safe_landscape: [0.0, 21.0, 47.0, 47.0], keyboard_height: [273, 162], frame: Some(([41.0, 26.0], 0.3333, true)), quality: 10, display_size: Small },
    Row { id: "iphone_16", name: "iPhone 16", form: Phone, platform: Ios, tier: Some(Flagship), memory_mb: 8192, size: [852, 393], ppi: 460.0, resolution_scale: 3.0, safe_portrait: [59.0, 34.0, 0.0, 0.0], safe_landscape: [0.0, 21.0, 59.0, 59.0], keyboard_height: [298, 177], frame: Some(([22.0, 29.0], 0.3333, true)), quality: 21, display_size: Small },
    Row { id: "iphone_16_pro", name: "iPhone 16 Pro", form: Phone, platform: Ios, tier: Some(Flagship), memory_mb: 8192, size: [874, 402], ppi: 460.0, resolution_scale: 3.0, safe_portrait: [62.0, 34.0, 0.0, 0.0], safe_landscape: [0.0, 21.0, 62.0, 62.0], keyboard_height: [306, 181], frame: Some(([17.0, 25.0], 0.3333, true)), quality: 21, display_size: Small },
    Row { id: "iphone_16_pro_max", name: "iPhone 16 Pro Max", form: Phone, platform: Ios, tier: Some(Flagship), memory_mb: 8192, size: [956, 440], ppi: 460.0, resolution_scale: 3.0, safe_portrait: [62.0, 34.0, 0.0, 0.0], safe_landscape: [0.0, 21.0, 62.0, 62.0], keyboard_height: [335, 198], frame: Some(([18.0, 25.0], 0.3333, true)), quality: 21, display_size: Small },
    Row { id: "iphone_17_pro", name: "iPhone 17 Pro", form: Phone, platform: Ios, tier: Some(Flagship), memory_mb: 12288, size: [874, 402], ppi: 460.0, resolution_scale: 3.0, safe_portrait: [62.0, 34.0, 0.0, 0.0], safe_landscape: [20.0, 20.0, 62.0, 62.0], keyboard_height: [306, 181], frame: Some(([17.0, 25.0], 0.3333, true)), quality: 21, display_size: Small },
    Row { id: "samsung_galaxy_s22_ultra", name: "Samsung Galaxy S22 Ultra", form: Phone, platform: Android, tier: Some(Flagship), memory_mb: 8192, size: [772, 360], ppi: 501.0, resolution_scale: 4.0, safe_portrait: [47.0, 34.0, 0.0, 0.0], safe_landscape: [0.0, 21.0, 47.0, 47.0], keyboard_height: [270, 162], frame: Some(([14.0, 18.0], 0.25, true)), quality: 15, display_size: Small },
    Row { id: "samsung_galaxy_s25_ultra", name: "Samsung Galaxy S25 Ultra", form: Phone, platform: Android, tier: Some(Flagship), memory_mb: 12288, size: [780, 360], ppi: 498.0, resolution_scale: 4.0, safe_portrait: [7.0, 34.0, 0.0, 0.0], safe_landscape: [0.0, 21.0, 47.0, 47.0], keyboard_height: [273, 162], frame: Some(([10.0, 18.0], 0.25, true)), quality: 21, display_size: Small },
    Row { id: "ipad_6th_generation", name: "iPad 6th Generation", form: Tablet, platform: Ios, tier: Some(EntryLevel), memory_mb: 1981, size: [1024, 768], ppi: 264.0, resolution_scale: 2.0, safe_portrait: [0.0, 0.0, 0.0, 0.0], safe_landscape: [0.0, 0.0, 0.0, 0.0], keyboard_height: [320, 408], frame: Some(([135.0, 54.0], 0.5, false)), quality: 21, display_size: Small },
    Row { id: "ipad_8th_generation", name: "iPad 8th Generation", form: Tablet, platform: Ios, tier: Some(EntryLevel), memory_mb: 3072, size: [1080, 810], ppi: 264.0, resolution_scale: 2.0, safe_portrait: [0.0, 0.0, 0.0, 0.0], safe_landscape: [0.0, 0.0, 0.0, 0.0], keyboard_height: [324, 408], frame: Some(([115.0, 58.0], 0.5, false)), quality: 21, display_size: Small },
    Row { id: "ipad_9th_generation", name: "iPad 9th Generation", form: Tablet, platform: Ios, tier: Some(EntryLevel), memory_mb: 3072, size: [1080, 810], ppi: 264.0, resolution_scale: 2.0, safe_portrait: [0.0, 0.0, 0.0, 0.0], safe_landscape: [0.0, 0.0, 0.0, 0.0], keyboard_height: [324, 408], frame: Some(([115.0, 58.0], 0.5, false)), quality: 21, display_size: Small },
    Row { id: "xiaomi_redmi_pad_se", name: "Xiaomi Redmi Pad SE", form: Tablet, platform: Android, tier: Some(EntryLevel), memory_mb: 4096, size: [960, 600], ppi: 206.0, resolution_scale: 2.0, safe_portrait: [0.0, 0.0, 0.0, 0.0], safe_landscape: [0.0, 0.0, 0.0, 0.0], keyboard_height: [288, 302], frame: Some(([40.0, 45.0], 0.5, true)), quality: 2, display_size: Medium },
    Row { id: "amazon_fire_hd10_2023", name: "Amazon Fire HD 10 (2023)", form: Tablet, platform: Android, tier: Some(EntryLevel), memory_mb: 3072, size: [960, 600], ppi: 224.0, resolution_scale: 2.0, safe_portrait: [0.0, 0.0, 0.0, 0.0], safe_landscape: [0.0, 0.0, 0.0, 0.0], keyboard_height: [288, 302], frame: Some(([69.0, 64.0], 0.5, false)), quality: 2, display_size: Small },
    Row { id: "ipad_10th_generation", name: "iPad 10th Generation", form: Tablet, platform: Ios, tier: Some(MidRange), memory_mb: 4096, size: [1180, 820], ppi: 264.0, resolution_scale: 2.0, safe_portrait: [0.0, 0.0, 0.0, 0.0], safe_landscape: [0.0, 0.0, 0.0, 0.0], keyboard_height: [354, 413], frame: Some(([54.0, 60.0], 0.5, true)), quality: 21, display_size: Medium },
    Row { id: "ipad_a16", name: "iPad A16", form: Tablet, platform: Ios, tier: Some(MidRange), memory_mb: 6144, size: [1180, 820], ppi: 264.0, resolution_scale: 2.0, safe_portrait: [0.0, 0.0, 0.0, 0.0], safe_landscape: [0.0, 0.0, 0.0, 0.0], keyboard_height: [354, 413], frame: Some(([54.0, 60.0], 0.5, true)), quality: 21, display_size: Medium },
    Row { id: "samsung_galaxy_tab_a8", name: "Samsung Galaxy Tab A8", form: Tablet, platform: Android, tier: Some(MidRange), memory_mb: 4096, size: [960, 600], ppi: 216.0, resolution_scale: 2.0, safe_portrait: [0.0, 0.0, 0.0, 0.0], safe_landscape: [0.0, 0.0, 0.0, 0.0], keyboard_height: [288, 302], frame: Some(([44.0, 49.0], 0.5, true)), quality: 2, display_size: Medium },
    Row { id: "samsung_galaxy_tab_a9", name: "Samsung Galaxy Tab A9", form: Tablet, platform: Android, tier: Some(MidRange), memory_mb: 4096, size: [1340, 800], ppi: 179.0, resolution_scale: 1.0, safe_portrait: [0.0, 0.0, 0.0, 0.0], safe_landscape: [0.0, 0.0, 0.0, 0.0], keyboard_height: [402, 403], frame: Some(([81.0, 54.0], 0.5, true)), quality: 14, display_size: Small },
    Row { id: "ipad_air_5th_generation", name: "iPad Air 5th Generation", form: Tablet, platform: Ios, tier: Some(Flagship), memory_mb: 8192, size: [1180, 820], ppi: 264.0, resolution_scale: 2.0, safe_portrait: [0.0, 0.0, 0.0, 0.0], safe_landscape: [0.0, 0.0, 0.0, 0.0], keyboard_height: [354, 413], frame: Some(([61.0, 61.0], 0.5, true)), quality: 21, display_size: Medium },
    Row { id: "ipad_pro_M4_11in", name: "iPad Pro M4 (11in)", form: Tablet, platform: Ios, tier: Some(Flagship), memory_mb: 8192, size: [1210, 834], ppi: 264.0, resolution_scale: 2.0, safe_portrait: [0.0, 0.0, 0.0, 0.0], safe_landscape: [0.0, 0.0, 0.0, 0.0], keyboard_height: [363, 420], frame: Some(([48.0, 54.0], 0.5, true)), quality: 21, display_size: Medium },
    Row { id: "ipad_pro_M5_13in", name: "iPad Pro M5 (13in)", form: Tablet, platform: Ios, tier: Some(Flagship), memory_mb: 12288, size: [1376, 1032], ppi: 264.0, resolution_scale: 2.0, safe_portrait: [0.0, 0.0, 0.0, 0.0], safe_landscape: [0.0, 0.0, 0.0, 0.0], keyboard_height: [413, 520], frame: Some(([45.0, 51.0], 0.5, true)), quality: 21, display_size: Medium },
    Row { id: "samsung_galaxy_tab_a9+", name: "Samsung Galaxy Tab A9+", form: Tablet, platform: Android, tier: Some(Flagship), memory_mb: 8192, size: [960, 600], ppi: 206.0, resolution_scale: 2.0, safe_portrait: [0.0, 0.0, 0.0, 0.0], safe_landscape: [0.0, 0.0, 0.0, 0.0], keyboard_height: [288, 302], frame: Some(([45.0, 50.0], 0.5, true)), quality: 8, display_size: Medium },
    Row { id: "samsung_galaxy_tab_S11", name: "Samsung Galaxy Tab S11", form: Tablet, platform: Android, tier: Some(Flagship), memory_mb: 12288, size: [1280, 800], ppi: 274.0, resolution_scale: 2.0, safe_portrait: [0.0, 0.0, 0.0, 0.0], safe_landscape: [0.0, 0.0, 0.0, 0.0], keyboard_height: [384, 403], frame: Some(([52.0, 58.0], 0.5, true)), quality: 21, display_size: Medium },
];

fn insets(v: [f32; 4]) -> Insets {
    Insets::new(v[0], v[1], v[2], v[3])
}

/// Studio's 43 presets, in its table's order.
pub fn presets() -> &'static [Device] {
    static LIST: OnceLock<Vec<Device>> = OnceLock::new();
    LIST.get_or_init(|| {
        ROWS.iter()
            .map(|r| Device {
                id: r.id.to_string(),
                name: r.name.to_string(),
                form: r.form,
                platform: r.platform,
                tier: r.tier,
                memory_mb: r.memory_mb,
                size: r.size,
                ppi: r.ppi,
                resolution_scale: r.resolution_scale,
                safe_portrait: insets(r.safe_portrait),
                safe_landscape: insets(r.safe_landscape),
                keyboard_height: r.keyboard_height,
                frame: r.frame.map(|(screen_offset, image_scale, special_corners)| FrameSpec {
                    screen_offset,
                    image_scale,
                    special_corners,
                }),
                quality: r.quality,
                display_size: r.display_size,
                custom: false,
            })
            .collect()
    })
}

pub fn preset(id: &str) -> Option<&'static Device> {
    presets().iter().find(|d| d.id == id)
}

impl Device {
    pub fn button(&self) -> FormButton {
        match self.form {
            Form::Phone | Form::Tablet => FormButton::Mobile,
            Form::Desktop => FormButton::Desktop,
            Form::Console => FormButton::Console,
            Form::Headset => FormButton::Headset,
        }
    }

    /// Phones and tablets turn between landscape and portrait.
    pub fn rotates(&self) -> bool {
        matches!(self.form, Form::Phone | Form::Tablet)
    }

    /// The screen in points for an orientation (the table is landscape).
    pub fn points(&self, o: Orientation) -> [u32; 2] {
        match o {
            Orientation::Landscape => self.size,
            Orientation::Portrait if self.rotates() => [self.size[1], self.size[0]],
            Orientation::Portrait => self.size,
        }
    }

    /// The safe-area insets Studio applies. On iOS in landscape the bottom
    /// inset (the home indicator) is not taken off (measured: iPhone 14
    /// 749x389, not 749x368; Android uses all four, Galaxy S22 677x338).
    pub fn safe_insets(&self, o: Orientation) -> Insets {
        match o {
            Orientation::Portrait if self.rotates() => self.safe_portrait,
            _ => {
                let mut s = self.safe_landscape;
                if self.platform == Platform::Ios {
                    s.bottom = 0.0;
                }
                s
            }
        }
    }

    /// Studio's TV (Android TV 1080p): a touch device with a gamepad.
    fn is_tv(&self) -> bool {
        self.rotates() && self.tier.is_none() && !self.custom
    }

    /// The device's own input, with the Controller Emulator closed
    /// (measured in Studio's Play for every preset).
    pub fn flags(&self) -> InputFlags {
        let f = |touch, keyboard, mouse, gamepad, ten_foot, preferred| InputFlags {
            touch,
            keyboard,
            mouse,
            gamepad,
            ten_foot,
            preferred,
        };
        match self.form {
            Form::Phone | Form::Tablet => f(true, false, false, self.is_tv(), false, Preferred::Touch),
            Form::Desktop => InputFlags::DESKTOP,
            Form::Console if matches!(self.platform, Platform::XboxOne | Platform::Ps4 | Platform::Ps5) => {
                f(false, false, false, true, true, Preferred::Gamepad)
            }
            // The generic handhelds: everything, preferring the gamepad.
            Form::Console => f(true, true, true, true, false, Preferred::Gamepad),
            Form::Headset => f(false, false, true, true, false, Preferred::Gamepad),
        }
    }

    /// Gamepad1 is connected while this device is simulated (consoles,
    /// handhelds, headsets and the TV; measured).
    pub fn native_gamepad(&self) -> bool {
        matches!(self.form, Form::Console | Form::Headset) || self.is_tv()
    }
}

/// The screen size scripts see at `k_px` window pixels per device point:
/// the scaled screen is floored to whole pixels, then divided back and
/// floored (Studio: iPhone 14 844x390 shows as 843x389 when fitted).
pub fn logical_size(points: [u32; 2], k_px: f64) -> [f32; 2] {
    let axis = |p: u32| {
        if k_px <= 0.0 {
            return p as f32;
        }
        // (1e-4: the scale comes from f32 window sizes; 768 points at
        // 718/768 must give 718 pixels, not 717)
        let px = (p as f64 * k_px + 1e-4).floor();
        (px / k_px + 1e-4).floor() as f32
    };
    [axis(points[0]), axis(points[1])]
}

/// `GuiService.ViewportDisplaySize` for a device: fitted to the 43 measured
/// presets (it agrees with all of them); for custom devices it is an
/// estimate. Consoles are Large, headsets Medium; others by the physical
/// diagonal: 40 in and up Large (the 50 in TV), under 10.35 in Small.
pub fn display_size_for(form: Form, platform: Platform, size: [u32; 2], resolution_scale: f32, ppi: f32) -> DisplaySize {
    if matches!(platform, Platform::XboxOne | Platform::Ps4 | Platform::Ps5) {
        return DisplaySize::Large;
    }
    if form == Form::Headset {
        return DisplaySize::Medium;
    }
    let inches = (size[0] as f32).hypot(size[1] as f32) * resolution_scale / ppi.max(1.0);
    if inches >= 40.0 {
        DisplaySize::Large
    } else if inches < 10.35 {
        DisplaySize::Small
    } else {
        DisplaySize::Medium
    }
}

// ------------------------------------------------------------ frame art

/// The six kinds of device frame Studio draws (its art is Roblox's;
/// Rustudio draws its own from these measurements).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FrameFamily {
    HomeButtonIphone,
    NotchIphone,
    IslandIphone,
    TeardropAndroid,
    PunchHoleAndroid,
    Tablet { camera_and_home: bool },
}

/// A device's frame, landscape, in device points from the screen's top-left
/// corner (portrait turns it a quarter turn).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct FrameArt {
    pub family: FrameFamily,
    /// The black body round the screen.
    pub bezel: Insets,
    pub outer_radius: f32,
    /// The screen's own rounded corners (0: square).
    pub screen_radius: f32,
    /// Older devices have a dark grey body rather than a black one.
    pub grey_body: bool,
    /// A side frame drawn round the body, this wide (0: none).
    pub rim: f32,
    /// The notch, island, teardrop or punch hole: x, y, w, h.
    pub cutout: Option<[f32; 4]>,
    /// Buttons on the body's edges: x, y, w, h.
    pub side_buttons: &'static [[f32; 4]],
    /// Speaker slot, camera and home button in the bezel: x, y, w, h.
    pub features: &'static [[f32; 4]],
}

impl FrameArt {
    /// How far the frame (body, side frame and buttons) reaches past the
    /// screen on each side, landscape.
    pub fn extent(&self, size: [u32; 2]) -> Insets {
        let (w, h) = (size[0] as f32, size[1] as f32);
        let mut e = Insets::new(self.bezel.top + self.rim, self.bezel.bottom + self.rim, self.bezel.left + self.rim, self.bezel.right + self.rim);
        for &[x, y, bw, bh] in self.side_buttons {
            e.top = e.top.max(-y);
            e.bottom = e.bottom.max(y + bh - h);
            e.left = e.left.max(-x);
            e.right = e.right.max(x + bw - w);
        }
        e
    }
}

struct FrameRow {
    id: &'static str,
    family: FrameFamily,
    bezel: [f32; 4],
    outer_radius: f32,
    screen_radius: f32,
    grey_body: bool,
    rim: f32,
    cutout: Option<[f32; 4]>,
    side_buttons: &'static [[f32; 4]],
    features: &'static [[f32; 4]],
}

use FrameFamily::{HomeButtonIphone, IslandIphone, NotchIphone, PunchHoleAndroid, TeardropAndroid};

/// Measured from Studio 0.748's frame captures (every device with a
/// `wireframe_image_filename`): the body, screen corners, cut-outs, buttons
/// and bezel features, converted to device points by the captured screen's
/// width. Studio's XML `wireframe_screen_x/y` place the screen in its image,
/// not the visible bezel (iPhone 14: 24 / 31 px against a 21 pt bezel), so
/// they are not used. The iPhone 6 Plus's home button didn't show in its
/// capture: placed as on the iPhone 7 (estimate).
#[rustfmt::skip]
const FRAMES: &[FrameRow] = &[
    FrameRow { id: "iphone_6_Plus", family: HomeButtonIphone, bezel: [31.0, 31.0, 126.5, 119.0], outer_radius: 56.0, screen_radius: 0.0, grey_body: true, rim: 0.0, cutout: None, side_buttons: &[[858.5, 428.5, 5.0, 9.0], [836.5, 448.5, 10.5, 4.0]], features: &[[-65.5, 174.0, 7.5, 65.0], [-69.0, 265.0, 14.0, 14.0], [762.0, 173.5, 67.0, 67.0]] },
    FrameRow { id: "iphone_7", family: HomeButtonIphone, bezel: [28.0, 28.0, 114.5, 106.0], outer_radius: 50.0, screen_radius: 0.0, grey_body: true, rim: 0.0, cutout: None, side_buttons: &[], features: &[[-59.5, 150.0, 7.0, 76.5], [684.5, 154.5, 67.0, 67.5], [-64.0, 248.5, 16.0, 15.0]] },
    FrameRow { id: "iphone_XR", family: NotchIphone, bezel: [34.5, 34.5, 34.5, 34.5], outer_radius: 70.5, screen_radius: 44.0, grey_body: false, rim: 9.0, cutout: Some([0.0, 88.5, 32.0, 237.5]), side_buttons: &[], features: &[] },
    FrameRow { id: "iphone_11", family: NotchIphone, bezel: [34.5, 34.5, 34.5, 34.5], outer_radius: 70.5, screen_radius: 44.0, grey_body: false, rim: 9.0, cutout: Some([0.0, 88.5, 32.0, 237.5]), side_buttons: &[], features: &[] },
    FrameRow { id: "samsung_galaxy_a06", family: TeardropAndroid, bezel: [18.0, 18.0, 17.5, 37.5], outer_radius: 37.5, screen_radius: 24.5, grey_body: false, rim: 0.0, cutout: Some([0.0, 155.0, 25.0, 47.0]), side_buttons: &[[135.0, -25.0, 113.0, 5.5], [294.0, -25.0, 66.5, 5.5]], features: &[] },
    FrameRow { id: "iphone_13", family: NotchIphone, bezel: [21.0, 21.0, 21.0, 21.0], outer_radius: 55.5, screen_radius: 39.0, grey_body: false, rim: 0.0, cutout: Some([0.0, 108.0, 33.5, 174.5]), side_buttons: &[[238.5, -28.5, 109.0, 8.0], [214.5, 411.0, 73.0, 8.0], [305.0, 411.0, 72.5, 8.0], [154.0, 411.5, 36.5, 7.0]], features: &[] },
    FrameRow { id: "iphone_13_pro", family: NotchIphone, bezel: [21.0, 21.0, 21.0, 21.0], outer_radius: 55.5, screen_radius: 39.0, grey_body: false, rim: 0.0, cutout: Some([0.0, 108.0, 33.5, 174.5]), side_buttons: &[[238.5, -28.5, 109.0, 8.0], [214.5, 411.0, 72.5, 8.0], [305.0, 411.0, 72.5, 8.0], [154.0, 411.5, 36.5, 7.0]], features: &[] },
    FrameRow { id: "iphone_13_pro_max", family: NotchIphone, bezel: [20.5, 20.5, 20.5, 20.5], outer_radius: 58.0, screen_radius: 44.5, grey_body: false, rim: 8.0, cutout: Some([0.0, 128.5, 33.5, 171.5]), side_buttons: &[], features: &[] },
    FrameRow { id: "iphone_14", family: NotchIphone, bezel: [21.0, 21.0, 21.0, 21.0], outer_radius: 55.5, screen_radius: 39.0, grey_body: false, rim: 0.0, cutout: Some([0.0, 111.5, 33.5, 166.5]), side_buttons: &[[238.5, -28.5, 109.0, 8.0], [214.5, 411.0, 72.5, 8.0], [305.0, 411.0, 72.5, 8.0], [154.0, 411.5, 36.5, 7.0]], features: &[] },
    FrameRow { id: "samsung_galaxy_a16", family: TeardropAndroid, bezel: [17.0, 17.0, 18.0, 37.5], outer_radius: 37.5, screen_radius: 26.0, grey_body: false, rim: 7.0, cutout: Some([0.0, 153.0, 32.5, 55.0]), side_buttons: &[], features: &[] },
    FrameRow { id: "iphone_16", family: IslandIphone, bezel: [19.5, 19.5, 18.5, 18.5], outer_radius: 63.5, screen_radius: 49.5, grey_body: false, rim: 8.0, cutout: Some([8.5, 135.5, 36.0, 123.0]), side_buttons: &[], features: &[] },
    FrameRow { id: "iphone_16_pro", family: IslandIphone, bezel: [14.5, 14.5, 14.5, 14.5], outer_radius: 66.0, screen_radius: 52.5, grey_body: false, rim: 0.0, cutout: Some([8.5, 138.5, 36.5, 124.0]), side_buttons: &[[263.0, -23.0, 109.0, 8.0], [323.5, 416.0, 72.5, 8.0], [172.5, 417.0, 42.5, 7.0], [239.0, 417.0, 72.5, 7.0]], features: &[] },
    FrameRow { id: "iphone_16_pro_max", family: IslandIphone, bezel: [14.5, 14.5, 14.5, 14.5], outer_radius: 67.5, screen_radius: 54.5, grey_body: false, rim: 0.0, cutout: Some([8.5, 157.0, 36.0, 124.5]), side_buttons: &[[67.5, -22.5, 636.5, 8.0], [172.0, 454.0, 42.5, 8.0], [238.5, 454.0, 637.5, 8.0]], features: &[] },
    FrameRow { id: "iphone_17_pro", family: IslandIphone, bezel: [15.0, 15.0, 14.0, 14.0], outer_radius: 68.0, screen_radius: 56.5, grey_body: false, rim: 0.0, cutout: Some([14.5, 139.5, 36.0, 123.0]), side_buttons: &[[262.0, -23.0, 108.5, 8.0], [237.5, 417.0, 72.5, 8.0], [322.0, 417.0, 72.5, 8.0], [171.5, 417.5, 42.5, 7.0]], features: &[] },
    FrameRow { id: "samsung_galaxy_s22_ultra", family: PunchHoleAndroid, bezel: [9.5, 9.5, 11.5, 11.5], outer_radius: 1.0, screen_radius: 0.0, grey_body: false, rim: 6.5, cutout: Some([4.5, 169.0, 22.5, 22.0]), side_buttons: &[], features: &[] },
    FrameRow { id: "samsung_galaxy_s25_ultra", family: PunchHoleAndroid, bezel: [9.5, 9.5, 8.0, 8.0], outer_radius: 12.0, screen_radius: 5.0, grey_body: false, rim: 6.5, cutout: Some([4.5, 170.0, 19.5, 19.5]), side_buttons: &[], features: &[] },
    FrameRow { id: "ipad_6th_generation", family: FrameFamily::Tablet { camera_and_home: true }, bezel: [55.0, 61.5, 135.5, 117.5], outer_radius: 34.5, screen_radius: 0.0, grey_body: true, rim: 0.0, cutout: None, side_buttons: &[], features: &[[1061.0, 359.0, 56.0, 56.0], [-57.5, 381.0, 13.0, 13.0]] },
    FrameRow { id: "ipad_8th_generation", family: FrameFamily::Tablet { camera_and_home: false }, bezel: [49.0, 49.0, 113.0, 113.0], outer_radius: 39.0, screen_radius: 0.0, grey_body: false, rim: 6.5, cutout: None, side_buttons: &[], features: &[] },
    FrameRow { id: "ipad_9th_generation", family: FrameFamily::Tablet { camera_and_home: false }, bezel: [49.0, 49.0, 113.0, 113.0], outer_radius: 39.0, screen_radius: 0.0, grey_body: false, rim: 6.5, cutout: None, side_buttons: &[], features: &[] },
    FrameRow { id: "xiaomi_redmi_pad_se", family: FrameFamily::Tablet { camera_and_home: false }, bezel: [39.0, 39.0, 38.0, 38.0], outer_radius: 24.0, screen_radius: 5.0, grey_body: false, rim: 3.0, cutout: None, side_buttons: &[[50.0, -44.0, 98.0, 4.0]], features: &[] },
    FrameRow { id: "amazon_fire_hd10_2023", family: FrameFamily::Tablet { camera_and_home: false }, bezel: [60.5, 60.5, 61.0, 55.5], outer_radius: 24.0, screen_radius: 0.0, grey_body: false, rim: 6.5, cutout: None, side_buttons: &[], features: &[] },
    FrameRow { id: "ipad_10th_generation", family: FrameFamily::Tablet { camera_and_home: false }, bezel: [51.0, 51.0, 52.0, 52.0], outer_radius: 61.5, screen_radius: 19.0, grey_body: false, rim: 7.0, cutout: None, side_buttons: &[], features: &[] },
    FrameRow { id: "ipad_a16", family: FrameFamily::Tablet { camera_and_home: false }, bezel: [51.0, 51.0, 52.0, 52.0], outer_radius: 61.5, screen_radius: 19.0, grey_body: false, rim: 7.0, cutout: None, side_buttons: &[], features: &[] },
    FrameRow { id: "samsung_galaxy_tab_a8", family: FrameFamily::Tablet { camera_and_home: false }, bezel: [42.0, 42.0, 41.5, 41.5], outer_radius: 22.5, screen_radius: 7.0, grey_body: false, rim: 0.0, cutout: None, side_buttons: &[[-13.0, -47.5, 241.0, 6.0], [261.0, -47.5, 708.0, 6.0]], features: &[] },
    FrameRow { id: "samsung_galaxy_tab_a9", family: FrameFamily::Tablet { camera_and_home: false }, bezel: [41.5, 41.5, 77.0, 77.0], outer_radius: 44.5, screen_radius: 9.0, grey_body: false, rim: 0.0, cutout: None, side_buttons: &[[311.0, -51.0, 86.5, 8.0], [502.5, -51.0, 185.0, 8.0]], features: &[] },
    FrameRow { id: "ipad_air_5th_generation", family: FrameFamily::Tablet { camera_and_home: false }, bezel: [52.0, 52.0, 52.0, 52.0], outer_radius: 45.0, screen_radius: 7.0, grey_body: false, rim: 7.0, cutout: None, side_buttons: &[], features: &[] },
    FrameRow { id: "ipad_pro_M4_11in", family: FrameFamily::Tablet { camera_and_home: false }, bezel: [45.0, 45.0, 45.5, 45.5], outer_radius: 46.5, screen_radius: 11.0, grey_body: false, rim: 7.0, cutout: None, side_buttons: &[], features: &[] },
    FrameRow { id: "ipad_pro_M5_13in", family: FrameFamily::Tablet { camera_and_home: false }, bezel: [42.5, 41.0, 42.5, 41.0], outer_radius: 49.5, screen_radius: 16.5, grey_body: false, rim: 3.5, cutout: None, side_buttons: &[[18.0, 1075.0, 1037.5, 8.0]], features: &[] },
    FrameRow { id: "samsung_galaxy_tab_a9+", family: FrameFamily::Tablet { camera_and_home: false }, bezel: [43.0, 43.0, 43.0, 43.0], outer_radius: 24.5, screen_radius: 7.0, grey_body: false, rim: 5.0, cutout: None, side_buttons: &[], features: &[] },
    FrameRow { id: "samsung_galaxy_tab_S11", family: FrameFamily::Tablet { camera_and_home: false }, bezel: [49.0, 49.0, 50.0, 50.0], outer_radius: 37.0, screen_radius: 16.0, grey_body: false, rim: 6.5, cutout: None, side_buttons: &[], features: &[] },
];

/// The frame Studio draws round a device; None for the TV, desktops,
/// consoles and headsets (Studio letterboxes them).
pub fn frame_art(id: &str) -> Option<FrameArt> {
    FRAMES.iter().find(|r| r.id == id).map(|r| FrameArt {
        family: r.family,
        bezel: insets(r.bezel),
        outer_radius: r.outer_radius,
        screen_radius: r.screen_radius,
        grey_body: r.grey_body,
        rim: r.rim,
        cutout: r.cutout,
        side_buttons: r.side_buttons,
        features: r.features,
    })
}

// ------------------------------------------------------------ dropdown

/// A row of the device dropdown.
#[derive(Clone, Debug, PartialEq)]
pub enum DeviceRef {
    /// "Current device": the window itself.
    Current,
    Device(Device),
}

#[derive(Clone, Debug, PartialEq)]
pub struct DropdownGroup {
    pub title: &'static str,
    pub items: Vec<DeviceRef>,
}

fn by_name(list: &mut [Device]) {
    list.sort_by_key(|d| d.name.to_lowercase());
}

/// The device dropdown for a form button, in Studio's groups and order
/// (A to Z by name, ignoring case; custom devices first, under "Custom").
pub fn dropdown(button: FormButton, custom: &[Device]) -> Vec<DropdownGroup> {
    let of_form = |list: &[Device]| -> Vec<Device> { list.iter().filter(|d| d.button() == button).cloned().collect() };
    let mut groups = Vec::new();
    let mut own = of_form(custom);
    by_name(&mut own);
    if !own.is_empty() {
        groups.push(DropdownGroup { title: "Custom", items: own.into_iter().map(DeviceRef::Device).collect() });
    }
    let presets = of_form(presets());
    let group = |title: &'static str, keep: &dyn Fn(&Device) -> bool| {
        let mut list: Vec<Device> = presets.iter().filter(|d| keep(d)).cloned().collect();
        by_name(&mut list);
        DropdownGroup { title, items: list.into_iter().map(DeviceRef::Device).collect() }
    };
    match button {
        FormButton::Mobile => {
            groups.push(group("Flagship", &|d| d.tier == Some(Tier::Flagship)));
            groups.push(group("Mid-range", &|d| d.tier == Some(Tier::MidRange)));
            groups.push(group("Entry level", &|d| d.tier == Some(Tier::EntryLevel)));
            groups.push(group("Other", &|d| d.tier.is_none()));
        }
        FormButton::Desktop => {
            let mut g = group("Desktop computer", &|_| true);
            g.items.insert(0, DeviceRef::Current);
            groups.push(g);
        }
        FormButton::Console => groups.push(group("Console", &|_| true)),
        FormButton::Headset => groups.push(group("Headset", &|_| true)),
    }
    groups.retain(|g| !g.items.is_empty());
    groups
}

// ------------------------------------------------------- custom devices

/// Manage devices' Memory options per form, in GB, before "Custom"
/// (measured in Studio's dialog).
pub fn memory_options(button: FormButton) -> &'static [f32] {
    match button {
        FormButton::Mobile => &[2.0, 3.0, 4.0, 6.0, 8.0, 12.0],
        FormButton::Desktop => &[4.0, 8.0, 16.0, 32.0],
        FormButton::Console => &[8.0, 10.0, 12.0, 16.0],
        FormButton::Headset => &[6.0, 8.0],
    }
}

/// A Custom memory value as Studio keeps it: at least 0.5 GB.
pub fn clamp_memory_gb(gb: f32) -> f32 {
    if gb.is_finite() { gb.max(crate::mech::DEVICE_MEMORY_MIN_GB) } else { crate::mech::DEVICE_MEMORY_MIN_GB }
}

/// The platforms Manage devices offers for a form (Studio's Platform list;
/// which form each belongs to is an estimate).
pub fn platforms(button: FormButton) -> &'static [Platform] {
    match button {
        FormButton::Mobile => &[Platform::Ios, Platform::Android],
        FormButton::Desktop => &[Platform::Windows, Platform::MacOs],
        FormButton::Console => &[Platform::XboxOne, Platform::Ps4, Platform::Ps5, Platform::Windows],
        FormButton::Headset => &[Platform::MetaOs],
    }
}

impl Platform {
    pub fn label(self) -> &'static str {
        match self {
            Platform::Ios => "iOS",
            Platform::Android => "Android",
            Platform::Windows => "Windows",
            Platform::MacOs => "macOS",
            Platform::XboxOne => "Xbox One",
            Platform::Ps4 => "PS4",
            Platform::Ps5 => "PS5",
            Platform::MetaOs => "Meta OS",
        }
    }

    fn key(self) -> &'static str {
        match self {
            Platform::Ios => "ios",
            Platform::Android => "android",
            Platform::Windows => "windows",
            Platform::MacOs => "macos",
            Platform::XboxOne => "xboxone",
            Platform::Ps4 => "ps4",
            Platform::Ps5 => "ps5",
            Platform::MetaOs => "metaos",
        }
    }

    fn from_key(k: &str) -> Option<Self> {
        [Platform::Ios, Platform::Android, Platform::Windows, Platform::MacOs, Platform::XboxOne, Platform::Ps4, Platform::Ps5, Platform::MetaOs]
            .into_iter()
            .find(|p| p.key() == k)
    }
}

/// "New device", then "New device 2", 3 … (Studio's names), counting every
/// custom device on every tab.
pub fn new_device_name(existing: &[Device]) -> String {
    let taken = |n: &str| existing.iter().any(|d| d.name == n);
    if !taken("New device") {
        return "New device".into();
    }
    (2..).map(|i| format!("New device {i}")).find(|n| !taken(n)).expect("a free name")
}

/// "{name} copy", then "{name} copy 2" … (Studio's names).
pub fn copy_name(name: &str, existing: &[Device]) -> String {
    let taken = |n: &str| existing.iter().any(|d| d.name == n) || presets().iter().any(|d| d.name == n);
    let first = format!("{name} copy");
    if !taken(&first) {
        return first;
    }
    (2..).map(|i| format!("{name} copy {i}")).find(|n| !taken(n)).expect("a free name")
}

/// A new custom device on a form's tab, with the defaults Studio's dialog
/// shows (measured: platform and memory per form, a console's 1920 x 1080
/// at 96 PPI; the other forms' sizes are estimates).
pub fn new_device(button: FormButton, existing: &[Device]) -> Device {
    let (form, platform, mem_gb, size, ppi) = match button {
        FormButton::Mobile => (Form::Phone, Platform::Ios, 4.0, [844, 390], 460.0),
        FormButton::Desktop => (Form::Desktop, Platform::Windows, 8.0, [1920, 1080], 96.0),
        FormButton::Console => (Form::Console, Platform::XboxOne, 8.0, [1920, 1080], 96.0),
        FormButton::Headset => (Form::Headset, Platform::MetaOs, 8.0, [688, 736], 1218.0),
    };
    let mut d = Device {
        id: String::new(),
        name: new_device_name(existing),
        form,
        platform,
        tier: None,
        memory_mb: (mem_gb * 1024.0) as u32,
        size,
        ppi,
        resolution_scale: 1.0,
        safe_portrait: Insets::default(),
        safe_landscape: Insets::default(),
        keyboard_height: [0, 0],
        frame: None,
        quality: 21,
        display_size: DisplaySize::Medium,
        custom: true,
    };
    d.id = custom_id(existing);
    settle(&mut d);
    d
}

fn custom_id(existing: &[Device]) -> String {
    (1..).map(|i| format!("custom_{i}")).find(|id| !existing.iter().any(|d| &d.id == id)).expect("a free id")
}

/// What a custom device's other numbers follow from its form, size and
/// density: a phone or tablet by its diagonal (7 in and up a tablet), a
/// mobile resolution scale of PPI / 160 rounded (desktops, consoles 1), and
/// the display size rule. Estimates: Studio's dialog doesn't show them.
pub fn settle(d: &mut Device) {
    if matches!(d.form, Form::Phone | Form::Tablet) {
        d.resolution_scale = (d.ppi / 160.0).round().max(1.0);
        let inches = (d.size[0] as f32).hypot(d.size[1] as f32) * d.resolution_scale / d.ppi.max(1.0);
        d.form = if inches >= 7.0 { Form::Tablet } else { Form::Phone };
    } else if d.form == Form::Headset {
        d.resolution_scale = 3.0;
    } else {
        d.resolution_scale = 1.0;
    }
    d.display_size = display_size_for(d.form, d.platform, d.size, d.resolution_scale, d.ppi);
}

/// Studio's checks on a custom device, with its messages.
pub fn validate(d: &Device, others: &[Device]) -> Result<(), String> {
    use crate::mech::*;
    let name = d.name.trim();
    if name.is_empty() {
        return Err("Name is required".into());
    }
    if name.chars().count() > DEVICE_NAME_MAX {
        return Err(format!("Name must be {DEVICE_NAME_MAX} characters or fewer"));
    }
    if presets().iter().any(|p| p.name.eq_ignore_ascii_case(name)) || name.eq_ignore_ascii_case("Current device") {
        return Err("This name is reserved".into());
    }
    if others.iter().any(|o| o.id != d.id && o.name.trim().eq_ignore_ascii_case(name)) {
        return Err("Device names must be unique".into());
    }
    let within = |v: u32, (lo, hi): (u32, u32)| (lo..=hi).contains(&v);
    if !within(d.size[0], DEVICE_WIDTH_RANGE) {
        return Err(format!("Width must be between {} and {}", DEVICE_WIDTH_RANGE.0, DEVICE_WIDTH_RANGE.1));
    }
    if !within(d.size[1], DEVICE_HEIGHT_RANGE) {
        return Err(format!("Height must be between {} and {}", DEVICE_HEIGHT_RANGE.0, DEVICE_HEIGHT_RANGE.1));
    }
    if !(d.ppi >= DEVICE_DENSITY_RANGE.0 as f32 && d.ppi <= DEVICE_DENSITY_RANGE.1 as f32) {
        return Err(format!("Pixel density must be between {} and {}", DEVICE_DENSITY_RANGE.0, DEVICE_DENSITY_RANGE.1));
    }
    if !within(d.keyboard_height[0], DEVICE_KEYBOARD_RANGE) {
        return Err(format!("Portrait keyboard height must be between {} and {}", DEVICE_KEYBOARD_RANGE.0, DEVICE_KEYBOARD_RANGE.1));
    }
    if !within(d.keyboard_height[1], DEVICE_KEYBOARD_RANGE) {
        return Err(format!("Landscape keyboard height must be between {} and {}", DEVICE_KEYBOARD_RANGE.0, DEVICE_KEYBOARD_RANGE.1));
    }
    if (d.memory_mb as f32) < DEVICE_MEMORY_MIN_GB * 1024.0 {
        return Err(format!("Memory must be {DEVICE_MEMORY_MIN_GB} GB or greater"));
    }
    Ok(())
}

/// Custom devices and the Standard devices hidden from the lists
/// (`devices.json` in Rustudio's config folder).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CustomDevices {
    pub devices: Vec<Device>,
    pub hidden_standard: Vec<String>,
}

pub fn custom_path() -> std::path::PathBuf {
    crate::paths::config().join("rustudio").join("devices.json")
}

/// Reads `devices.json`; a broken file or entry is skipped with one log
/// line, never reaching the simulator.
pub fn load_custom(path: &std::path::Path) -> CustomDevices {
    let Ok(text) = std::fs::read_to_string(path) else { return CustomDevices::default() };
    let v: serde_json::Value = match serde_json::from_str(&text) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("rustudio: Device Simulator: ignoring {}: {e}", path.display());
            return CustomDevices::default();
        }
    };
    let mut out = CustomDevices::default();
    let mut skipped = 0;
    for e in v["devices"].as_array().into_iter().flatten() {
        match device_from_json(e) {
            Some(d) if validate(&d, &out.devices).is_ok() && !out.devices.iter().any(|o| o.id == d.id) => out.devices.push(d),
            _ => skipped += 1,
        }
    }
    if skipped > 0 {
        eprintln!("rustudio: Device Simulator: skipped {skipped} bad device(s) in {}", path.display());
    }
    out.hidden_standard = v["hidden_standard"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|s| s.as_str().filter(|id| preset(id).is_some()).map(str::to_string))
        .collect();
    out
}

pub fn save_custom(path: &std::path::Path, list: &CustomDevices) -> std::io::Result<()> {
    let devices: Vec<serde_json::Value> = list.devices.iter().map(device_to_json).collect();
    let v = serde_json::json!({ "version": 1, "devices": devices, "hidden_standard": list.hidden_standard });
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_string_pretty(&v).unwrap_or_default())?;
    std::fs::rename(tmp, path)
}

fn form_key(f: Form) -> &'static str {
    match f {
        Form::Phone => "phone",
        Form::Tablet => "tablet",
        Form::Desktop => "desktop",
        Form::Console => "console",
        Form::Headset => "headset",
    }
}

fn device_to_json(d: &Device) -> serde_json::Value {
    serde_json::json!({
        "id": d.id,
        "name": d.name,
        "form": form_key(d.form),
        "platform": d.platform.key(),
        "memory_mb": d.memory_mb,
        "width": d.size[0],
        "height": d.size[1],
        "ppi": d.ppi,
        "keyboard_portrait": d.keyboard_height[0],
        "keyboard_landscape": d.keyboard_height[1],
    })
}

fn device_from_json(v: &serde_json::Value) -> Option<Device> {
    let num = |k: &str| v[k].as_f64();
    let form = match v["form"].as_str()? {
        "phone" => Form::Phone,
        "tablet" => Form::Tablet,
        "desktop" => Form::Desktop,
        "console" => Form::Console,
        "headset" => Form::Headset,
        _ => return None,
    };
    let u = |k: &str| num(k).filter(|n| *n >= 0.0 && *n <= u32::MAX as f64).map(|n| n as u32);
    let mut d = Device {
        id: v["id"].as_str()?.to_string(),
        name: v["name"].as_str()?.to_string(),
        form,
        platform: Platform::from_key(v["platform"].as_str()?)?,
        tier: None,
        memory_mb: u("memory_mb")?,
        size: [u("width")?, u("height")?],
        ppi: num("ppi")? as f32,
        resolution_scale: 1.0,
        safe_portrait: Insets::default(),
        safe_landscape: Insets::default(),
        keyboard_height: [u("keyboard_portrait").unwrap_or(0), u("keyboard_landscape").unwrap_or(0)],
        frame: None,
        quality: 21,
        display_size: DisplaySize::Medium,
        custom: true,
    };
    if !d.id.starts_with("custom_") {
        return None;
    }
    settle(&mut d);
    Some(d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_are_studios_dropdown_list() {
        assert_eq!(presets().len(), 43);
        assert!(preset("default").is_none() && preset("average_laptop").is_none());
        let mut ids: Vec<_> = presets().iter().map(|d| &d.id).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), 43);
    }

    #[test]
    fn iphone_14_row_matches_studio() {
        let d = preset("iphone_14").unwrap();
        assert_eq!((d.size, d.ppi, d.resolution_scale, d.memory_mb), ([844, 390], 460.0, 3.0, 6144));
        assert_eq!((d.form, d.platform, d.tier), (Form::Phone, Platform::Ios, Some(Tier::MidRange)));
        assert_eq!(d.safe_portrait, Insets::new(47.0, 34.0, 0.0, 0.0));
        assert_eq!(d.safe_landscape, Insets::new(0.0, 21.0, 47.0, 47.0));
        assert_eq!(d.keyboard_height, [295, 176]);
        assert_eq!(d.name, "iPhone 14");
        assert_eq!(preset("generic_handheld_720").unwrap().name, "Generic Handheld HD 720");
    }

    #[test]
    fn android_tv_keeps_its_tv_safe_zone() {
        let d = preset("android_tv_1080").unwrap();
        assert_eq!((d.form, d.platform, d.tier), (Form::Tablet, Platform::Android, None));
        assert_eq!(d.safe_landscape, Insets::new(54.0, 54.0, 96.0, 96.0));
    }

    #[test]
    fn ios_landscape_ignores_the_bottom_inset() {
        let o = Orientation::Landscape;
        assert_eq!(preset("iphone_14").unwrap().safe_insets(o), Insets::new(0.0, 0.0, 47.0, 47.0));
        assert_eq!(preset("samsung_galaxy_s22_ultra").unwrap().safe_insets(o), Insets::new(0.0, 21.0, 47.0, 47.0));
        assert_eq!(preset("iphone_14").unwrap().safe_insets(Orientation::Portrait), Insets::new(47.0, 34.0, 0.0, 0.0));
        // A console doesn't turn: portrait keeps the landscape screen.
        assert_eq!(preset("xbox").unwrap().points(Orientation::Portrait), [1920, 1080]);
    }

    #[test]
    fn logical_size_floors_like_studio() {
        assert_eq!(logical_size([844, 390], 1.5315), [843.0, 389.0]); // measured iPhone 14: 843 x 389
        assert_eq!(logical_size([1920, 1080], 718.0 / 1080.0), [1919.0, 1080.0]); // measured Xbox: 1919 x 1080
        assert_eq!(logical_size([844, 390], 1.0), [844.0, 390.0]);
        // measured desktops filling a 718 px panel (the scale as f32 math gives it)
        let k = |h: f32| f64::from(718.0f32 / h);
        assert_eq!(logical_size([1366, 768], k(768.0)), [1365.0, 768.0]); // Average Windows
        assert_eq!(logical_size([1280, 720], k(720.0)), [1279.0, 720.0]); // HD 720
        assert_eq!(logical_size([1920, 1080], k(1080.0)), [1919.0, 1080.0]); // HD 1080
    }

    #[test]
    fn flags_follow_the_device() {
        // measured in Studio's Play, the Controller Emulator closed
        let f = |id| preset(id).unwrap().flags();
        let fl = |touch, keyboard, mouse, gamepad, ten_foot, preferred| InputFlags { touch, keyboard, mouse, gamepad, ten_foot, preferred };
        use Preferred::*;
        assert_eq!(f("iphone_14"), fl(true, false, false, false, false, Touch));
        assert_eq!(f("ipad_10th_generation"), fl(true, false, false, false, false, Touch));
        assert_eq!(f("xbox"), fl(false, false, false, true, true, Gamepad));
        assert_eq!(f("ps5"), fl(false, false, false, true, true, Gamepad));
        assert_eq!(f("android_tv_1080"), fl(true, false, false, true, false, Touch)); // a TV, but not ten-foot
        assert_eq!(f("meta_quest_3"), fl(false, false, true, true, false, Gamepad));
        assert_eq!(f("generic_handheld_720"), fl(true, true, true, true, false, Gamepad));
        assert_eq!(f("hd_1080"), InputFlags::DESKTOP);
        let pads: Vec<_> = presets().iter().filter(|d| d.native_gamepad()).map(|d| d.id.as_str()).collect();
        assert_eq!(pads, ["xbox", "ps4", "ps5", "android_tv_1080", "meta_quest_2", "meta_quest_3", "generic_handheld_720", "generic_handheld_1080"]);
    }

    #[test]
    fn quality_and_display_size_match_studio() {
        // measured in Studio
        let q = |id| {
            let d = preset(id).unwrap();
            (d.quality, d.display_size)
        };
        use DisplaySize::*;
        assert_eq!(q("iphone_14"), (21, Small));
        assert_eq!(q("android_tv_1080"), (1, Large));
        assert_eq!(q("xiaomi_redmi_pad_se"), (2, Medium));
        assert_eq!(q("ipad_8th_generation"), (21, Small));
        assert_eq!(q("samsung_galaxy_tab_a9"), (14, Small));
        assert_eq!(q("hd_1080"), (17, Medium));
        assert_eq!(q("vga"), (17, Small));
        assert_eq!(q("meta_quest_2"), (14, Medium));
        assert_eq!(q("xbox"), (12, Large));
        // the fitted rule agrees with every measured row (it serves custom devices)
        for d in presets() {
            assert_eq!(display_size_for(d.form, d.platform, d.size, d.resolution_scale, d.ppi), d.display_size, "{}", d.id);
        }
    }

    #[test]
    fn frame_families_match_studios_art() {
        use FrameFamily::*;
        assert_eq!(frame_art("iphone_7").unwrap().family, HomeButtonIphone);
        assert_eq!(frame_art("iphone_14").unwrap().family, NotchIphone);
        assert_eq!(frame_art("iphone_16_pro").unwrap().family, IslandIphone);
        assert_eq!(frame_art("samsung_galaxy_a16").unwrap().family, TeardropAndroid);
        assert_eq!(frame_art("samsung_galaxy_s22_ultra").unwrap().family, PunchHoleAndroid);
        assert_eq!(frame_art("ipad_6th_generation").unwrap().family, Tablet { camera_and_home: true });
        for id in ["android_tv_1080", "xbox", "hd_1080", "meta_quest_3"] {
            assert!(frame_art(id).is_none());
        }
        // every preset with a frame image in Studio's table
        assert_eq!(presets().iter().filter(|d| frame_art(&d.id).is_some()).count(), 30);
        assert!(presets().iter().all(|d| frame_art(&d.id).is_some() == d.frame.is_some()));
    }

    #[test]
    fn iphone_14_frame_matches_the_capture() {
        // measured: a 21 pt bezel all round; the notch on the left in landscape
        let f = frame_art("iphone_14").unwrap();
        assert_eq!(f.bezel, Insets::new(21.0, 21.0, 21.0, 21.0));
        assert_eq!(f.cutout, Some([0.0, 111.5, 33.5, 166.5]));
        // the buttons stand 7.5 pt off the body, top and bottom
        let e = f.extent([844, 390]);
        assert_eq!((e.top, e.bottom, e.left, e.right), (28.5, 29.0, 21.0, 21.0));
    }

    fn names(g: &DropdownGroup) -> Vec<String> {
        g.items.iter().map(|i| match i {
            DeviceRef::Current => "Current device".to_string(),
            DeviceRef::Device(d) => d.name.clone(),
        }).collect()
    }

    #[test]
    fn dropdown_groups_match_studio() {
        let g = dropdown(FormButton::Mobile, &[]);
        assert_eq!(g.iter().map(|g| g.title).collect::<Vec<_>>(), ["Flagship", "Mid-range", "Entry level", "Other"]);
        assert_eq!(names(&g[1]), ["iPad 10th Generation", "iPad A16", "iPhone 13", "iPhone 13 Pro", "iPhone 13 Pro Max",
            "iPhone 14", "Samsung Galaxy A16", "Samsung Galaxy Tab A8", "Samsung Galaxy Tab A9"]);
        assert_eq!(names(&g[2])[..5], ["Amazon Fire HD 10 (2023)", "iPad 6th Generation", "iPad 8th Generation", "iPad 9th Generation", "iPhone 11"]);
        assert_eq!(names(&g[3]), ["Android TV 1080p"]);
        let d = dropdown(FormButton::Desktop, &[]);
        assert_eq!(d[0].title, "Desktop computer");
        assert_eq!(names(&d[0]), ["Current device", "Average MacBook", "Average Windows", "HD 1080", "HD 720", "VGA"]);
        assert_eq!(names(&dropdown(FormButton::Console, &[])[0]), ["Generic Handheld HD 1080", "Generic Handheld HD 720", "PS4", "PS5", "Xbox One"]);
        assert_eq!(names(&dropdown(FormButton::Headset, &[])[0]), ["Meta Quest 2", "Meta Quest 3"]);
        let mut mine = preset("iphone_14").unwrap().clone();
        mine.name = "My phone".into();
        mine.custom = true;
        let g = dropdown(FormButton::Mobile, &[mine]);
        assert_eq!((g[0].title, names(&g[0])), ("Custom", vec!["My phone".to_string()]));
    }

    #[test]
    fn new_device_names_count_up_across_tabs() {
        let a = new_device(FormButton::Mobile, &[]);
        assert_eq!(a.name, "New device");
        let b = new_device(FormButton::Console, std::slice::from_ref(&a));
        assert_eq!((b.name.as_str(), b.platform, b.size, b.memory_mb), ("New device 2", Platform::XboxOne, [1920, 1080], 8192));
        assert_ne!(a.id, b.id);
    }

    #[test]
    fn duplicate_names_get_copy_suffix() {
        let mut a = new_device(FormButton::Mobile, &[]);
        a.name = "Tester".into();
        assert_eq!(copy_name("Tester", std::slice::from_ref(&a)), "Tester copy");
        let mut c = a.clone();
        c.name = "Tester copy".into();
        assert_eq!(copy_name("Tester", &[a, c]), "Tester copy 2");
    }

    #[test]
    fn validation_uses_studios_messages() {
        let mut d = new_device(FormButton::Mobile, &[]);
        assert_eq!(validate(&d, &[]), Ok(()));
        d.name = " ".into();
        assert_eq!(validate(&d, &[]).unwrap_err(), "Name is required");
        d.name = "iPhone 14".into();
        assert_eq!(validate(&d, &[]).unwrap_err(), "This name is reserved");
        d.name = "x".repeat(201);
        assert_eq!(validate(&d, &[]).unwrap_err(), "Name must be 200 characters or fewer");
        d.name = "Mine".into();
        let mut other = d.clone();
        other.id = "custom_9".into();
        assert_eq!(validate(&d, &[other]).unwrap_err(), "Device names must be unique");
        d.size = [0, 390];
        assert_eq!(validate(&d, &[]).unwrap_err(), "Width must be between 1 and 7680");
        d.size = [844, 5000];
        assert_eq!(validate(&d, &[]).unwrap_err(), "Height must be between 1 and 4320");
        d.size = [844, 390];
        d.ppi = 50.0;
        assert_eq!(validate(&d, &[]).unwrap_err(), "Pixel density must be between 72 and 10000");
        d.ppi = 460.0;
        d.keyboard_height = [5000, 0];
        assert_eq!(validate(&d, &[]).unwrap_err(), "Portrait keyboard height must be between 0 and 4320");
    }

    #[test]
    fn memory_options_follow_the_form() {
        assert_eq!(memory_options(FormButton::Mobile), [2.0, 3.0, 4.0, 6.0, 8.0, 12.0]);
        assert_eq!(memory_options(FormButton::Desktop), [4.0, 8.0, 16.0, 32.0]);
        assert_eq!(memory_options(FormButton::Console), [8.0, 10.0, 12.0, 16.0]);
        assert_eq!(memory_options(FormButton::Headset), [6.0, 8.0]);
        assert_eq!(clamp_memory_gb(0.0), 0.5);
        assert_eq!(clamp_memory_gb(3.0), 3.0);
    }

    #[test]
    fn custom_devices_round_trip_and_skip_bad_entries() {
        let dir = std::env::temp_dir().join(format!("rustudio-devices-{}", std::process::id()));
        let path = dir.join("devices.json");
        let a = new_device(FormButton::Mobile, &[]);
        let list = CustomDevices { devices: vec![a.clone()], hidden_standard: vec!["ps4".into()] };
        save_custom(&path, &list).unwrap();
        assert_eq!(load_custom(&path), list);
        // broken JSON: nothing, no panic
        std::fs::write(&path, "{ not json").unwrap();
        assert_eq!(load_custom(&path), CustomDevices::default());
        // a 0x0, a huge one and a duplicate name are dropped; the good one stays
        let good = device_to_json(&a);
        let mut zero = good.clone();
        zero["id"] = "custom_2".into();
        zero["name"] = "Zero".into();
        zero["width"] = 0.into();
        zero["height"] = 0.into();
        let mut huge = good.clone();
        huge["id"] = "custom_3".into();
        huge["name"] = "Huge".into();
        huge["width"] = 100000.into();
        huge["height"] = 100000.into();
        let mut dup = good.clone();
        dup["id"] = "custom_4".into();
        let v = serde_json::json!({ "devices": [good, zero, huge, dup] });
        std::fs::write(&path, v.to_string()).unwrap();
        let back = load_custom(&path);
        assert_eq!(back.devices.len(), 1);
        assert_eq!(back.devices[0].name, "New device");
        let _ = std::fs::remove_dir_all(dir);
    }
}
