//! Roblox Studio's Experience Settings (Game Settings) dialog — reverse
//! engineered from Studio 0.748's `GameSettings.rbxm`:
//! - Layout: left category sidebar (`190px` wide, `40px` entries, `3px` blue
//!   selection indicator, dirty/error dots), right scrollable form body with
//!   Studio two-column rows & inline validation, sticky `52px` footer with
//!   Creator Hub manage link, Cancel (with Discard Changes confirmation), and
//!   primary Save button.
//! - Pages:
//!   1. **Basic Info** (Name /50, Description /1000, Icon upload & preview,
//!      Screenshots & Media upload/reorder/alt-text/delete, Genre & Content
//!      Maturity Guidelines, Playable Devices w/ Console agreement modal,
//!      Team Create & Place Info).
//!   2. **Permissions** (Public / Friends / Private playability + collaborators
//!      summary & Creator Hub link).
//!   3. **Communication** (Microphone / Spatial Voice, Camera / Avatar Video,
//!      Chat Group API via `voice.roblox.com`).
//!   4. **Security** (Local `HttpService.HttpEnabled` & `AssetService.AllowInsertFreeAssets`,
//!      Cloud API Services / 3rd-party Sales & Teleports / Mesh & Image APIs
//!      w/ AMP ID-verification status, and Local Secrets table + Edit Secret
//!      sub-view in Studio's exact `StudioService.Secrets` JSON format).
//!   5. **Monetization** (Paid Access + marketplace tax calculator, Private
//!      Servers + active server/subscription counts, Rewarded Ads, Developer
//!      Products list + Create/Edit, Badges list).
//!   6. **World** (Offline-capable local `Workspace.Gravity` w/ Studio presets,
//!      `StarterPlayer` WalkSpeed / JumpHeight / JumpPower / MaxSlopeAngle /
//!      AutoJump, and Avatar Type R15 / R6 / Player Choice).
//!   7. **Additional Settings** (Studio 0.748's `CreatorHubPage` deep links).

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};

use egui::{Color32, CornerRadius, FontId, Pos2, Rect, RichText, Sense, Stroke, Vec2};
use rbx_dom_weak::types::{Ref, Variant};
use rustudio_account::game_settings::{
    creator_hub, format_genre_slug, ConfigurationPatch, DeveloperProduct, ExperienceThumbnail,
    GameSettingsSnapshot, MeshTextureAmpStatus, Playability, LEGACY_GENRES, PLAYABLE_DEVICES,
};
use rustudio_account::VersionType;
use rustudio_engine::theme;

use crate::app::Editor;
use crate::file_dialog::{Kind, Purpose};

const SIDEBAR_WIDTH: f32 = 192.0;
const ENTRY_HEIGHT: f32 = 38.0;
const FOOTER_HEIGHT: f32 = 52.0;
const LABEL_COL_WIDTH: f32 = 178.0;
const NAME_LIMIT: usize = 50;
const DESC_LIMIT: usize = 1000;
const MAX_THUMBNAILS: usize = 10;
const MAX_IMAGE_BYTES: usize = 20 * 1024 * 1024;
const STUDIO_BLUE: Color32 = Color32::from_rgb(0, 162, 255);
const STUDIO_BLUE_HOVER: Color32 = Color32::from_rgb(38, 178, 255);
const STUDIO_GREEN: Color32 = Color32::from_rgb(52, 199, 89);

/// Sidebar pages in Studio order (with Permissions, Monetization & World
/// integrated natively so users don't have to leave rusTudio).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SettingsPage {
    BasicInfo,
    Permissions,
    Communication,
    Security,
    Monetization,
    World,
    AdditionalSettings,
}

impl SettingsPage {
    pub const ALL: [SettingsPage; 7] = [
        SettingsPage::BasicInfo,
        SettingsPage::Permissions,
        SettingsPage::Communication,
        SettingsPage::Security,
        SettingsPage::Monetization,
        SettingsPage::World,
        SettingsPage::AdditionalSettings,
    ];

    pub fn label(self) -> &'static str {
        match self {
            SettingsPage::BasicInfo => "Basic Info",
            SettingsPage::Permissions => "Permissions",
            SettingsPage::Communication => "Communication",
            SettingsPage::Security => "Security",
            SettingsPage::Monetization => "Monetization",
            SettingsPage::World => "World",
            SettingsPage::AdditionalSettings => "Additional Settings",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            SettingsPage::BasicInfo => "info-circle",
            SettingsPage::Permissions => "lock",
            SettingsPage::Communication => "microphone",
            SettingsPage::Security => "shield",
            SettingsPage::Monetization => "coin",
            SettingsPage::World => "world",
            SettingsPage::AdditionalSettings => "external-link",
        }
    }

    pub fn parse(s: &str) -> Option<SettingsPage> {
        match s.trim().to_ascii_lowercase().as_str() {
            "basic" | "basicinfo" | "basic_info" => Some(SettingsPage::BasicInfo),
            "permissions" | "perms" => Some(SettingsPage::Permissions),
            "communication" | "comm" | "voice" => Some(SettingsPage::Communication),
            "security" | "secrets" => Some(SettingsPage::Security),
            "monetization" | "money" => Some(SettingsPage::Monetization),
            "world" | "physics" => Some(SettingsPage::World),
            "additional" | "creatorhub" | "creator_hub" => Some(SettingsPage::AdditionalSettings),
            _ => None,
        }
    }
}

/// One row in Studio's `StudioService.Secrets` local secrets table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecretRow {
    pub name: String,
    /// Base64-encoded secret value (or `""` if blank), matching `SecretUtils.lua`.
    pub encoded_value: String,
    /// Allowed domain pattern (`"*"` by default).
    pub domain: String,
    /// Marks whether the value was changed in the current unsaved draft.
    pub updated: bool,
}

/// Inline draft while editing/creating a local secret (`EditSecret` sub-view).
#[derive(Debug, Clone)]
struct SecretDraft {
    /// `None` = creating a new secret; `Some(idx)` = editing row `idx`.
    index: Option<usize>,
    name: String,
    new_value: String,
    domain: String,
    error: Option<String>,
}

/// Inline form for creating or editing a Developer Product on the Monetization tab.
#[derive(Debug, Clone)]
struct DevProductDraft {
    /// `None` = creating new; `Some(product_id)` = updating existing.
    product_id: Option<u64>,
    name: String,
    price_text: String,
    description: String,
    error: Option<String>,
    busy: bool,
}

/// Pending local image upload (icon or screenshot) before Save.
#[derive(Debug, Clone)]
struct PendingImage {
    filename: String,
    bytes: Vec<u8>,
    preview: Option<egui::ColorImage>,
}

/// Local DataModel settings captured when opening Experience Settings.
#[derive(Debug, Clone, PartialEq)]
struct LocalDraft {
    http_enabled: bool,
    allow_insert_free_assets: bool,
    gravity: f32,
    walk_speed: f32,
    use_jump_power: bool,
    jump_power: f32,
    jump_height: f32,
    max_slope_angle: f32,
    auto_jump: bool,
    avatar_type: u32,
}

impl Default for LocalDraft {
    fn default() -> Self {
        Self {
            http_enabled: false,
            allow_insert_free_assets: false,
            gravity: 196.2,
            walk_speed: 16.0,
            use_jump_power: false,
            jump_power: 50.0,
            jump_height: 7.2,
            max_slope_angle: 89.0,
            auto_jump: true,
            avatar_type: 1,
        }
    }
}

/// Editable cloud draft copied from `GameSettingsSnapshot`.
#[derive(Debug, Clone, PartialEq)]
struct CloudDraft {
    name: String,
    description: String,
    genre: String,
    playable_devices: Vec<String>,
    playability: Playability,
    audio_enabled: bool,
    video_enabled: bool,
    chat_group_api_enabled: bool,
    studio_access_to_apis_allowed: bool,
    allow_third_party_sales: bool,
    allow_third_party_teleports: bool,
    allow_mesh_texture_apis: bool,
    is_for_sale: bool,
    price_text: String,
    allow_private_servers: bool,
    private_server_price_text: String,
    allow_rewarded_ads: bool,
    thumbnails: Vec<ExperienceThumbnail>,
    deleted_thumbnails: Vec<u64>,
}

impl CloudDraft {
    fn from_snapshot(s: &GameSettingsSnapshot) -> Self {
        Self {
            name: s.config.name.clone(),
            description: s.config.description.clone(),
            genre: if s.config.genre.is_empty() { "All".into() } else { s.config.genre.clone() },
            playable_devices: s.config.playable_devices.clone(),
            playability: s.config.playability(),
            audio_enabled: s.communication.audio_enabled,
            video_enabled: s.communication.video_enabled,
            chat_group_api_enabled: s.communication.chat_group_api_enabled,
            studio_access_to_apis_allowed: s.config.studio_access_to_apis_allowed,
            allow_third_party_sales: s.config.allow_third_party_sales,
            allow_third_party_teleports: s.config.allow_third_party_teleports,
            allow_mesh_texture_apis: s.config.allow_mesh_texture_apis,
            is_for_sale: s.config.is_for_sale,
            price_text: s.config.price.map(|p| p.to_string()).unwrap_or_else(|| "25".into()),
            allow_private_servers: s.config.allow_private_servers,
            private_server_price_text: s.config.private_server_price.map(|p| p.to_string()).unwrap_or_else(|| "0".into()),
            allow_rewarded_ads: s.config.allow_rewarded_ads,
            thumbnails: s.thumbnails.clone(),
            deleted_thumbnails: Vec::new(),
        }
    }

    fn parsed_price(&self) -> Option<u64> {
        self.price_text.trim().parse::<u64>().ok()
    }

    fn parsed_private_server_price(&self) -> Option<u64> {
        self.private_server_price_text.trim().parse::<u64>().ok()
    }
}

/// Async results from worker threads back to the UI thread.
enum WorkerMsg {
    Loaded {
        universe_id: u64,
        result: Result<GameSettingsSnapshot, String>,
    },
    ImageLoaded {
        asset_id: u64,
        image: egui::ColorImage,
    },
    Saved {
        universe_id: u64,
        result: Result<Option<GameSettingsSnapshot>, SaveFailure>,
    },
    DevProductSaved {
        universe_id: u64,
        result: Result<Vec<DeveloperProduct>, String>,
    },
}

#[derive(Debug, Clone)]
struct SaveFailure {
    message: String,
    name_moderated: bool,
    description_moderated: bool,
    vip_price_cooldown: Option<String>,
}

pub struct GameSettingsState {
    pub page: SettingsPage,
    loaded_universe: Option<u64>,
    loading: bool,
    load_error: Option<String>,
    snapshot: Option<GameSettingsSnapshot>,
    cloud_draft: Option<CloudDraft>,

    local_base: LocalDraft,
    local_draft: LocalDraft,

    secrets_base: Vec<SecretRow>,
    secrets_draft: Vec<SecretRow>,
    secret_edit: Option<SecretDraft>,

    pending_icon: Option<PendingImage>,
    pending_icon_tex: Option<egui::TextureHandle>,
    pending_thumbs: Vec<PendingImage>,
    pending_thumb_tex: Vec<Option<egui::TextureHandle>>,

    /// Downloaded Roblox asset thumbnails (`image_id` -> texture).
    remote_textures: HashMap<u64, egui::TextureHandle>,
    remote_requested: HashMap<u64, bool>,

    saving: bool,
    save_status: Option<Result<String, String>>,
    name_moderated_err: bool,
    desc_moderated_err: bool,
    vip_cooldown_err: Option<String>,

    /// Confirmation dialogs matching Studio's modals.
    confirm_discard: bool,
    confirm_console: bool,
    dev_product_draft: Option<DevProductDraft>,

    tx: Sender<WorkerMsg>,
    rx: Receiver<WorkerMsg>,
}

impl Default for GameSettingsState {
    fn default() -> Self {
        let (tx, rx) = channel();
        Self {
            page: SettingsPage::BasicInfo,
            loaded_universe: None,
            loading: false,
            load_error: None,
            snapshot: None,
            cloud_draft: None,
            local_base: LocalDraft::default(),
            local_draft: LocalDraft::default(),
            secrets_base: Vec::new(),
            secrets_draft: Vec::new(),
            secret_edit: None,
            pending_icon: None,
            pending_icon_tex: None,
            pending_thumbs: Vec::new(),
            pending_thumb_tex: Vec::new(),
            remote_textures: HashMap::new(),
            remote_requested: HashMap::new(),
            saving: false,
            save_status: None,
            name_moderated_err: false,
            desc_moderated_err: false,
            vip_cooldown_err: None,
            confirm_discard: false,
            confirm_console: false,
            dev_product_draft: None,
            tx,
            rx,
        }
    }
}

// ------------------------------------------------------------------ Local Secrets disk store

fn secrets_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("rustudio")
        .join("local-secrets.json")
}

/// Loads Studio-compatible `StudioService.Secrets` JSON (`{ "name": ["base64", "domain"] }`).
pub fn load_local_secrets() -> Vec<SecretRow> {
    let Ok(text) = std::fs::read_to_string(secrets_path()) else {
        return Vec::new();
    };
    parse_secrets_json(&text)
}

/// Saves Studio-compatible `StudioService.Secrets` JSON (`{ "name": ["base64", "domain"] }`).
pub fn save_local_secrets(rows: &[SecretRow]) -> Result<(), String> {
    let path = secrets_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let text = encode_secrets_json(rows);
    std::fs::write(&path, text).map_err(|e| e.to_string())
}

pub fn parse_secrets_json(text: &str) -> Vec<SecretRow> {
    let Ok(serde_json::Value::Object(map)) = serde_json::from_str::<serde_json::Value>(text) else {
        return Vec::new();
    };
    let mut rows: Vec<SecretRow> = map
        .into_iter()
        .filter_map(|(name, val)| {
            let arr = val.as_array()?;
            let encoded = arr.first().and_then(|v| v.as_str()).unwrap_or("").to_string();
            let domain = arr
                .get(1)
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .unwrap_or("*")
                .to_string();
            Some(SecretRow { name, encoded_value: encoded, domain, updated: false })
        })
        .collect();
    rows.sort_by(|a, b| a.name.to_ascii_lowercase().cmp(&b.name.to_ascii_lowercase()));
    rows
}

pub fn encode_secrets_json(rows: &[SecretRow]) -> String {
    let mut map: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for r in rows {
        let dom = if r.domain.trim().is_empty() { "*" } else { r.domain.trim() };
        map.insert(r.name.clone(), vec![r.encoded_value.clone(), dom.to_string()]);
    }
    serde_json::to_string_pretty(&map).unwrap_or_else(|_| "{}".into())
}

fn base64_encode(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = *chunk.get(1).unwrap_or(&0) as u32;
        let b2 = *chunk.get(2).unwrap_or(&0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(TABLE[((n >> 18) & 63) as usize] as char);
        out.push(TABLE[((n >> 12) & 63) as usize] as char);
        out.push(if chunk.len() > 1 { TABLE[((n >> 6) & 63) as usize] as char } else { '=' });
        out.push(if chunk.len() > 2 { TABLE[(n & 63) as usize] as char } else { '=' });
    }
    out
}

/// Implements Studio's exact `SecretUtils.transformToStorableFormat(newValue, oldEncoded)`:
/// - `""` keeps the previous encoded value
/// - `"<blank>"` stores `""`
/// - anything else is base64-encoded
fn transform_secret_value(input: &str, old_encoded: &str) -> (String, bool) {
    if input.is_empty() {
        (old_encoded.to_string(), false)
    } else if input == "<blank>" {
        (String::new(), true)
    } else {
        (base64_encode(input.as_bytes()), true)
    }
}

fn decode_color_image(bytes: &[u8]) -> Option<egui::ColorImage> {
    let img = image::load_from_memory(bytes).ok()?.to_rgba8();
    let size = [img.width() as usize, img.height() as usize];
    Some(egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw()))
}

// ------------------------------------------------------------------ Dirty & validation logic

impl GameSettingsState {
    fn cloud_dirty(&self) -> bool {
        let (Some(snap), Some(draft)) = (&self.snapshot, &self.cloud_draft) else {
            return false;
        };
        if !snap.can_manage_config {
            return false;
        }
        if self.pending_icon.is_some() || !self.pending_thumbs.is_empty() {
            return true;
        }
        let base = CloudDraft::from_snapshot(snap);
        *draft != base
    }

    fn local_dirty(&self) -> bool {
        self.local_draft != self.local_base
    }

    fn secrets_dirty(&self) -> bool {
        self.secrets_draft != self.secrets_base
    }

    pub fn is_dirty(&self) -> bool {
        self.cloud_dirty() || self.local_dirty() || self.secrets_dirty()
    }

    fn page_dirty(&self, page: SettingsPage) -> bool {
        match page {
            SettingsPage::BasicInfo => {
                if self.pending_icon.is_some() || !self.pending_thumbs.is_empty() {
                    return true;
                }
                if let (Some(snap), Some(d)) = (&self.snapshot, &self.cloud_draft) {
                    let b = CloudDraft::from_snapshot(snap);
                    d.name != b.name
                        || d.description != b.description
                        || d.genre != b.genre
                        || d.playable_devices != b.playable_devices
                        || d.thumbnails != b.thumbnails
                        || !d.deleted_thumbnails.is_empty()
                } else {
                    false
                }
            }
            SettingsPage::Permissions => {
                if let (Some(snap), Some(d)) = (&self.snapshot, &self.cloud_draft) {
                    d.playability != snap.config.playability()
                } else {
                    false
                }
            }
            SettingsPage::Communication => {
                if let (Some(snap), Some(d)) = (&self.snapshot, &self.cloud_draft) {
                    d.audio_enabled != snap.communication.audio_enabled
                        || d.video_enabled != snap.communication.video_enabled
                        || d.chat_group_api_enabled != snap.communication.chat_group_api_enabled
                } else {
                    false
                }
            }
            SettingsPage::Security => {
                let local = self.local_draft.http_enabled != self.local_base.http_enabled
                    || self.local_draft.allow_insert_free_assets != self.local_base.allow_insert_free_assets
                    || self.secrets_dirty();
                let cloud = if let (Some(snap), Some(d)) = (&self.snapshot, &self.cloud_draft) {
                    d.studio_access_to_apis_allowed != snap.config.studio_access_to_apis_allowed
                        || d.allow_third_party_sales != snap.config.allow_third_party_sales
                        || d.allow_third_party_teleports != snap.config.allow_third_party_teleports
                        || d.allow_mesh_texture_apis != snap.config.allow_mesh_texture_apis
                } else {
                    false
                };
                local || cloud
            }
            SettingsPage::Monetization => {
                if let (Some(snap), Some(d)) = (&self.snapshot, &self.cloud_draft) {
                    let b = CloudDraft::from_snapshot(snap);
                    d.is_for_sale != b.is_for_sale
                        || d.price_text != b.price_text
                        || d.allow_private_servers != b.allow_private_servers
                        || d.private_server_price_text != b.private_server_price_text
                        || d.allow_rewarded_ads != b.allow_rewarded_ads
                } else {
                    false
                }
            }
            SettingsPage::World => {
                let a = &self.local_draft;
                let b = &self.local_base;
                (a.gravity - b.gravity).abs() > 1e-3
                    || (a.walk_speed - b.walk_speed).abs() > 1e-3
                    || a.use_jump_power != b.use_jump_power
                    || (a.jump_power - b.jump_power).abs() > 1e-3
                    || (a.jump_height - b.jump_height).abs() > 1e-3
                    || (a.max_slope_angle - b.max_slope_angle).abs() > 1e-3
                    || a.auto_jump != b.auto_jump
                    || a.avatar_type != b.avatar_type
            }
            SettingsPage::AdditionalSettings => false,
        }
    }

    fn page_error(&self, page: SettingsPage) -> Option<String> {
        let d = self.cloud_draft.as_ref()?;
        match page {
            SettingsPage::BasicInfo => {
                if d.name.trim().is_empty() {
                    return Some("Experience name cannot be empty.".into());
                }
                if d.name.chars().count() > NAME_LIMIT {
                    return Some(format!("Name cannot exceed {NAME_LIMIT} characters."));
                }
                if self.name_moderated_err {
                    return Some("Name was moderated by Roblox.".into());
                }
                if d.description.chars().count() > DESC_LIMIT {
                    return Some(format!("Description cannot exceed {DESC_LIMIT} characters."));
                }
                if self.desc_moderated_err {
                    return Some("Description was moderated by Roblox.".into());
                }
                if d.playable_devices.is_empty() {
                    return Some("Select at least one playable device.".into());
                }
                None
            }
            SettingsPage::Monetization => {
                if d.is_for_sale {
                    match d.parsed_price() {
                        Some(p) if (25..=1000).contains(&p) => {}
                        _ => return Some("Paid Access price must be between 25 and 1,000 Robux.".into()),
                    }
                }
                if d.allow_private_servers {
                    match d.parsed_private_server_price() {
                        Some(0) => {}
                        Some(p) if p >= 10 => {}
                        _ => return Some("Private server price must be 0 (Free) or at least 10 Robux.".into()),
                    }
                }
                None
            }
            _ => None,
        }
    }

    fn has_any_error(&self) -> bool {
        SettingsPage::ALL.iter().any(|&p| self.page_error(p).is_some())
    }

    fn build_config_patch(&self) -> ConfigurationPatch {
        let (Some(snap), Some(d)) = (&self.snapshot, &self.cloud_draft) else {
            return ConfigurationPatch::default();
        };
        let b = CloudDraft::from_snapshot(snap);
        let mut patch = ConfigurationPatch::default();
        if d.name != b.name {
            patch.name = Some(d.name.clone());
        }
        if d.description != b.description {
            patch.description = Some(d.description.clone());
        }
        if d.genre != b.genre {
            patch.genre = Some(d.genre.clone());
        }
        if d.playable_devices != b.playable_devices {
            patch.playable_devices = Some(d.playable_devices.clone());
        }
        if d.playability.is_friends_only() != b.playability.is_friends_only() && d.playability.is_active() {
            patch.is_friends_only = Some(d.playability.is_friends_only());
        }
        if d.is_for_sale != b.is_for_sale {
            patch.is_for_sale = Some(d.is_for_sale);
        }
        if d.is_for_sale && d.price_text != b.price_text {
            patch.price = d.parsed_price();
        }
        if d.allow_private_servers != b.allow_private_servers {
            patch.allow_private_servers = Some(d.allow_private_servers);
        }
        if d.allow_private_servers && d.private_server_price_text != b.private_server_price_text {
            patch.private_server_price = d.parsed_private_server_price();
        }
        if d.studio_access_to_apis_allowed != b.studio_access_to_apis_allowed {
            patch.studio_access_to_apis_allowed = Some(d.studio_access_to_apis_allowed);
        }
        if d.allow_third_party_sales != b.allow_third_party_sales {
            patch.allow_third_party_sales = Some(d.allow_third_party_sales);
        }
        if d.allow_third_party_teleports != b.allow_third_party_teleports {
            patch.allow_third_party_teleports = Some(d.allow_third_party_teleports);
        }
        if d.allow_mesh_texture_apis != b.allow_mesh_texture_apis {
            patch.allow_mesh_texture_apis = Some(d.allow_mesh_texture_apis);
        }
        if d.allow_rewarded_ads != b.allow_rewarded_ads {
            patch.allow_rewarded_ads = Some(d.allow_rewarded_ads);
        }
        patch
    }

    pub fn reset_for_new_place(&mut self) {
        self.loaded_universe = None;
        self.loading = false;
        self.load_error = None;
        self.snapshot = None;
        self.cloud_draft = None;
        self.pending_icon = None;
        self.pending_icon_tex = None;
        self.pending_thumbs.clear();
        self.pending_thumb_tex.clear();
        self.save_status = None;
        self.secret_edit = None;
        self.dev_product_draft = None;
        self.confirm_discard = false;
        self.confirm_console = false;
    }
}

// ------------------------------------------------------------------ Editor integration

impl Editor {
    fn find_service_ref(&self, class_name: &str) -> Option<Ref> {
        let root = self.game.doc.dom.root_ref();
        let root_inst = self.game.doc.dom.get_by_ref(root)?;
        root_inst.children().iter().copied().find(|&c| {
            self.game
                .doc
                .dom
                .get_by_ref(c)
                .is_some_and(|inst| inst.class == class_name)
        })
    }

    fn read_local_draft(&self) -> LocalDraft {
        let mut d = LocalDraft::default();
        if let Some(hs) = self.find_service_ref("HttpService") {
            if let Some(Variant::Bool(b)) = self.game.doc.prop(hs, "HttpEnabled") {
                d.http_enabled = b;
            }
        }
        if let Some(asvc) = self.find_service_ref("AssetService") {
            if let Some(Variant::Bool(b)) = self.game.doc.prop(asvc, "AllowInsertFreeAssets") {
                d.allow_insert_free_assets = b;
            }
        }
        if let Some(ws) = self.find_service_ref("Workspace") {
            if let Some(Variant::Float32(g)) = self.game.doc.prop(ws, "Gravity") {
                d.gravity = g;
            }
        }
        if let Some(sp) = self.find_service_ref("StarterPlayer") {
            if let Some(Variant::Float32(v)) = self.game.doc.prop(sp, "CharacterWalkSpeed") {
                d.walk_speed = v;
            }
            if let Some(Variant::Bool(v)) = self.game.doc.prop(sp, "CharacterUseJumpPower") {
                d.use_jump_power = v;
            }
            if let Some(Variant::Float32(v)) = self.game.doc.prop(sp, "CharacterJumpPower") {
                d.jump_power = v;
            }
            if let Some(Variant::Float32(v)) = self.game.doc.prop(sp, "CharacterJumpHeight") {
                d.jump_height = v;
            }
            if let Some(Variant::Float32(v)) = self.game.doc.prop(sp, "CharacterMaxSlopeAngle") {
                d.max_slope_angle = v;
            }
            if let Some(Variant::Bool(v)) = self.game.doc.prop(sp, "AutoJumpEnabled") {
                d.auto_jump = v;
            }
        }
        d.avatar_type = self.avatar_type();
        d
    }

    fn apply_local_draft(&mut self) {
        let d = self.game_settings.local_draft.clone();
        let b = self.game_settings.local_base.clone();
        let mut edits: Vec<(Ref, rbx_dom_weak::Ustr, Variant)> = Vec::new();
        let root = self.game.doc.dom.root_ref();

        if d.http_enabled != b.http_enabled {
            let hs = self.find_service_ref("HttpService").unwrap_or_else(|| {
                let ib = rbx_dom_weak::InstanceBuilder::new("HttpService").with_name("HttpService");
                self.game.doc.dom.insert(root, ib)
            });
            edits.push((hs, rbx_dom_weak::ustr("HttpEnabled"), Variant::Bool(d.http_enabled)));
        }
        if d.allow_insert_free_assets != b.allow_insert_free_assets {
            let asvc = self.find_service_ref("AssetService").unwrap_or_else(|| {
                let ib = rbx_dom_weak::InstanceBuilder::new("AssetService").with_name("AssetService");
                self.game.doc.dom.insert(root, ib)
            });
            edits.push((
                asvc,
                rbx_dom_weak::ustr("AllowInsertFreeAssets"),
                Variant::Bool(d.allow_insert_free_assets),
            ));
        }
        if (d.gravity - b.gravity).abs() > 1e-3 {
            if let Some(ws) = self.find_service_ref("Workspace") {
                edits.push((ws, rbx_dom_weak::ustr("Gravity"), Variant::Float32(d.gravity)));
            }
        }
        if let Some(sp) = self.find_service_ref("StarterPlayer") {
            if (d.walk_speed - b.walk_speed).abs() > 1e-3 {
                edits.push((sp, rbx_dom_weak::ustr("CharacterWalkSpeed"), Variant::Float32(d.walk_speed)));
            }
            if d.use_jump_power != b.use_jump_power {
                edits.push((sp, rbx_dom_weak::ustr("CharacterUseJumpPower"), Variant::Bool(d.use_jump_power)));
            }
            if (d.jump_power - b.jump_power).abs() > 1e-3 {
                edits.push((sp, rbx_dom_weak::ustr("CharacterJumpPower"), Variant::Float32(d.jump_power)));
            }
            if (d.jump_height - b.jump_height).abs() > 1e-3 {
                edits.push((sp, rbx_dom_weak::ustr("CharacterJumpHeight"), Variant::Float32(d.jump_height)));
            }
            if (d.max_slope_angle - b.max_slope_angle).abs() > 1e-3 {
                edits.push((
                    sp,
                    rbx_dom_weak::ustr("CharacterMaxSlopeAngle"),
                    Variant::Float32(d.max_slope_angle),
                ));
            }
            if d.auto_jump != b.auto_jump {
                edits.push((sp, rbx_dom_weak::ustr("AutoJumpEnabled"), Variant::Bool(d.auto_jump)));
            }
        }
        if !edits.is_empty() {
            self.game.doc.set_props("Experience Settings", edits, None);
        }
        if d.avatar_type != b.avatar_type {
            self.set_avatar_type(d.avatar_type);
        }
        self.game_settings.local_base = d;
    }

    /// Opens Experience Settings (File > Experience Settings or Home > Settings).
    pub fn open_game_settings(&mut self) {
        self.cloud.settings_open = true;
        let local = self.read_local_draft();
        self.game_settings.local_base = local.clone();
        self.game_settings.local_draft = local;
        let secrets = load_local_secrets();
        self.game_settings.secrets_base = secrets.clone();
        self.game_settings.secrets_draft = secrets;
        self.game_settings.save_status = None;
        self.game_settings.secret_edit = None;
        self.game_settings.confirm_discard = false;

        if let Some(o) = self.game.doc.cloud.clone() {
            if self.game_settings.loaded_universe != Some(o.universe_id) || self.game_settings.snapshot.is_none() {
                self.fetch_game_settings_async(o.universe_id, o.root_place_id);
            }
        }
    }

    fn fetch_game_settings_async(&mut self, universe_id: u64, root_place_id: u64) {
        self.game_settings.loaded_universe = Some(universe_id);
        self.game_settings.loading = true;
        self.game_settings.load_error = None;
        let api = self.cloud.factory(self.game.account.shared_client());
        let tx = self.game_settings.tx.clone();
        std::thread::spawn(move || {
            let result = match api() {
                Some(a) => a.fetch_game_settings(universe_id, root_place_id).map_err(|e| e.0),
                None => Err("Sign in to your Roblox account to load and edit cloud experience settings.".into()),
            };
            let _ = tx.send(WorkerMsg::Loaded { universe_id, result });
        });
    }

    fn request_remote_asset_image(&mut self, asset_id: u64, size: &'static str) {
        if asset_id == 0
            || self.game_settings.remote_textures.contains_key(&asset_id)
            || self.game_settings.remote_requested.contains_key(&asset_id)
        {
            return;
        }
        self.game_settings.remote_requested.insert(asset_id, true);
        let api = self.cloud.factory(self.game.account.shared_client());
        let tx = self.game_settings.tx.clone();
        std::thread::spawn(move || {
            let Some(a) = api() else { return };
            if let Ok(bytes) = a.fetch_asset_thumbnail_bytes(asset_id, size) {
                if let Some(image) = decode_color_image(&bytes) {
                    let _ = tx.send(WorkerMsg::ImageLoaded { asset_id, image });
                }
            }
        });
    }

    /// Handles a picked local image file for Experience Icon or Screenshot.
    pub fn game_settings_picked_image(&mut self, is_icon: bool, path: PathBuf) {
        let Ok(bytes) = std::fs::read(&path) else {
            self.game_settings.save_status = Some(Err(format!("Could not read {}", path.display())));
            return;
        };
        if bytes.len() > MAX_IMAGE_BYTES {
            self.game_settings.save_status = Some(Err("Image exceeds Roblox's 20 MB upload limit.".into()));
            return;
        }
        let filename = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("image.png")
            .to_string();
        let preview = decode_color_image(&bytes);
        let pending = PendingImage { filename, bytes, preview };
        if is_icon {
            self.game_settings.pending_icon = Some(pending);
            self.game_settings.pending_icon_tex = None;
        } else {
            self.game_settings.pending_thumbs.push(pending);
            self.game_settings.pending_thumb_tex.push(None);
        }
    }

    fn poll_game_settings(&mut self, ctx: &egui::Context) {
        while let Ok(msg) = self.game_settings.rx.try_recv() {
            match msg {
                WorkerMsg::Loaded { universe_id, result } => {
                    if self.game_settings.loaded_universe != Some(universe_id) {
                        continue;
                    }
                    self.game_settings.loading = false;
                    match result {
                        Ok(snap) => {
                            self.cloud.status = Some((universe_id, snap.team_create.clone()));
                            let draft = CloudDraft::from_snapshot(&snap);
                            let icon_id = snap.icon.image_id;
                            let thumb_ids: Vec<u64> =
                                snap.thumbnails.iter().filter_map(|t| t.image_id).collect();
                            self.game_settings.snapshot = Some(snap);
                            self.game_settings.cloud_draft = Some(draft);
                            self.game_settings.load_error = None;
                            if let Some(id) = icon_id {
                                self.request_remote_asset_image(id, "150x150");
                            }
                            for id in thumb_ids {
                                self.request_remote_asset_image(id, "420x420");
                            }
                        }
                        Err(e) => {
                            self.game_settings.load_error = Some(e);
                        }
                    }
                }
                WorkerMsg::ImageLoaded { asset_id, image } => {
                    let tex = ctx.load_texture(
                        format!("gs_img_{asset_id}"),
                        image,
                        egui::TextureOptions::LINEAR,
                    );
                    self.game_settings.remote_textures.insert(asset_id, tex);
                }
                WorkerMsg::Saved { universe_id, result } => {
                    self.game_settings.saving = false;
                    match result {
                        Ok(new_snap) => {
                            self.game_settings.name_moderated_err = false;
                            self.game_settings.desc_moderated_err = false;
                            self.game_settings.vip_cooldown_err = None;
                            self.game_settings.pending_icon = None;
                            self.game_settings.pending_icon_tex = None;
                            self.game_settings.pending_thumbs.clear();
                            self.game_settings.pending_thumb_tex.clear();
                            if let Some(snap) = new_snap {
                                if let Some(origin) = self.game.doc.cloud.as_mut() {
                                    if origin.universe_id == universe_id && !snap.config.name.is_empty() {
                                        origin.experience = snap.config.name.clone();
                                    }
                                }
                                let icon_id = snap.icon.image_id;
                                let thumb_ids: Vec<u64> =
                                    snap.thumbnails.iter().filter_map(|t| t.image_id).collect();
                                self.game_settings.cloud_draft = Some(CloudDraft::from_snapshot(&snap));
                                self.game_settings.snapshot = Some(snap);
                                if let Some(id) = icon_id {
                                    self.game_settings.remote_textures.remove(&id);
                                    self.game_settings.remote_requested.remove(&id);
                                    self.request_remote_asset_image(id, "150x150");
                                }
                                for id in thumb_ids {
                                    self.request_remote_asset_image(id, "420x420");
                                }
                            }
                            self.game_settings.save_status = Some(Ok("Experience settings saved.".into()));
                            self.info("Saved Experience Settings.");
                        }
                        Err(fail) => {
                            self.game_settings.name_moderated_err = fail.name_moderated;
                            self.game_settings.desc_moderated_err = fail.description_moderated;
                            self.game_settings.vip_cooldown_err = fail.vip_price_cooldown;
                            self.game_settings.save_status = Some(Err(fail.message.clone()));
                            self.warn(format!("Experience Settings save failed: {}", fail.message));
                        }
                    }
                }
                WorkerMsg::DevProductSaved { universe_id, result } => {
                    if let Some(dp) = self.game_settings.dev_product_draft.as_mut() {
                        dp.busy = false;
                    }
                    match result {
                        Ok(products) => {
                            if let Some(snap) = self.game_settings.snapshot.as_mut() {
                                if snap.config.universe_id == universe_id {
                                    snap.monetization.dev_products = products;
                                }
                            }
                            self.game_settings.dev_product_draft = None;
                            self.game_settings.save_status = Some(Ok("Developer product saved.".into()));
                        }
                        Err(e) => {
                            if let Some(dp) = self.game_settings.dev_product_draft.as_mut() {
                                dp.error = Some(e);
                            }
                        }
                    }
                }
            }
            ctx.request_repaint();
        }
    }

    fn save_game_settings(&mut self) {
        self.game_settings.save_status = None;
        self.game_settings.name_moderated_err = false;
        self.game_settings.desc_moderated_err = false;
        self.game_settings.vip_cooldown_err = None;

        // 1. Apply local DataModel settings (undoable)
        if self.game_settings.local_dirty() {
            self.apply_local_draft();
        }

        // 2. Persist Local Secrets (StudioService.Secrets format)
        if self.game_settings.secrets_dirty() {
            for r in &mut self.game_settings.secrets_draft {
                r.updated = false;
            }
            if let Err(e) = save_local_secrets(&self.game_settings.secrets_draft) {
                self.game_settings.save_status = Some(Err(format!("Failed to save local secrets: {e}")));
                return;
            }
            self.game_settings.secrets_base = self.game_settings.secrets_draft.clone();
        }

        // 3. Dispatch cloud mutations if any cloud field changed
        if !self.game_settings.cloud_dirty() {
            self.game_settings.save_status = Some(Ok("Local settings saved.".into()));
            return;
        }
        let Some(origin) = self.game.doc.cloud.clone() else {
            self.game_settings.save_status = Some(Ok("Local settings saved.".into()));
            return;
        };
        let (Some(snap), Some(draft)) = (self.game_settings.snapshot.clone(), self.game_settings.cloud_draft.clone())
        else {
            return;
        };

        let patch = self.game_settings.build_config_patch();
        let active_change = if draft.playability.is_active() != snap.config.is_active {
            Some(draft.playability.is_active())
        } else {
            None
        };
        let comm_audio = (draft.audio_enabled != snap.communication.audio_enabled).then_some(draft.audio_enabled);
        let comm_video = (draft.video_enabled != snap.communication.video_enabled).then_some(draft.video_enabled);
        let comm_chat = (draft.chat_group_api_enabled != snap.communication.chat_group_api_enabled)
            .then_some(draft.chat_group_api_enabled);
        let pending_icon = self
            .game_settings
            .pending_icon
            .as_ref()
            .map(|p| (p.filename.clone(), p.bytes.clone()));
        let pending_thumbs: Vec<(String, Vec<u8>)> = self
            .game_settings
            .pending_thumbs
            .iter()
            .map(|p| (p.filename.clone(), p.bytes.clone()))
            .collect();
        let deleted_thumbs = draft.deleted_thumbnails.clone();
        let orig_order: Vec<u64> = snap.thumbnails.iter().map(|t| t.id).collect();
        let new_order: Vec<u64> = draft.thumbnails.iter().map(|t| t.id).collect();
        let alt_changes: Vec<(u64, String)> = draft
            .thumbnails
            .iter()
            .filter_map(|t| {
                let old = snap.thumbnails.iter().find(|o| o.id == t.id)?;
                (old.alt_text != t.alt_text).then(|| (t.id, t.alt_text.clone()))
            })
            .collect();

        let universe_id = origin.universe_id;
        let root_place_id = origin.root_place_id;
        let api = self.cloud.factory(self.game.account.shared_client());
        let tx = self.game_settings.tx.clone();
        self.game_settings.saving = true;

        std::thread::spawn(move || {
            let Some(a) = api() else {
                let _ = tx.send(WorkerMsg::Saved {
                    universe_id,
                    result: Err(SaveFailure {
                        message: "Sign in to Roblox to save cloud experience settings.".into(),
                        name_moderated: false,
                        description_moderated: false,
                        vip_price_cooldown: None,
                    }),
                });
                return;
            };
            let fail = |message: String| SaveFailure {
                message,
                name_moderated: false,
                description_moderated: false,
                vip_price_cooldown: None,
            };

            if let Some(active) = active_change {
                if let Err(e) = a.set_universe_active(universe_id, active) {
                    let _ = tx.send(WorkerMsg::Saved { universe_id, result: Err(fail(e.0)) });
                    return;
                }
            }
            if !patch.is_empty() {
                if let Err(e) = a.patch_universe_configuration(universe_id, &patch) {
                    let _ = tx.send(WorkerMsg::Saved {
                        universe_id,
                        result: Err(SaveFailure {
                            message: e.message,
                            name_moderated: e.name_moderated,
                            description_moderated: e.description_moderated,
                            vip_price_cooldown: e.vip_price_cooldown,
                        }),
                    });
                    return;
                }
            }
            if comm_audio.is_some() || comm_video.is_some() || comm_chat.is_some() {
                if let Err(e) = a.set_communication_settings(universe_id, comm_audio, comm_video, comm_chat) {
                    let _ = tx.send(WorkerMsg::Saved { universe_id, result: Err(fail(e.0)) });
                    return;
                }
            }
            if let Some((fname, bytes)) = pending_icon {
                if let Err(e) = a.upload_universe_icon(universe_id, &fname, &bytes) {
                    let _ = tx.send(WorkerMsg::Saved { universe_id, result: Err(fail(e.0)) });
                    return;
                }
            }
            for tid in deleted_thumbs {
                if let Err(e) = a.delete_universe_thumbnail(universe_id, tid) {
                    let _ = tx.send(WorkerMsg::Saved { universe_id, result: Err(fail(e.0)) });
                    return;
                }
            }
            let mut final_order = new_order.clone();
            for (fname, bytes) in pending_thumbs {
                match a.upload_universe_thumbnail(universe_id, &fname, &bytes) {
                    Ok(new_id) if new_id != 0 => final_order.push(new_id),
                    Ok(_) => {}
                    Err(e) => {
                        let _ = tx.send(WorkerMsg::Saved { universe_id, result: Err(fail(e.0)) });
                        return;
                    }
                }
            }
            if final_order != orig_order && !final_order.is_empty() {
                if let Err(e) = a.set_thumbnail_order(universe_id, &final_order) {
                    let _ = tx.send(WorkerMsg::Saved { universe_id, result: Err(fail(e.0)) });
                    return;
                }
            }
            for (tid, alt) in alt_changes {
                if let Err(e) = a.set_thumbnail_alt_text(universe_id, tid, &alt) {
                    let _ = tx.send(WorkerMsg::Saved { universe_id, result: Err(fail(e.0)) });
                    return;
                }
            }
            let refreshed = a.fetch_game_settings(universe_id, root_place_id).ok();
            let _ = tx.send(WorkerMsg::Saved { universe_id, result: Ok(refreshed) });
        });
    }

    // ------------------------------------------------------------------ Window UI

    /// Studio's Experience Settings dialog (replaces the old read-only `place_info_window`).
    pub fn game_settings_window(&mut self, ctx: &egui::Context) {
        self.poll_game_settings(ctx);
        if !self.cloud.settings_open {
            return;
        }

        // First-frame lazy init if opened via CLI flag (`--roblox-picker settings`).
        if self.game_settings.local_base == LocalDraft::default()
            && self.game_settings.local_draft == LocalDraft::default()
        {
            let local = self.read_local_draft();
            self.game_settings.local_base = local.clone();
            self.game_settings.local_draft = local;
        }
        if let Some(o) = self.game.doc.cloud.clone() {
            if self.game_settings.loaded_universe != Some(o.universe_id) && !self.game_settings.loading {
                self.fetch_game_settings_async(o.universe_id, o.root_place_id);
            }
        }

        // Upload any freshly picked local image previews as textures.
        if let Some(p) = &self.game_settings.pending_icon {
            if self.game_settings.pending_icon_tex.is_none() {
                if let Some(img) = p.preview.clone() {
                    self.game_settings.pending_icon_tex =
                        Some(ctx.load_texture("gs_pending_icon", img, egui::TextureOptions::LINEAR));
                }
            }
        }
        for i in 0..self.game_settings.pending_thumbs.len() {
            if self.game_settings.pending_thumb_tex.get(i).is_some_and(Option::is_none) {
                if let Some(img) = self.game_settings.pending_thumbs[i].preview.clone() {
                    self.game_settings.pending_thumb_tex[i] = Some(ctx.load_texture(
                        format!("gs_pending_thumb_{i}"),
                        img,
                        egui::TextureOptions::LINEAR,
                    ));
                }
            }
        }

        let mut want_close = false;
        let mut trigger_save = false;
        let mut trigger_retry = false;
        let mut trigger_publish_save = false;
        let mut pick_icon = false;
        let mut pick_thumb = false;
        let mut save_dev_product: Option<DevProductDraft> = None;

        let screen = ctx.content_rect();
        let def_size = Vec2::new(940.0, 630.0).min(screen.size() - Vec2::splat(40.0));
        let def_pos = screen.center() - def_size * 0.5;

        let mut window_open = true;
        egui::Window::new("Experience Settings")
            .id(egui::Id::new("experience_settings_dialog"))
            .open(&mut window_open)
            .default_size(def_size)
            .default_pos(def_pos)
            .min_size([720.0, 460.0])
            .resizable(true)
            .collapsible(false)
            .frame(
                egui::Frame::window(&ctx.global_style())
                    .fill(theme::PANEL)
                    .inner_margin(egui::Margin::ZERO)
                    .stroke(Stroke::new(1.0, theme::DIVIDER)),
            )
            .show(ctx, |ui| {
                let full = ui.available_rect_before_wrap();
                let full = Rect::from_min_size(
                    full.min,
                    Vec2::new(full.width().max(720.0), full.height().max(430.0)),
                );
                ui.allocate_rect(full, Sense::hover());

                let body_h = (full.height() - FOOTER_HEIGHT - 1.0).max(200.0);
                let sidebar_rect = Rect::from_min_size(full.min, Vec2::new(SIDEBAR_WIDTH, body_h));
                let vsep_rect = Rect::from_min_size(
                    Pos2::new(sidebar_rect.max.x, full.min.y),
                    Vec2::new(1.0, body_h),
                );
                let content_outer = Rect::from_min_max(
                    Pos2::new(vsep_rect.max.x, full.min.y),
                    Pos2::new(full.max.x, full.min.y + body_h),
                );
                let hsep_rect = Rect::from_min_size(
                    Pos2::new(full.min.x, full.min.y + body_h),
                    Vec2::new(full.width(), 1.0),
                );
                let footer_rect = Rect::from_min_max(
                    Pos2::new(full.min.x, hsep_rect.max.y),
                    full.max,
                );

                // Backgrounds & crisp 1px Studio dividers
                let p = ui.painter();
                p.rect_filled(sidebar_rect, 0.0, theme::PANEL_DARK);
                p.rect_filled(content_outer, 0.0, theme::PANEL);
                p.rect_filled(footer_rect, 0.0, theme::PANEL_DARK);
                p.rect_filled(vsep_rect, 0.0, theme::DIVIDER);
                p.rect_filled(hsep_rect, 0.0, theme::DIVIDER);

                // ---- Left Sidebar (MenuBar) ----
                let sidebar_inner = Rect::from_min_max(
                    Pos2::new(sidebar_rect.min.x, sidebar_rect.min.y + 8.0),
                    Pos2::new(sidebar_rect.max.x, sidebar_rect.max.y - 8.0),
                );
                ui.scope_builder(
                    egui::UiBuilder::new()
                        .max_rect(sidebar_inner)
                        .layout(egui::Layout::top_down(egui::Align::LEFT)),
                    |ui| {
                        ui.set_clip_rect(sidebar_rect);
                        ui.spacing_mut().item_spacing = Vec2::ZERO;
                        self.draw_sidebar(ui);
                    },
                );

                // ---- Right Content Pane ----
                let content_inner = Rect::from_min_max(
                    Pos2::new(content_outer.min.x + 26.0, content_outer.min.y + 18.0),
                    Pos2::new(content_outer.max.x - 18.0, content_outer.max.y - 14.0),
                );
                ui.scope_builder(
                    egui::UiBuilder::new()
                        .max_rect(content_inner)
                        .layout(egui::Layout::top_down(egui::Align::LEFT)),
                    |ui| {
                        ui.set_clip_rect(content_outer);
                        egui::ScrollArea::vertical()
                            .id_salt(("gs_scroll", self.game_settings.page))
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                ui.set_width((content_inner.width() - 16.0).max(240.0));
                                self.draw_page_content(
                                    ui,
                                    &mut trigger_retry,
                                    &mut trigger_publish_save,
                                    &mut pick_icon,
                                    &mut pick_thumb,
                                    &mut save_dev_product,
                                );
                            });
                    },
                );

                // ---- Sticky Footer (52px) ----
                let footer_inner = footer_rect.shrink2(Vec2::new(18.0, 10.0));
                ui.scope_builder(
                    egui::UiBuilder::new()
                        .max_rect(footer_inner)
                        .layout(egui::Layout::left_to_right(egui::Align::Center)),
                    |ui| {
                        ui.set_clip_rect(footer_rect);
                        self.draw_footer(ui, &mut want_close, &mut trigger_save);
                    },
                );
            });

        if !window_open {
            want_close = true;
        }
        if want_close {
            if self.game_settings.is_dirty() && !self.game_settings.saving {
                self.game_settings.confirm_discard = true;
            } else {
                self.cloud.settings_open = false;
            }
        }
        if trigger_save && !self.game_settings.saving && !self.game_settings.has_any_error() {
            self.save_game_settings();
        }
        if trigger_retry {
            if let Some(o) = self.game.doc.cloud.clone() {
                self.fetch_game_settings_async(o.universe_id, o.root_place_id);
            }
        }
        if trigger_publish_save {
            self.open_roblox_destination(VersionType::Saved);
        }
        // (crate::file_dialog::Request: the desktop's rfd dialog, Android's
        // own picker — src/android/storage.rs)
        if pick_icon {
            let d = crate::file_dialog::Request::new().add_filter("Image (PNG/JPG)", &["png", "jpg", "jpeg"]);
            self.dialogs.start(Purpose::GameSettingsIcon, d, Kind::PickFile);
        }
        if pick_thumb {
            let d = crate::file_dialog::Request::new().add_filter("Image (PNG/JPG)", &["png", "jpg", "jpeg"]);
            self.dialogs.start(Purpose::GameSettingsThumbnail, d, Kind::PickFile);
        }
        if let Some(dp) = save_dev_product {
            self.submit_dev_product(dp);
        }

        // Studio confirmation modals (Discard Changes & Console Content Guidelines)
        self.draw_confirm_modals(ctx);
    }

    fn submit_dev_product(&mut self, dp: DevProductDraft) {
        let Some(o) = self.game.doc.cloud.clone() else { return };
        let Ok(price) = dp.price_text.trim().parse::<u64>() else { return };
        if let Some(cur) = self.game_settings.dev_product_draft.as_mut() {
            cur.busy = true;
            cur.error = None;
        }
        let universe_id = o.universe_id;
        let api = self.cloud.factory(self.game.account.shared_client());
        let tx = self.game_settings.tx.clone();
        std::thread::spawn(move || {
            let Some(a) = api() else {
                let _ = tx.send(WorkerMsg::DevProductSaved {
                    universe_id,
                    result: Err("Sign in to manage developer products.".into()),
                });
                return;
            };
            let res = match dp.product_id {
                Some(pid) => a
                    .update_developer_product(universe_id, pid, dp.name.trim(), price, dp.description.trim())
                    .map(|_| ()),
                None => a
                    .create_developer_product(universe_id, dp.name.trim(), price, dp.description.trim())
                    .map(|_| ()),
            };
            let out = match res {
                Ok(()) => Ok(a.fetch_monetization_summary(universe_id).dev_products),
                Err(e) => Err(e.0),
            };
            let _ = tx.send(WorkerMsg::DevProductSaved { universe_id, result: out });
        });
    }

    fn draw_sidebar(&mut self, ui: &mut egui::Ui) {
        // Compact header badge showing the experience / local place name
        let (title, badge, is_cloud) = match &self.game.doc.cloud {
            Some(o) => {
                let name = if !o.experience.is_empty() {
                    o.experience.clone()
                } else if !o.name.is_empty() {
                    o.name.clone()
                } else {
                    format!("Universe {}", o.universe_id)
                };
                (name, format!("ID {}", o.universe_id), true)
            }
            None => {
                let name = self
                    .game
                    .doc
                    .path
                    .as_ref()
                    .and_then(|p| p.file_stem())
                    .and_then(|s| s.to_str())
                    .unwrap_or("Untitled Place")
                    .to_string();
                (name, "Local Place".to_string(), false)
            }
        };

        ui.add_space(4.0);
        egui::Frame::NONE
            .inner_margin(egui::Margin { left: 14, right: 12, top: 4, bottom: 10 })
            .show(ui, |ui| {
                ui.add(
                    egui::Label::new(RichText::new(title).strong().size(13.0).color(theme::TEXT)).truncate(),
                );
                ui.add_space(3.0);
                let pill_bg = if is_cloud {
                    Color32::from_rgba_unmultiplied(0, 162, 255, 32)
                } else {
                    Color32::from_rgba_unmultiplied(240, 200, 90, 28)
                };
                let pill_fg = if is_cloud { STUDIO_BLUE } else { theme::WARN };
                egui::Frame::NONE
                    .fill(pill_bg)
                    .corner_radius(CornerRadius::same(3))
                    .inner_margin(egui::Margin::symmetric(6, 2))
                    .show(ui, |ui| {
                        ui.label(RichText::new(badge).size(10.5).color(pill_fg));
                    });
            });

        let (div_r, _) = ui.allocate_exact_size(Vec2::new(SIDEBAR_WIDTH, 1.0), Sense::hover());
        ui.painter().rect_filled(div_r, 0.0, theme::DIVIDER);
        ui.add_space(6.0);

        // Studio MenuEntry list
        for page in SettingsPage::ALL {
            let selected = self.game_settings.page == page;
            let has_err = self.game_settings.page_error(page).is_some();
            let dirty = self.game_settings.page_dirty(page);

            let (r, resp) = ui.allocate_exact_size(Vec2::new(SIDEBAR_WIDTH, ENTRY_HEIGHT), Sense::click());
            let p = ui.painter();

            if selected {
                p.rect_filled(r, 0.0, theme::ACTIVE_TAB);
                let bar = Rect::from_min_size(r.min, Vec2::new(3.0, r.height()));
                p.rect_filled(bar, 0.0, STUDIO_BLUE);
            } else if resp.hovered() {
                p.rect_filled(r, 0.0, theme::HOVER_BG);
            }

            let icon_col = if selected {
                STUDIO_BLUE
            } else if resp.hovered() {
                theme::TEXT
            } else {
                theme::TEXT_DIM
            };
            let icon_r = Rect::from_center_size(Pos2::new(r.min.x + 22.0, r.center().y), Vec2::splat(16.0));
            rustudio_engine::ti::paint(p, icon_r, page.icon(), icon_col);

            let text_col = if selected { Color32::WHITE } else { theme::TEXT };
            p.text(
                Pos2::new(r.min.x + 38.0, r.center().y),
                egui::Align2::LEFT_CENTER,
                page.label(),
                FontId::proportional(13.0),
                text_col,
            );

            if has_err {
                p.circle_filled(Pos2::new(r.max.x - 14.0, r.center().y), 4.0, theme::ERROR);
            } else if dirty {
                p.circle_filled(Pos2::new(r.max.x - 14.0, r.center().y), 3.5, STUDIO_BLUE);
            }

            if resp.clicked() {
                self.game_settings.page = page;
                self.game_settings.secret_edit = None;
            }
        }
    }

    fn draw_footer(&mut self, ui: &mut egui::Ui, want_close: &mut bool, trigger_save: &mut bool) {
        ui.horizontal_centered(|ui| {
            // Left: Studio's "Manage on Creator Hub" link + dirty/save status
            if let Some(o) = &self.game.doc.cloud {
                let link = ui
                    .add(
                        egui::Button::new(
                            RichText::new("Manage on Creator Hub ↗").size(12.0).color(STUDIO_BLUE),
                        )
                        .frame(false),
                    )
                    .on_hover_text(creator_hub::configure(o.universe_id));
                if link.clicked() {
                    ui.ctx().open_url(egui::OpenUrl::new_tab(creator_hub::configure(o.universe_id)));
                }
                ui.add_space(10.0);
            }

            if self.game_settings.saving {
                ui.spinner();
                ui.label(RichText::new("Saving changes…").size(12.0).color(theme::TEXT_DIM));
            } else if let Some(status) = &self.game_settings.save_status {
                match status {
                    Ok(msg) => {
                        ui.label(RichText::new(msg).size(12.0).color(STUDIO_GREEN));
                    }
                    Err(err) => {
                        ui.add(
                            egui::Label::new(RichText::new(err).size(12.0).color(theme::ERROR)).truncate(),
                        );
                    }
                }
            } else if self.game_settings.is_dirty() {
                ui.label(RichText::new("● Unsaved changes").size(12.0).color(theme::WARN));
            }

            // Right: Cancel + Primary Save buttons (Studio ButtonBar)
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let can_save = self.game_settings.is_dirty()
                    && !self.game_settings.saving
                    && !self.game_settings.has_any_error();

                let save_fill = if can_save {
                    STUDIO_BLUE
                } else {
                    Color32::from_rgb(48, 66, 86)
                };
                let save_text = if can_save { Color32::WHITE } else { theme::TEXT_DIM };
                let save_btn = egui::Button::new(RichText::new("Save").strong().size(13.0).color(save_text))
                    .fill(save_fill)
                    .corner_radius(CornerRadius::same(4))
                    .min_size(Vec2::new(84.0, 30.0));
                if ui.add_enabled(can_save, save_btn).clicked() {
                    *trigger_save = true;
                }

                ui.add_space(8.0);
                let cancel_btn = egui::Button::new(RichText::new("Cancel").size(13.0).color(theme::TEXT))
                    .fill(theme::PANEL_ALT)
                    .stroke(Stroke::new(1.0, theme::DIVIDER))
                    .corner_radius(CornerRadius::same(4))
                    .min_size(Vec2::new(84.0, 30.0));
                if ui.add_enabled(!self.game_settings.saving, cancel_btn).clicked() {
                    *want_close = true;
                }
            });
        });
    }

    fn draw_page_content(
        &mut self,
        ui: &mut egui::Ui,
        trigger_retry: &mut bool,
        trigger_publish_save: &mut bool,
        pick_icon: &mut bool,
        pick_thumb: &mut bool,
        save_dev_product: &mut Option<DevProductDraft>,
    ) {
        let page = self.game_settings.page;

        // If inside the Edit Secret sub-view on Security, draw that full sub-view
        if page == SettingsPage::Security && self.game_settings.secret_edit.is_some() {
            self.draw_edit_secret_subview(ui);
            return;
        }

        // Page header
        ui.horizontal(|ui| {
            ui.label(RichText::new(page.label()).size(20.0).strong().color(Color32::WHITE));
            if self.game.doc.cloud.is_some()
                && !matches!(page, SettingsPage::World | SettingsPage::AdditionalSettings)
            {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let r = ui
                        .add_enabled(
                            !self.game_settings.loading && !self.game_settings.saving,
                            egui::Button::new(RichText::new("Refresh").size(11.5))
                                .fill(theme::PANEL_ALT)
                                .corner_radius(CornerRadius::same(3)),
                        )
                        .on_hover_text("Reload settings from Roblox");
                    if r.clicked() {
                        *trigger_retry = true;
                    }
                });
            }
        });
        ui.add_space(6.0);
        ui.separator();
        ui.add_space(12.0);

        match page {
            SettingsPage::BasicInfo => {
                self.draw_basic_info_page(ui, trigger_retry, trigger_publish_save, pick_icon, pick_thumb);
            }
            SettingsPage::Permissions => {
                self.draw_permissions_page(ui, trigger_retry, trigger_publish_save);
            }
            SettingsPage::Communication => {
                self.draw_communication_page(ui, trigger_retry, trigger_publish_save);
            }
            SettingsPage::Security => {
                self.draw_security_page(ui, trigger_retry, trigger_publish_save);
            }
            SettingsPage::Monetization => {
                self.draw_monetization_page(ui, trigger_retry, trigger_publish_save, save_dev_product);
            }
            SettingsPage::World => {
                self.draw_world_page(ui);
            }
            SettingsPage::AdditionalSettings => {
                self.draw_creator_hub_page(ui, trigger_publish_save);
            }
        }
    }

    // ------------------------------------------------------------------ Shared UI helpers

    fn draw_unpublished_banner(&self, ui: &mut egui::Ui, trigger_publish_save: &mut bool, local_note: Option<&str>) {
        egui::Frame::NONE
            .fill(theme::PANEL_DARK)
            .stroke(Stroke::new(1.0, Color32::from_rgb(56, 86, 128)))
            .corner_radius(CornerRadius::same(6))
            .inner_margin(egui::Margin::same(16))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(
                    RichText::new("This place must be saved to Roblox to configure cloud experience settings.")
                        .strong()
                        .size(13.5)
                        .color(Color32::WHITE),
                );
                if let Some(note) = local_note {
                    ui.add_space(4.0);
                    ui.label(RichText::new(note).size(12.0).color(theme::TEXT_DIM));
                }
                ui.add_space(10.0);
                let btn = egui::Button::new(RichText::new("  Save to Roblox  ").strong().size(12.5).color(Color32::WHITE))
                    .fill(STUDIO_BLUE)
                    .corner_radius(CornerRadius::same(4))
                    .min_size(Vec2::new(0.0, 28.0));
                if ui.add(btn).clicked() {
                    *trigger_publish_save = true;
                }
            });
    }

    fn draw_cloud_gate(&self, ui: &mut egui::Ui, trigger_retry: &mut bool, trigger_publish_save: &mut bool) -> bool {
        if self.game.doc.cloud.is_none() {
            self.draw_unpublished_banner(
                ui,
                trigger_publish_save,
                Some("Local place settings (Security local toggles, Local Secrets, and World physics) are still editable offline."),
            );
            return false;
        }
        if self.game_settings.loading && self.game_settings.cloud_draft.is_none() {
            ui.add_space(28.0);
            ui.horizontal(|ui| {
                ui.spinner();
                ui.add_space(6.0);
                ui.label(RichText::new("Loading experience settings from Roblox…").color(theme::TEXT_DIM));
            });
            return false;
        }
        if let Some(err) = &self.game_settings.load_error {
            if self.game_settings.cloud_draft.is_none() {
                egui::Frame::NONE
                    .fill(Color32::from_rgba_unmultiplied(240, 96, 86, 24))
                    .stroke(Stroke::new(1.0, theme::ERROR))
                    .corner_radius(CornerRadius::same(5))
                    .inner_margin(egui::Margin::same(14))
                    .show(ui, |ui| {
                        ui.label(RichText::new("Failed to load experience settings").strong().color(theme::ERROR));
                        ui.add_space(4.0);
                        ui.label(RichText::new(err).size(12.0).color(theme::TEXT));
                        ui.add_space(8.0);
                        if ui.button("Retry").clicked() {
                            *trigger_retry = true;
                        }
                    });
                return false;
            }
        }
        if let Some(snap) = &self.game_settings.snapshot {
            if !snap.can_manage_config {
                Self::draw_warning_banner(
                    ui,
                    "View-only: this account does not have permission to manage cloud settings for this experience (only the owner or group managers can save cloud changes).",
                );
                ui.add_space(10.0);
            }
        }
        self.game_settings.cloud_draft.is_some()
    }

    fn draw_warning_banner(ui: &mut egui::Ui, text: &str) {
        egui::Frame::NONE
            .fill(Color32::from_rgba_unmultiplied(240, 200, 90, 22))
            .stroke(Stroke::new(1.0, Color32::from_rgb(176, 138, 42)))
            .corner_radius(CornerRadius::same(5))
            .inner_margin(egui::Margin::symmetric(12, 9))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    let (ir, _) = ui.allocate_exact_size(Vec2::splat(16.0), Sense::hover());
                    rustudio_engine::ti::paint(ui.painter(), ir, "alert-triangle", theme::WARN);
                    ui.add_space(4.0);
                    ui.label(RichText::new(text).size(12.0).color(theme::WARN));
                });
            });
    }

    // ------------------------------------------------------------------ Page 1: Basic Info

    fn draw_basic_info_page(
        &mut self,
        ui: &mut egui::Ui,
        trigger_retry: &mut bool,
        trigger_publish_save: &mut bool,
        pick_icon: &mut bool,
        pick_thumb: &mut bool,
    ) {
        if !self.draw_cloud_gate(ui, trigger_retry, trigger_publish_save) {
            return;
        }
        let origin = self.game.doc.cloud.clone().unwrap();
        let snap = self.game_settings.snapshot.clone().unwrap();
        let mut ask_console_confirm = false;
        let mut clear_pending_icon = false;
        let mut remove_pending_thumb: Option<usize> = None;
        let mut open_url: Option<String> = None;

        let name_mod = self.game_settings.name_moderated_err;
        let desc_mod = self.game_settings.desc_moderated_err;
        let has_pending_icon = self.game_settings.pending_icon.is_some();
        let pending_icon_name = self
            .game_settings
            .pending_icon
            .as_ref()
            .map(|p| p.filename.clone());
        let pending_icon_tex = self.game_settings.pending_icon_tex.clone();
        let remote_icon_tex = snap
            .icon
            .image_id
            .and_then(|id| self.game_settings.remote_textures.get(&id).cloned());
        let remote_textures = self.game_settings.remote_textures.clone();
        let pending_thumb_names: Vec<String> = self
            .game_settings
            .pending_thumbs
            .iter()
            .map(|p| p.filename.clone())
            .collect();
        let pending_thumb_tex = self.game_settings.pending_thumb_tex.clone();

        if let Some(d) = self.game_settings.cloud_draft.as_mut() {
            // Name
            setting_row(ui, "Name", |ui| {
                let len = d.name.chars().count();
                let resp = ui.add(
                    egui::TextEdit::singleline(&mut d.name)
                        .desired_width(420.0)
                        .hint_text("Experience name"),
                );
                if resp.changed() {
                    self.game_settings.name_moderated_err = false;
                }
                ui.horizontal(|ui| {
                    let count_col = if len == 0 || len > NAME_LIMIT { theme::ERROR } else { theme::TEXT_DIM };
                    ui.label(RichText::new(format!("{len}/{NAME_LIMIT}")).size(11.0).color(count_col));
                    if len == 0 {
                        ui.label(RichText::new("Name cannot be empty.").size(11.0).color(theme::ERROR));
                    } else if len > NAME_LIMIT {
                        ui.label(RichText::new("Name is too long.").size(11.0).color(theme::ERROR));
                    } else if name_mod {
                        ui.label(RichText::new("Name was moderated by Roblox.").size(11.0).color(theme::ERROR));
                    }
                });
            });

            ui.add_space(12.0);

            // Description
            setting_row(ui, "Description", |ui| {
                let len = d.description.chars().count();
                let resp = ui.add(
                    egui::TextEdit::multiline(&mut d.description)
                        .desired_width(420.0)
                        .desired_rows(4)
                        .hint_text("Describe your experience…"),
                );
                if resp.changed() {
                    self.game_settings.desc_moderated_err = false;
                }
                ui.horizontal(|ui| {
                    let count_col = if len > DESC_LIMIT { theme::ERROR } else { theme::TEXT_DIM };
                    ui.label(RichText::new(format!("{len}/{DESC_LIMIT}")).size(11.0).color(count_col));
                    if len > DESC_LIMIT {
                        ui.label(RichText::new("Description is too long.").size(11.0).color(theme::ERROR));
                    } else if desc_mod {
                        ui.label(
                            RichText::new("Description was moderated by Roblox.")
                                .size(11.0)
                                .color(theme::ERROR),
                        );
                    }
                });
            });

            ui.add_space(14.0);

            // Experience Icon
            setting_row(ui, "Game Icon", |ui| {
                ui.horizontal(|ui| {
                    let tex = pending_icon_tex.as_ref().or(remote_icon_tex.as_ref());
                    egui::Frame::NONE
                        .fill(theme::PANEL_DARK)
                        .stroke(Stroke::new(1.0, theme::DIVIDER))
                        .corner_radius(CornerRadius::same(6))
                        .show(ui, |ui| {
                            if let Some(t) = tex {
                                ui.image(egui::load::SizedTexture::new(t.id(), Vec2::splat(96.0)));
                            } else {
                                let (r, _) = ui.allocate_exact_size(Vec2::splat(96.0), Sense::hover());
                                rustudio_engine::ti::paint(ui.painter(), r.shrink(30.0), "photo", theme::TEXT_DIM);
                            }
                        });
                    ui.add_space(10.0);
                    ui.vertical(|ui| {
                        ui.label(
                            RichText::new("Square 512×512 PNG or JPEG recommended.")
                                .size(11.5)
                                .color(theme::TEXT_DIM),
                        );
                        if let Some(fname) = &pending_icon_name {
                            ui.label(
                                RichText::new(format!("Pending upload: {fname}"))
                                    .size(11.5)
                                    .color(STUDIO_BLUE),
                            );
                        } else if let Some(img_id) = snap.icon.image_id {
                            let st = if snap.icon.is_approved {
                                ("Approved", STUDIO_GREEN)
                            } else {
                                ("Pending review", theme::WARN)
                            };
                            ui.label(RichText::new(format!("Asset {img_id} · {}", st.0)).size(11.5).color(st.1));
                        } else {
                            ui.label(RichText::new("Using default placeholder icon.").size(11.5).color(theme::TEXT_DIM));
                        }
                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            if ui
                                .add(
                                    egui::Button::new("Upload Image…")
                                        .fill(theme::PANEL_ALT)
                                        .corner_radius(CornerRadius::same(4)),
                                )
                                .clicked()
                            {
                                *pick_icon = true;
                            }
                            if has_pending_icon && ui.button("Revert").clicked() {
                                clear_pending_icon = true;
                            }
                        });
                    });
                });
            });

            ui.add_space(14.0);

            // Screenshots & Videos
            let total_thumbs = d.thumbnails.len() + pending_thumb_names.len();
            setting_row(ui, &format!("Screenshots ({total_thumbs}/{MAX_THUMBNAILS})"), |ui| {
                ui.label(
                    RichText::new("1920×1080 PNG or JPEG (16:9). Reorder, edit alt text, or remove existing thumbnails.")
                        .size(11.5)
                        .color(theme::TEXT_DIM),
                );
                ui.add_space(6.0);

                let mut move_swap: Option<(usize, usize)> = None;
                let mut delete_idx: Option<usize> = None;
                let n = d.thumbnails.len();
                for (i, t) in d.thumbnails.iter_mut().enumerate() {
                    egui::Frame::NONE
                        .fill(theme::PANEL_DARK)
                        .stroke(Stroke::new(1.0, theme::DIVIDER))
                        .corner_radius(CornerRadius::same(5))
                        .inner_margin(egui::Margin::same(8))
                        .show(ui, |ui| {
                            ui.set_width(420.0);
                            ui.horizontal(|ui| {
                                let preview_size = Vec2::new(112.0, 63.0);
                                if let Some(tex) = t.image_id.and_then(|id| remote_textures.get(&id)) {
                                    ui.image(egui::load::SizedTexture::new(tex.id(), preview_size));
                                } else {
                                    let (r, _) = ui.allocate_exact_size(preview_size, Sense::hover());
                                    ui.painter().rect_filled(r, 3.0, theme::PANEL);
                                    rustudio_engine::ti::paint(ui.painter(), r.shrink(18.0), "photo", theme::TEXT_DIM);
                                }
                                ui.add_space(6.0);
                                ui.vertical(|ui| {
                                    ui.horizontal(|ui| {
                                        let kind = if t.asset_type.is_empty() { "Image" } else { &t.asset_type };
                                        ui.label(RichText::new(format!("#{} · {kind} ({})", i + 1, t.id)).size(11.5).strong());
                                        let badge = if t.approved { ("Approved", STUDIO_GREEN) } else { ("Pending", theme::WARN) };
                                        ui.label(RichText::new(badge.0).size(10.5).color(badge.1));
                                    });
                                    ui.add(
                                        egui::TextEdit::singleline(&mut t.alt_text)
                                            .desired_width(275.0)
                                            .hint_text("Alt text (accessibility description)"),
                                    );
                                    ui.horizontal(|ui| {
                                        if ui.add_enabled(i > 0, egui::Button::new("↑ Up").small()).clicked() {
                                            move_swap = Some((i, i - 1));
                                        }
                                        if ui.add_enabled(i + 1 < n, egui::Button::new("↓ Down").small()).clicked() {
                                            move_swap = Some((i, i + 1));
                                        }
                                        if ui
                                            .add(egui::Button::new(RichText::new("Remove").color(theme::ERROR)).small())
                                            .clicked()
                                        {
                                            delete_idx = Some(i);
                                        }
                                    });
                                });
                            });
                        });
                    ui.add_space(4.0);
                }
                if let Some((a, b)) = move_swap {
                    d.thumbnails.swap(a, b);
                }
                if let Some(i) = delete_idx {
                    let removed = d.thumbnails.remove(i);
                    d.deleted_thumbnails.push(removed.id);
                }

                for (pi, fname) in pending_thumb_names.iter().enumerate() {
                    egui::Frame::NONE
                        .fill(Color32::from_rgba_unmultiplied(0, 162, 255, 18))
                        .stroke(Stroke::new(1.0, STUDIO_BLUE))
                        .corner_radius(CornerRadius::same(5))
                        .inner_margin(egui::Margin::same(8))
                        .show(ui, |ui| {
                            ui.set_width(420.0);
                            ui.horizontal(|ui| {
                                if let Some(Some(tex)) = pending_thumb_tex.get(pi) {
                                    ui.image(egui::load::SizedTexture::new(tex.id(), Vec2::new(80.0, 45.0)));
                                }
                                ui.label(RichText::new(format!("New upload: {fname}")).size(12.0).color(STUDIO_BLUE));
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    if ui.button("Cancel").clicked() {
                                        remove_pending_thumb = Some(pi);
                                    }
                                });
                            });
                        });
                    ui.add_space(4.0);
                }

                if ui
                    .add_enabled(
                        total_thumbs < MAX_THUMBNAILS,
                        egui::Button::new("+ Add Screenshot…")
                            .fill(theme::PANEL_ALT)
                            .corner_radius(CornerRadius::same(4)),
                    )
                    .clicked()
                {
                    *pick_thumb = true;
                }
            });

            ui.add_space(14.0);

            // Genre & Content Maturity Guidelines (Studio 0.748 `BasicInfo.lua`)
            setting_row(ui, "Genre & Maturity", |ui| {
                egui::Frame::NONE
                    .fill(theme::PANEL_DARK)
                    .stroke(Stroke::new(1.0, theme::DIVIDER))
                    .corner_radius(CornerRadius::same(5))
                    .inner_margin(egui::Margin::same(12))
                    .show(ui, |ui| {
                        ui.set_width(420.0);
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("Experience Genre:").size(12.0).color(theme::TEXT_DIM));
                            let raw_genre = snap
                                .experience_genre
                                .as_deref()
                                .filter(|s| !matches!(s.trim().to_ascii_lowercase().as_str(), "" | "na" | "n/a" | "none" | "unknown"))
                                .map(format_genre_slug);
                            match raw_genre.filter(|s| !s.is_empty()) {
                                Some(label) => ui.label(RichText::new(label).size(12.0).strong()),
                                None => ui.label(RichText::new("Not set").size(12.0).color(theme::TEXT_DIM)),
                            };
                        });
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("Legacy Genre:").size(12.0).color(theme::TEXT_DIM));
                            egui::ComboBox::from_id_salt("gs_legacy_genre")
                                .selected_text(&d.genre)
                                .width(160.0)
                                .show_ui(ui, |ui| {
                                    for &g in LEGACY_GENRES.iter() {
                                        ui.selectable_value(&mut d.genre, g.to_string(), g);
                                    }
                                });
                        });
                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("Content Maturity:").size(12.0).color(theme::TEXT_DIM));
                            let maturity = snap
                                .guidelines
                                .as_ref()
                                .map(|g| {
                                    if !g.display_name.is_empty() {
                                        g.display_name.clone()
                                    } else {
                                        g.content_maturity.clone()
                                    }
                                })
                                .filter(|s| !matches!(s.trim().to_ascii_lowercase().as_str(), "" | "unknown" | "unrated" | "na"))
                                .unwrap_or_else(|| "Unrated".into());
                            let col = if maturity.eq_ignore_ascii_case("unrated") {
                                theme::WARN
                            } else {
                                STUDIO_GREEN
                            };
                            ui.label(RichText::new(maturity).size(12.0).strong().color(col));
                        });
                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            if ui.button("Manage Genre ↗").clicked() {
                                open_url = Some(creator_hub::configure(origin.universe_id));
                            }
                            if ui.button("Maturity Questionnaire ↗").clicked() {
                                open_url = Some(creator_hub::questionnaire(origin.universe_id));
                            }
                        });
                    });
            });

            ui.add_space(14.0);

            // Playable Devices
            setting_row(ui, "Playable Devices", |ui| {
                ui.label(
                    RichText::new("Select the devices that players can use to join this experience.")
                        .size(11.5)
                        .color(theme::TEXT_DIM),
                );
                ui.add_space(4.0);
                for &dev in PLAYABLE_DEVICES.iter() {
                    let mut checked = d.playable_devices.iter().any(|x| x == dev);
                    let was = checked;
                    let icon = match dev {
                        "Computer" => "device-desktop",
                        "Phone" => "device-mobile",
                        "Tablet" => "device-tablet",
                        "Console" => "device-gamepad",
                        _ => "headset",
                    };
                    ui.horizontal(|ui| {
                        ui.checkbox(&mut checked, "");
                        let (ir, _) = ui.allocate_exact_size(Vec2::splat(15.0), Sense::hover());
                        rustudio_engine::ti::paint(ui.painter(), ir, icon, theme::TEXT_DIM);
                        ui.label(dev);
                    });
                    if checked != was {
                        if checked {
                            if dev == "Console" {
                                // Studio's Console Content Guidelines confirmation dialog
                                ask_console_confirm = true;
                            } else if !d.playable_devices.iter().any(|x| x == dev) {
                                d.playable_devices.push(dev.to_string());
                            }
                        } else {
                            d.playable_devices.retain(|x| x != dev);
                        }
                    }
                }
                if d.playable_devices.is_empty() {
                    ui.label(
                        RichText::new("At least one playable device must be selected.")
                            .size(11.5)
                            .color(theme::ERROR),
                    );
                }
            });

            ui.add_space(16.0);
            ui.separator();
            ui.add_space(12.0);

            // Place & Team Create Details
            setting_row(ui, "Place & Collaboration", |ui| {
                egui::Frame::NONE
                    .fill(theme::PANEL_DARK)
                    .stroke(Stroke::new(1.0, theme::DIVIDER))
                    .corner_radius(CornerRadius::same(5))
                    .inner_margin(egui::Margin::same(12))
                    .show(ui, |ui| {
                        ui.set_width(420.0);
                        egui::Grid::new("gs_place_details").num_columns(2).spacing([16.0, 5.0]).show(ui, |ui| {
                            let mut kv = |k: &str, v: String| {
                                ui.label(RichText::new(k).size(12.0).color(theme::TEXT_DIM));
                                ui.label(RichText::new(v).size(12.0));
                                ui.end_row();
                            };
                            kv("Place", format!("{} ({})", origin.name, origin.place_id));
                            kv("Universe ID", origin.universe_id.to_string());
                            kv(
                                "Creator",
                                format!("{} ({} {})", origin.creator_label(), origin.creator_type.as_str(), origin.creator_id),
                            );
                            kv(
                                "Start Place",
                                if origin.root_place_id == origin.place_id {
                                    "This place".into()
                                } else {
                                    origin.root_place_id.to_string()
                                },
                            );
                            kv(
                                "Version",
                                format!(
                                    "{}{}",
                                    origin.version,
                                    if self.game.doc.dirty() { " (unsaved local edits)" } else { "" }
                                ),
                            );
                            kv("Published", if snap.has_published_version { "Yes".into() } else { "Not yet".into() });
                            kv(
                                "Team Create",
                                match snap.team_create.enabled {
                                    Some(true) => "Enabled".into(),
                                    Some(false) => "Disabled".into(),
                                    None => "Unknown".into(),
                                },
                            );
                        });
                        ui.add_space(6.0);
                        ui.label(
                            RichText::new(snap.team_create.describe())
                                .size(11.5)
                                .color(theme::TEXT_DIM),
                        );
                    });
            });
        }

        if clear_pending_icon {
            self.game_settings.pending_icon = None;
            self.game_settings.pending_icon_tex = None;
        }
        if let Some(i) = remove_pending_thumb {
            self.game_settings.pending_thumbs.remove(i);
            self.game_settings.pending_thumb_tex.remove(i);
        }
        if ask_console_confirm {
            self.game_settings.confirm_console = true;
        }
        if let Some(url) = open_url {
            ui.ctx().open_url(egui::OpenUrl::new_tab(url));
        }
    }

    // ------------------------------------------------------------------ Page 2: Permissions

    fn draw_permissions_page(
        &mut self,
        ui: &mut egui::Ui,
        trigger_retry: &mut bool,
        trigger_publish_save: &mut bool,
    ) {
        if !self.draw_cloud_gate(ui, trigger_retry, trigger_publish_save) {
            return;
        }
        let origin = self.game.doc.cloud.clone().unwrap();
        let snap = self.game_settings.snapshot.clone().unwrap();
        let mut open_url: Option<String> = None;

        if let Some(d) = self.game_settings.cloud_draft.as_mut() {
            setting_row(ui, "Playability", |ui| {
                let choices = [
                    (Playability::Public, "Public", "Anyone on Roblox can discover and play this experience."),
                    (Playability::Friends, "Friends", "Only your connections can join and play this experience."),
                    (Playability::Private, "Private", "Only you and team members with edit access can play."),
                ];
                for (val, title, desc) in choices {
                    let active = d.playability == val;
                    let stroke = if active { STUDIO_BLUE } else { theme::DIVIDER };
                    let fill = if active {
                        Color32::from_rgba_unmultiplied(0, 162, 255, 20)
                    } else {
                        theme::PANEL_DARK
                    };
                    let mut radio_clicked = false;
                    let resp = egui::Frame::NONE
                        .fill(fill)
                        .stroke(Stroke::new(1.0, stroke))
                        .corner_radius(CornerRadius::same(5))
                        .inner_margin(egui::Margin::symmetric(12, 10))
                        .show(ui, |ui| {
                            ui.set_width(420.0);
                            ui.horizontal(|ui| {
                                if ui.radio(active, "").clicked() {
                                    radio_clicked = true;
                                }
                                ui.vertical(|ui| {
                                    ui.label(RichText::new(title).strong().size(13.0));
                                    ui.label(RichText::new(desc).size(11.5).color(theme::TEXT_DIM));
                                });
                            });
                        })
                        .response
                        .interact(Sense::click());
                    if resp.clicked() || radio_clicked {
                        d.playability = val;
                    }
                    ui.add_space(6.0);
                }

                if snap.config.is_active && !d.playability.is_active() {
                    ui.add_space(4.0);
                    Self::draw_warning_banner(
                        ui,
                        "Switching to Private will deactivate this experience and shut down any active servers.",
                    );
                }
            });

            ui.add_space(16.0);
            ui.separator();
            ui.add_space(12.0);

            setting_row(ui, "Collaborators", |ui| {
                egui::Frame::NONE
                    .fill(theme::PANEL_DARK)
                    .stroke(Stroke::new(1.0, theme::DIVIDER))
                    .corner_radius(CornerRadius::same(5))
                    .inner_margin(egui::Margin::same(12))
                    .show(ui, |ui| {
                        ui.set_width(420.0);
                        ui.label(
                            RichText::new(format!("Owner: {}", origin.creator_label()))
                                .size(12.5)
                                .strong(),
                        );
                        ui.add_space(4.0);
                        let yn = |b: Option<bool>| match b {
                            Some(true) => "Yes",
                            Some(false) => "No",
                            None => "Unknown",
                        };
                        ui.label(
                            RichText::new(format!(
                                "Can manage: {}  ·  Can edit in cloud: {}  ·  Can collaborate: {}",
                                yn(snap.team_create.can_manage),
                                yn(snap.team_create.can_cloud_edit),
                                yn(snap.team_create.can_collaborate),
                            ))
                            .size(11.5)
                            .color(theme::TEXT_DIM),
                        );
                        ui.add_space(8.0);
                        if ui.button("Manage Collaborators on Creator Hub ↗").clicked() {
                            open_url = Some(creator_hub::collaborators(origin.universe_id));
                        }
                    });
            });
        }

        if let Some(url) = open_url {
            ui.ctx().open_url(egui::OpenUrl::new_tab(url));
        }
    }

    // ------------------------------------------------------------------ Page 3: Communication

    fn draw_communication_page(
        &mut self,
        ui: &mut egui::Ui,
        trigger_retry: &mut bool,
        trigger_publish_save: &mut bool,
    ) {
        if !self.draw_cloud_gate(ui, trigger_retry, trigger_publish_save) {
            return;
        }
        let snap = self.game_settings.snapshot.clone().unwrap();
        let mut open_url: Option<&'static str> = None;

        if !snap.communication.user_avatar_chat_enabled {
            Self::draw_warning_banner(
                ui,
                "Your Roblox account does not currently have Avatar Chat / Voice enabled (`voice/v1/settings`).",
            );
            ui.add_space(12.0);
        }

        if let Some(d) = self.game_settings.cloud_draft.as_mut() {
            setting_row(ui, "Enable Microphone", |ui| {
                toggle_row(
                    ui,
                    &mut d.audio_enabled,
                    "Allow eligible players to communicate with spatial voice inside this experience.",
                );
                if link_button(ui, "Learn more about Voice Chat ↗") {
                    open_url = Some("https://developer.roblox.com/articles/chat-with-avatars-audio");
                }
            });

            ui.add_space(14.0);

            setting_row(ui, "Enable Camera", |ui| {
                toggle_row(
                    ui,
                    &mut d.video_enabled,
                    "Allow eligible players to animate their avatar's face using their device camera.",
                );
                if link_button(ui, "Learn more about Avatar Video ↗") {
                    open_url = Some("https://developer.roblox.com/articles/chat-with-avatars-video");
                }
            });

            ui.add_space(14.0);

            setting_row(ui, "Enable Chat Group API", |ui| {
                toggle_row(
                    ui,
                    &mut d.chat_group_api_enabled,
                    "Allow scripts to manage Voice & Text chat groups through Roblox's Chat Group API.",
                );
            });

            ui.add_space(16.0);
            ui.separator();
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new("Communication features must comply with:").size(11.5).color(theme::TEXT_DIM));
                if link_button(ui, "Terms of Use ↗") {
                    open_url = Some("https://help.roblox.com/hc/articles/115004647846-Roblox-Terms-of-Use");
                }
                ui.label(RichText::new("·").color(theme::TEXT_DIM));
                if link_button(ui, "Community Standards ↗") {
                    open_url = Some("https://help.roblox.com/hc/articles/203313410-Roblox-Community-Standards");
                }
            });
        }

        if let Some(url) = open_url {
            ui.ctx().open_url(egui::OpenUrl::new_tab(url));
        }
    }

    // ------------------------------------------------------------------ Page 4: Security

    fn draw_security_page(
        &mut self,
        ui: &mut egui::Ui,
        trigger_retry: &mut bool,
        trigger_publish_save: &mut bool,
    ) {
        let mut open_url: Option<String> = None;

        // Studio's exact amber InsecureWarning banner when any risky bypass is enabled
        let risky_local = self.game_settings.local_draft.http_enabled
            || self.game_settings.local_draft.allow_insert_free_assets;
        let risky_cloud = self.game_settings.cloud_draft.as_ref().is_some_and(|d| {
            d.allow_third_party_sales || d.allow_third_party_teleports || d.allow_mesh_texture_apis
        });
        if risky_local || risky_cloud {
            Self::draw_warning_banner(
                ui,
                "Enabling security bypasses may make your experience vulnerable to third party attacks.",
            );
            ui.add_space(12.0);
        }

        // ---- Local DataModel Security Settings (always available, even offline) ----
        section_heading(ui, "Place Settings", "Saved directly in this place file (.rbxl / .rbxlx)");
        ui.add_space(8.0);

        setting_row(ui, "Allow HTTP Requests", |ui| {
            toggle_row(
                ui,
                &mut self.game_settings.local_draft.http_enabled,
                "Allows scripts to send outbound HTTP requests via HttpService (`HttpService.HttpEnabled`).",
            );
        });

        ui.add_space(12.0);

        setting_row(ui, "Allow Loading Third Party Assets", |ui| {
            toggle_row(
                ui,
                &mut self.game_settings.local_draft.allow_insert_free_assets,
                "Enables AssetService APIs to load assets not in your inventory (`AssetService.AllowInsertFreeAssets`).",
            );
            if self.game_settings.local_draft.allow_insert_free_assets {
                ui.add_space(4.0);
                ui.label(
                    RichText::new("You are responsible for assets inserted in your experience complying with Roblox Terms of Use.")
                        .size(11.0)
                        .color(theme::WARN),
                );
            }
        });

        ui.add_space(16.0);
        ui.separator();
        ui.add_space(12.0);

        // ---- Cloud Experience Security Settings ----
        section_heading(ui, "Cloud Experience Permissions", "Synced to develop.roblox.com/v2/universes/{id}/configuration");
        ui.add_space(8.0);

        if self.draw_cloud_gate(ui, trigger_retry, trigger_publish_save) {
            let amp = self
                .game_settings
                .snapshot
                .as_ref()
                .map(|s| s.mesh_texture_amp)
                .unwrap_or(MeshTextureAmpStatus::Unknown);

            if let Some(d) = self.game_settings.cloud_draft.as_mut() {
                setting_row(ui, "Enable Studio Access to API Services", |ui| {
                    toggle_row(
                        ui,
                        &mut d.studio_access_to_apis_allowed,
                        "Allows DataStoreService and other Roblox API services to be accessed from Studio playtests.",
                    );
                });

                ui.add_space(12.0);

                setting_row(ui, "Allow Third Party Sales", |ui| {
                    toggle_row(
                        ui,
                        &mut d.allow_third_party_sales,
                        "By enabling, players can purchase items provided by third parties.",
                    );
                });

                ui.add_space(12.0);

                setting_row(ui, "Allow Third Party Teleports", |ui| {
                    toggle_row(
                        ui,
                        &mut d.allow_third_party_teleports,
                        "By enabling, players can be teleported to other experiences outside your own.",
                    );
                });

                ui.add_space(12.0);

                setting_row(ui, "Allow Mesh / Image APIs", |ui| {
                    let can_edit = matches!(amp, MeshTextureAmpStatus::Granted | MeshTextureAmpStatus::Unknown);
                    ui.add_enabled_ui(can_edit, |ui| {
                        toggle_row(
                            ui,
                            &mut d.allow_mesh_texture_apis,
                            "This experience can access EditableMesh and EditableImage APIs.",
                        );
                    });
                    match amp {
                        MeshTextureAmpStatus::Actionable => {
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new("Requires 13+ ID verification.")
                                        .size(11.5)
                                        .color(theme::WARN),
                                );
                                if link_button(ui, "Verify your age ↗") {
                                    open_url = Some("https://www.roblox.com/my/account#!/info".into());
                                }
                            });
                        }
                        MeshTextureAmpStatus::Denied => {
                            ui.label(
                                RichText::new("You must be 13+ ID verified to access this API.")
                                    .size(11.5)
                                    .color(theme::ERROR),
                            );
                        }
                        _ => {}
                    }
                    if link_button(ui, "Creator Terms of Use ↗") {
                        open_url = Some(
                            "https://help.roblox.com/hc/articles/115004647846-Roblox-Terms-of-Use#creator-terms".into(),
                        );
                    }
                });
            }
        }

        ui.add_space(16.0);
        ui.separator();
        ui.add_space(12.0);

        // ---- Local Secrets Table (`StudioService.Secrets`) ----
        section_heading(
            ui,
            "Secrets",
            "Stored locally on this device for local Playtest mode (`StudioService.Secrets`).",
        );
        ui.add_space(6.0);
        if let Some(o) = &self.game.doc.cloud {
            if link_button(ui, "Manage live production secrets on Creator Hub ↗") {
                open_url = Some(creator_hub::secrets(o.universe_id));
            }
            ui.add_space(6.0);
        }

        let mut edit_row: Option<usize> = None;
        let mut delete_row: Option<usize> = None;
        let mut add_new = false;

        egui::Frame::NONE
            .fill(theme::PANEL_DARK)
            .stroke(Stroke::new(1.0, theme::DIVIDER))
            .corner_radius(CornerRadius::same(5))
            .inner_margin(egui::Margin::same(10))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                if self.game_settings.secrets_draft.is_empty() {
                    ui.label(
                        RichText::new("No local secrets configured. Click \"+ Add Secret\" to create one.")
                            .size(12.0)
                            .color(theme::TEXT_DIM),
                    );
                } else {
                    egui::Grid::new("gs_secrets_table")
                        .num_columns(4)
                        .striped(true)
                        .spacing([14.0, 6.0])
                        .show(ui, |ui| {
                            ui.label(RichText::new("Name").strong().size(12.0));
                            ui.label(RichText::new("Value").strong().size(12.0));
                            ui.label(RichText::new("Domain").strong().size(12.0));
                            ui.label(RichText::new("Actions").strong().size(12.0));
                            ui.end_row();

                            for (i, row) in self.game_settings.secrets_draft.iter().enumerate() {
                                ui.label(RichText::new(&row.name).monospace().size(12.0));
                                let masked = if row.updated { "<updated>" } else { "<hidden>" };
                                let mask_col = if row.updated { STUDIO_BLUE } else { theme::TEXT_DIM };
                                ui.label(RichText::new(masked).size(12.0).color(mask_col));
                                ui.label(RichText::new(&row.domain).monospace().size(12.0));
                                ui.horizontal(|ui| {
                                    if ui.small_button("Edit").clicked() {
                                        edit_row = Some(i);
                                    }
                                    if ui
                                        .add(egui::Button::new(RichText::new("Delete").color(theme::ERROR)).small())
                                        .clicked()
                                    {
                                        delete_row = Some(i);
                                    }
                                });
                                ui.end_row();
                            }
                        });
                }
                ui.add_space(8.0);
                if ui
                    .add(
                        egui::Button::new("+ Add Secret")
                            .fill(theme::PANEL_ALT)
                            .corner_radius(CornerRadius::same(4)),
                    )
                    .clicked()
                {
                    add_new = true;
                }
            });

        if let Some(i) = edit_row {
            let row = self.game_settings.secrets_draft[i].clone();
            self.game_settings.secret_edit = Some(SecretDraft {
                index: Some(i),
                name: row.name,
                new_value: String::new(),
                domain: row.domain,
                error: None,
            });
        } else if add_new {
            self.game_settings.secret_edit = Some(SecretDraft {
                index: None,
                name: String::new(),
                new_value: String::new(),
                domain: "*".into(),
                error: None,
            });
        }
        if let Some(i) = delete_row {
            self.game_settings.secrets_draft.remove(i);
        }

        if let Some(url) = open_url {
            ui.ctx().open_url(egui::OpenUrl::new_tab(url));
        }
    }

    fn draw_edit_secret_subview(&mut self, ui: &mut egui::Ui) {
        let mut cancel = false;
        let mut apply = false;

        ui.horizontal(|ui| {
            if ui
                .add(
                    egui::Button::new("← Back")
                        .fill(theme::PANEL_ALT)
                        .corner_radius(CornerRadius::same(4)),
                )
                .clicked()
            {
                cancel = true;
            }
            ui.add_space(8.0);
            let is_new = self.game_settings.secret_edit.as_ref().is_some_and(|e| e.index.is_none());
            ui.label(
                RichText::new(if is_new { "Add Local Secret" } else { "Edit Secret" })
                    .size(18.0)
                    .strong()
                    .color(Color32::WHITE),
            );
        });
        ui.add_space(6.0);
        ui.separator();
        ui.add_space(10.0);

        ui.label(
            RichText::new("You are editing a draft. Your changes won't be saved until you click Apply, then Save.")
                .size(12.0)
                .color(theme::TEXT_DIM),
        );
        ui.add_space(12.0);

        if let Some(ed) = self.game_settings.secret_edit.as_mut() {
            setting_row(ui, "Name", |ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut ed.name)
                        .desired_width(360.0)
                        .hint_text("e.g. API_KEY"),
                );
            });
            ui.add_space(10.0);
            setting_row(ui, if ed.index.is_some() { "New Value" } else { "Value" }, |ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut ed.new_value)
                        .password(true)
                        .desired_width(360.0)
                        .hint_text("Enter secret value"),
                );
                if ed.index.is_some() {
                    ui.label(
                        RichText::new("Leave empty to keep the current value. Enter \"<blank>\" without quotes to save an empty value.")
                            .size(11.0)
                            .color(theme::TEXT_DIM),
                    );
                }
            });
            ui.add_space(10.0);
            setting_row(ui, "Domain", |ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut ed.domain)
                        .desired_width(360.0)
                        .hint_text("* or api.example.com"),
                );
            });

            if let Some(err) = &ed.error {
                ui.add_space(8.0);
                ui.label(RichText::new(err).size(12.0).color(theme::ERROR));
            }

            ui.add_space(14.0);
            ui.horizontal(|ui| {
                if ui
                    .add(
                        egui::Button::new(RichText::new("Apply").strong().color(Color32::WHITE))
                            .fill(STUDIO_BLUE)
                            .corner_radius(CornerRadius::same(4))
                            .min_size(Vec2::new(88.0, 28.0)),
                    )
                    .clicked()
                {
                    apply = true;
                }
                if ui.button("Discard Draft").clicked() {
                    cancel = true;
                }
            });
        }

        if apply {
            if let Some(mut ed) = self.game_settings.secret_edit.take() {
                let name = ed.name.trim().to_string();
                let domain = ed.domain.trim().to_string();
                let dup = self
                    .game_settings
                    .secrets_draft
                    .iter()
                    .enumerate()
                    .any(|(idx, r)| Some(idx) != ed.index && r.name == name);
                let err = if name.is_empty() {
                    Some("Secret name cannot be empty.".into())
                } else if name.len() > 256 {
                    Some("Secret name cannot be longer than 256 characters.".into())
                } else if dup {
                    Some("Secret name has been used.".into())
                } else if domain.is_empty() {
                    Some("Domain cannot be empty.".into())
                } else if domain.len() > 1024 {
                    Some("Domain name cannot be longer than 1024 characters.".into())
                } else {
                    None
                };
                if let Some(e) = err {
                    ed.error = Some(e);
                    self.game_settings.secret_edit = Some(ed);
                } else {
                    match ed.index {
                        Some(idx) => {
                            let old = self.game_settings.secrets_draft[idx].encoded_value.clone();
                            let (encoded_value, changed) = transform_secret_value(&ed.new_value, &old);
                            self.game_settings.secrets_draft[idx] = SecretRow {
                                name,
                                encoded_value,
                                domain,
                                updated: changed || self.game_settings.secrets_draft[idx].updated,
                            };
                        }
                        None => {
                            let (encoded_value, _) = transform_secret_value(&ed.new_value, "");
                            self.game_settings.secrets_draft.push(SecretRow {
                                name,
                                encoded_value,
                                domain,
                                updated: true,
                            });
                        }
                    }
                    self.game_settings
                        .secrets_draft
                        .sort_by(|a, b| a.name.to_ascii_lowercase().cmp(&b.name.to_ascii_lowercase()));
                }
            }
        } else if cancel {
            self.game_settings.secret_edit = None;
        }
    }

    // ------------------------------------------------------------------ Page 5: Monetization

    fn draw_monetization_page(
        &mut self,
        ui: &mut egui::Ui,
        trigger_retry: &mut bool,
        trigger_publish_save: &mut bool,
        save_dev_product: &mut Option<DevProductDraft>,
    ) {
        if !self.draw_cloud_gate(ui, trigger_retry, trigger_publish_save) {
            return;
        }
        let origin = self.game.doc.cloud.clone().unwrap();
        let snap = self.game_settings.snapshot.clone().unwrap();
        let tax_rate = snap.monetization.tax_rate;
        let min_fee = snap.monetization.minimum_fee;
        let vip_cooldown = self.game_settings.vip_cooldown_err.clone();
        let mut open_url: Option<String> = None;

        if let Some(d) = self.game_settings.cloud_draft.as_mut() {
            // Paid Access
            setting_row(ui, "Paid Access", |ui| {
                toggle_row(
                    ui,
                    &mut d.is_for_sale,
                    "Require players to purchase access with Robux before playing.",
                );
                if d.is_for_sale {
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        ui.label("Price (Robux):");
                        ui.add(egui::TextEdit::singleline(&mut d.price_text).desired_width(90.0));
                        ui.label(RichText::new("(25 – 1,000)").size(11.5).color(theme::TEXT_DIM));
                    });
                    if let Some(price) = d.parsed_price().filter(|&p| (25..=1000).contains(&p)) {
                        let fee = ((price as f64 * tax_rate).round() as u64).max(min_fee);
                        let earn = price.saturating_sub(fee);
                        ui.label(
                            RichText::new(format!(
                                "Marketplace fee ({:.0}%): R$ {fee}  ·  You earn: R$ {earn} per sale",
                                tax_rate * 100.0
                            ))
                            .size(11.5)
                            .color(theme::TEXT_DIM),
                        );
                    } else {
                        ui.label(
                            RichText::new("Enter a valid Robux price between 25 and 1,000.")
                                .size(11.5)
                                .color(theme::ERROR),
                        );
                    }
                }
            });

            ui.add_space(14.0);

            // Private Servers
            setting_row(ui, "Private Servers", |ui| {
                toggle_row(
                    ui,
                    &mut d.allow_private_servers,
                    "Allow players to create private servers for themselves and their friends.",
                );
                if d.allow_private_servers {
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        ui.label("Monthly Price (Robux):");
                        ui.add(egui::TextEdit::singleline(&mut d.private_server_price_text).desired_width(90.0));
                        ui.label(RichText::new("(0 = Free, or ≥ 10)").size(11.5).color(theme::TEXT_DIM));
                    });
                    if let Some(price) = d.parsed_private_server_price() {
                        if price == 0 {
                            ui.label(RichText::new("Free private servers.").size(11.5).color(STUDIO_GREEN));
                        } else if price >= 10 {
                            let fee = ((price as f64 * tax_rate).round() as u64).max(min_fee);
                            let earn = price.saturating_sub(fee);
                            ui.label(
                                RichText::new(format!(
                                    "Marketplace fee ({:.0}%): R$ {fee}  ·  You earn: R$ {earn} / month",
                                    tax_rate * 100.0
                                ))
                                .size(11.5)
                                .color(theme::TEXT_DIM),
                            );
                        } else {
                            ui.label(
                                RichText::new("Price must be 0 (Free) or at least 10 Robux.")
                                    .size(11.5)
                                    .color(theme::ERROR),
                            );
                        }
                    } else {
                        ui.label(
                            RichText::new("Enter a valid number (0 for Free, or ≥ 10).")
                                .size(11.5)
                                .color(theme::ERROR),
                        );
                    }
                }
                if snap.monetization.active_vip_servers > 0 || snap.monetization.active_vip_subscriptions > 0 {
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new(format!(
                            "Active private servers: {}  ·  Active subscriptions: {}",
                            snap.monetization.active_vip_servers, snap.monetization.active_vip_subscriptions
                        ))
                        .size(11.5)
                        .color(theme::TEXT_DIM),
                    );
                }
                if let Some(cool) = &vip_cooldown {
                    ui.label(RichText::new(cool).size(11.5).color(theme::ERROR));
                }
            });

            ui.add_space(14.0);

            // Rewarded On-Demand Ads
            setting_row(ui, "Rewarded Ads", |ui| {
                let eligible = snap.monetization.rewarded_ads_eligible;
                ui.add_enabled_ui(eligible != Some(false), |ui| {
                    toggle_row(
                        ui,
                        &mut d.allow_rewarded_ads,
                        "Allow players to opt in to rewarded video ads for in-experience rewards.",
                    );
                });
                if eligible == Some(false) {
                    ui.label(
                        RichText::new("This experience does not currently meet Roblox's rewarded ads eligibility criteria.")
                            .size(11.5)
                            .color(theme::TEXT_DIM),
                    );
                }
            });
        }

        ui.add_space(16.0);
        ui.separator();
        ui.add_space(12.0);

        // Developer Products
        section_heading(ui, "Developer Products", "Consumable in-experience purchases");
        ui.add_space(6.0);
        let mut edit_dp: Option<DeveloperProduct> = None;
        let mut new_dp = false;

        egui::Frame::NONE
            .fill(theme::PANEL_DARK)
            .stroke(Stroke::new(1.0, theme::DIVIDER))
            .corner_radius(CornerRadius::same(5))
            .inner_margin(egui::Margin::same(10))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                if snap.monetization.dev_products.is_empty() {
                    ui.label(RichText::new("No developer products yet.").size(12.0).color(theme::TEXT_DIM));
                } else {
                    egui::Grid::new("gs_dev_products")
                        .num_columns(4)
                        .striped(true)
                        .spacing([14.0, 5.0])
                        .show(ui, |ui| {
                            ui.label(RichText::new("ID").strong().size(12.0));
                            ui.label(RichText::new("Name").strong().size(12.0));
                            ui.label(RichText::new("Price").strong().size(12.0));
                            ui.label(RichText::new("Action").strong().size(12.0));
                            ui.end_row();
                            for p in &snap.monetization.dev_products {
                                ui.label(RichText::new(p.product_id.to_string()).monospace().size(12.0));
                                ui.label(RichText::new(&p.name).size(12.0));
                                ui.label(RichText::new(format!("R$ {}", p.price_in_robux)).size(12.0));
                                if ui.small_button("Edit").clicked() {
                                    edit_dp = Some(p.clone());
                                }
                                ui.end_row();
                            }
                        });
                }
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui
                        .add(
                            egui::Button::new("+ Create Developer Product")
                                .fill(theme::PANEL_ALT)
                                .corner_radius(CornerRadius::same(4)),
                        )
                        .clicked()
                    {
                        new_dp = true;
                    }
                    if link_button(ui, "Open on Creator Hub ↗") {
                        open_url = Some(creator_hub::developer_products(origin.universe_id));
                    }
                });
            });

        if let Some(p) = edit_dp {
            self.game_settings.dev_product_draft = Some(DevProductDraft {
                product_id: Some(p.product_id),
                name: p.name,
                price_text: p.price_in_robux.to_string(),
                description: p.description,
                error: None,
                busy: false,
            });
        } else if new_dp {
            self.game_settings.dev_product_draft = Some(DevProductDraft {
                product_id: None,
                name: String::new(),
                price_text: "50".into(),
                description: String::new(),
                error: None,
                busy: false,
            });
        }

        if let Some(dp) = self.game_settings.dev_product_draft.as_mut() {
            ui.add_space(8.0);
            let mut close_dp = false;
            egui::Frame::NONE
                .fill(theme::PANEL_DARK)
                .stroke(Stroke::new(1.0, STUDIO_BLUE))
                .corner_radius(CornerRadius::same(5))
                .inner_margin(egui::Margin::same(12))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    let title = if dp.product_id.is_some() {
                        "Edit Developer Product"
                    } else {
                        "New Developer Product"
                    };
                    ui.label(RichText::new(title).strong().size(13.0));
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        ui.label("Name:");
                        ui.add(egui::TextEdit::singleline(&mut dp.name).desired_width(220.0));
                        ui.label("Price (R$):");
                        ui.add(egui::TextEdit::singleline(&mut dp.price_text).desired_width(80.0));
                    });
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.label("Description:");
                        ui.add(egui::TextEdit::singleline(&mut dp.description).desired_width(340.0));
                    });
                    if let Some(e) = &dp.error {
                        ui.label(RichText::new(e).size(11.5).color(theme::ERROR));
                    }
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        let valid = !dp.name.trim().is_empty() && dp.price_text.trim().parse::<u64>().is_ok();
                        if ui
                            .add_enabled(
                                valid && !dp.busy,
                                egui::Button::new(RichText::new("Save Product").color(Color32::WHITE))
                                    .fill(STUDIO_BLUE)
                                    .corner_radius(CornerRadius::same(4)),
                            )
                            .clicked()
                        {
                            *save_dev_product = Some(dp.clone());
                        }
                        if ui.add_enabled(!dp.busy, egui::Button::new("Cancel")).clicked() {
                            close_dp = true;
                        }
                        if dp.busy {
                            ui.spinner();
                        }
                    });
                });
            if close_dp {
                self.game_settings.dev_product_draft = None;
            }
        }

        ui.add_space(14.0);

        // Badges summary
        section_heading(ui, "Badges", "Read-only overview — create & edit badges on Creator Hub");
        ui.add_space(6.0);
        egui::Frame::NONE
            .fill(theme::PANEL_DARK)
            .stroke(Stroke::new(1.0, theme::DIVIDER))
            .corner_radius(CornerRadius::same(5))
            .inner_margin(egui::Margin::same(10))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                if snap.monetization.badges.is_empty() {
                    ui.label(RichText::new("No badges found for this experience.").size(12.0).color(theme::TEXT_DIM));
                } else {
                    egui::Grid::new("gs_badges_table")
                        .num_columns(3)
                        .striped(true)
                        .spacing([14.0, 4.0])
                        .show(ui, |ui| {
                            ui.label(RichText::new("ID").strong().size(12.0));
                            ui.label(RichText::new("Name").strong().size(12.0));
                            ui.label(RichText::new("Status").strong().size(12.0));
                            ui.end_row();
                            for b in &snap.monetization.badges {
                                ui.label(RichText::new(b.id.to_string()).monospace().size(12.0));
                                ui.label(RichText::new(&b.name).size(12.0));
                                let (lbl, col) = if b.enabled {
                                    ("Enabled", STUDIO_GREEN)
                                } else {
                                    ("Disabled", theme::TEXT_DIM)
                                };
                                ui.label(RichText::new(lbl).size(11.5).color(col));
                                ui.end_row();
                            }
                        });
                }
                ui.add_space(6.0);
                if link_button(ui, "Manage Badges on Creator Hub ↗") {
                    open_url = Some(creator_hub::badges(origin.universe_id));
                }
            });

        if let Some(url) = open_url {
            ui.ctx().open_url(egui::OpenUrl::new_tab(url));
        }
    }

    // ------------------------------------------------------------------ Page 6: World (local DataModel)

    fn draw_world_page(&mut self, ui: &mut egui::Ui) {
        section_heading(
            ui,
            "Physics & Character",
            "Edits Workspace & StarterPlayer properties directly in this place (undoable, works offline).",
        );
        ui.add_space(10.0);

        let d = &mut self.game_settings.local_draft;

        // Gravity + Studio's 3 presets
        setting_row(ui, "Gravity", |ui| {
            ui.horizontal(|ui| {
                ui.add(
                    egui::DragValue::new(&mut d.gravity)
                        .range(0.0..=1000.0)
                        .speed(0.5)
                        .suffix(" studs/s²"),
                );
                ui.add_space(8.0);
                for (name, val) in [("Classic", 196.2f32), ("Action", 75.0), ("Realistic", 35.0)] {
                    let active = (d.gravity - val).abs() < 0.05;
                    let btn = egui::Button::new(RichText::new(name).size(11.5))
                        .fill(if active { theme::SELECT_BG } else { theme::PANEL_ALT })
                        .corner_radius(CornerRadius::same(3));
                    if ui.add(btn).clicked() {
                        d.gravity = val;
                    }
                }
            });
            ui.label(
                RichText::new("Sets `Workspace.Gravity` (default 196.2 studs/s²).")
                    .size(11.5)
                    .color(theme::TEXT_DIM),
            );
        });

        ui.add_space(12.0);

        // WalkSpeed
        setting_row(ui, "Walk Speed", |ui| {
            ui.add(
                egui::DragValue::new(&mut d.walk_speed)
                    .range(0.0..=500.0)
                    .speed(0.25)
                    .suffix(" studs/s"),
            );
            ui.label(
                RichText::new("Sets `StarterPlayer.CharacterWalkSpeed` (default 16).")
                    .size(11.5)
                    .color(theme::TEXT_DIM),
            );
        });

        ui.add_space(12.0);

        // Jump Height vs Jump Power
        setting_row(ui, "Jump Mode", |ui| {
            ui.horizontal(|ui| {
                ui.radio_value(&mut d.use_jump_power, false, "Jump Height");
                ui.add_space(10.0);
                ui.radio_value(&mut d.use_jump_power, true, "Jump Power");
            });
            ui.add_space(4.0);
            if d.use_jump_power {
                ui.add(
                    egui::DragValue::new(&mut d.jump_power)
                        .range(0.0..=1000.0)
                        .speed(0.5),
                );
                ui.label(
                    RichText::new("Sets `StarterPlayer.CharacterJumpPower` (default 50).")
                        .size(11.5)
                        .color(theme::TEXT_DIM),
                );
            } else {
                ui.add(
                    egui::DragValue::new(&mut d.jump_height)
                        .range(0.0..=500.0)
                        .speed(0.1)
                        .suffix(" studs"),
                );
                ui.label(
                    RichText::new("Sets `StarterPlayer.CharacterJumpHeight` (default 7.2 studs).")
                        .size(11.5)
                        .color(theme::TEXT_DIM),
                );
            }
        });

        ui.add_space(12.0);

        // Max Slope Angle
        setting_row(ui, "Max Slope Angle", |ui| {
            ui.add(
                egui::DragValue::new(&mut d.max_slope_angle)
                    .range(0.0..=89.9)
                    .speed(0.5)
                    .suffix("°"),
            );
            ui.label(
                RichText::new("Sets `StarterPlayer.CharacterMaxSlopeAngle` (default 89°).")
                    .size(11.5)
                    .color(theme::TEXT_DIM),
            );
        });

        ui.add_space(12.0);

        // Auto Jump
        setting_row(ui, "Auto Jump", |ui| {
            toggle_row(
                ui,
                &mut d.auto_jump,
                "Automatically jump over low obstacles on touch devices (`StarterPlayer.AutoJumpEnabled`).",
            );
        });

        ui.add_space(14.0);
        ui.separator();
        ui.add_space(12.0);

        // Avatar Type (synced with StarterPlayer.GameSettingsAvatar & AvatarSettings)
        setting_row(ui, "Avatar Type", |ui| {
            ui.horizontal(|ui| {
                ui.radio_value(&mut d.avatar_type, 1, "R15");
                ui.add_space(10.0);
                ui.radio_value(&mut d.avatar_type, 0, "R6");
                ui.add_space(10.0);
                ui.radio_value(&mut d.avatar_type, 2, "Player Choice");
            });
            ui.label(
                RichText::new("Updates `StarterPlayer.GameSettingsAvatar` and syncs to Roblox on Publish.")
                    .size(11.5)
                    .color(theme::TEXT_DIM),
            );
        });
    }

    // ------------------------------------------------------------------ Page 7: Additional Settings (CreatorHubPage)

    fn draw_creator_hub_page(&mut self, ui: &mut egui::Ui, trigger_publish_save: &mut bool) {
        ui.label(
            RichText::new("We have streamlined Game Settings in Studio")
                .size(15.0)
                .strong()
                .color(Color32::WHITE),
        );
        ui.add_space(4.0);
        ui.label(
            RichText::new("Additional settings can be found on Creator Hub")
                .size(12.5)
                .color(theme::TEXT_DIM),
        );
        ui.add_space(14.0);

        let Some(o) = self.game.doc.cloud.clone() else {
            self.draw_unpublished_banner(
                ui,
                trigger_publish_save,
                Some("Once saved to Roblox, direct Creator Hub links for this experience will appear here."),
            );
            return;
        };

        let u = o.universe_id;
        let links = [
            ("Audience > Access", "Manage playable devices, paid access, and private servers.", creator_hub::access(u)),
            ("Configure > Settings", "Set experience icon and manage content settings.", creator_hub::configure(u)),
            ("Configure > Questionnaire", "Set your experience's content maturity label.", creator_hub::questionnaire(u)),
            ("Configure > Places", "Access place settings and thumbnails.", creator_hub::places(u)),
            ("Monetization > Developer Products", "Create and manage developer products.", creator_hub::developer_products(u)),
            ("Engagement > Badges", "Create and manage badges.", creator_hub::badges(u)),
            ("Audience > Localization", "Manage language support and translation settings.", creator_hub::localization(u)),
        ];

        for (title, desc, url) in links {
            let resp = egui::Frame::NONE
                .fill(theme::PANEL_DARK)
                .stroke(Stroke::new(1.0, theme::DIVIDER))
                .corner_radius(CornerRadius::same(5))
                .inner_margin(egui::Margin::symmetric(14, 10))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            ui.label(RichText::new(title).strong().size(13.0).color(STUDIO_BLUE));
                            ui.label(RichText::new(desc).size(12.0).color(theme::TEXT_DIM));
                        });
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let (ir, _) = ui.allocate_exact_size(Vec2::splat(16.0), Sense::hover());
                            rustudio_engine::ti::paint(ui.painter(), ir, "external-link", theme::TEXT_DIM);
                        });
                    });
                })
                .response
                .interact(Sense::click())
                .on_hover_text(&url);
            if resp.clicked() {
                ui.ctx().open_url(egui::OpenUrl::new_tab(url));
            }
            ui.add_space(6.0);
        }
    }

    // ------------------------------------------------------------------ Modals

    fn draw_confirm_modals(&mut self, ctx: &egui::Context) {
        if self.game_settings.confirm_discard {
            let mut choice: Option<bool> = None;
            egui::Modal::new(egui::Id::new("gs_discard_modal")).show(ctx, |ui| {
                ui.set_max_width(380.0);
                ui.label(RichText::new("Discard Changes?").strong().size(15.0));
                ui.add_space(6.0);
                ui.label("You have unsaved Experience Settings changes. Would you like to discard all changes?");
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    if ui
                        .add(
                            egui::Button::new(RichText::new("Discard").strong().color(Color32::WHITE))
                                .fill(theme::ERROR)
                                .corner_radius(CornerRadius::same(4)),
                        )
                        .clicked()
                    {
                        choice = Some(true);
                    }
                    if ui.button("Keep Editing").clicked() {
                        choice = Some(false);
                    }
                });
            });
            if let Some(discard) = choice {
                self.game_settings.confirm_discard = false;
                if discard {
                    if let Some(snap) = &self.game_settings.snapshot {
                        self.game_settings.cloud_draft = Some(CloudDraft::from_snapshot(snap));
                    }
                    self.game_settings.local_draft = self.game_settings.local_base.clone();
                    self.game_settings.secrets_draft = self.game_settings.secrets_base.clone();
                    self.game_settings.pending_icon = None;
                    self.game_settings.pending_icon_tex = None;
                    self.game_settings.pending_thumbs.clear();
                    self.game_settings.pending_thumb_tex.clear();
                    self.game_settings.secret_edit = None;
                    self.cloud.settings_open = false;
                }
            }
        }

        if self.game_settings.confirm_console {
            let mut choice: Option<bool> = None;
            egui::Modal::new(egui::Id::new("gs_console_agreement")).show(ctx, |ui| {
                ui.set_max_width(440.0);
                ui.label(RichText::new("Console Content Guidelines").strong().size(15.0));
                ui.add_space(6.0);
                ui.label(
                    "By enabling Console playability, you confirm that your experience follows Roblox's Console Content Guidelines (including gamepad navigation support and no prohibited content).",
                );
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    if ui
                        .add(
                            egui::Button::new(RichText::new("Agree & Enable Console").strong().color(Color32::WHITE))
                                .fill(STUDIO_BLUE)
                                .corner_radius(CornerRadius::same(4)),
                        )
                        .clicked()
                    {
                        choice = Some(true);
                    }
                    if ui.button("Cancel").clicked() {
                        choice = Some(false);
                    }
                });
            });
            if let Some(agree) = choice {
                self.game_settings.confirm_console = false;
                if agree {
                    if let Some(d) = self.game_settings.cloud_draft.as_mut() {
                        if !d.playable_devices.iter().any(|x| x == "Console") {
                            d.playable_devices.push("Console".to_string());
                        }
                    }
                }
            }
        }
    }
}

// ------------------------------------------------------------------ Layout primitives

fn section_heading(ui: &mut egui::Ui, title: &str, subtitle: &str) {
    ui.label(RichText::new(title).size(14.5).strong().color(Color32::WHITE));
    ui.label(RichText::new(subtitle).size(11.5).color(theme::TEXT_DIM));
}

fn setting_row(ui: &mut egui::Ui, label: &str, body: impl FnOnce(&mut egui::Ui)) {
    ui.horizontal_top(|ui| {
        ui.allocate_ui_with_layout(
            Vec2::new(LABEL_COL_WIDTH, 22.0),
            egui::Layout::top_down(egui::Align::LEFT),
            |ui| {
                ui.set_min_width(LABEL_COL_WIDTH);
                ui.set_max_width(LABEL_COL_WIDTH);
                ui.add_space(2.0);
                ui.add(
                    egui::Label::new(RichText::new(label).size(13.0).color(theme::TEXT)).wrap(),
                );
            },
        );
        ui.add_space(10.0);
        ui.vertical(|ui| {
            body(ui);
        });
    });
}

fn toggle_row(ui: &mut egui::Ui, value: &mut bool, description: &str) {
    ui.horizontal(|ui| {
        studio_toggle(ui, value);
        ui.add_space(6.0);
        ui.label(
            RichText::new(if *value { "On" } else { "Off" })
                .size(12.0)
                .strong()
                .color(if *value { STUDIO_BLUE } else { theme::TEXT_DIM }),
        );
    });
    ui.add_space(3.0);
    ui.add(
        egui::Label::new(RichText::new(description).size(11.5).color(theme::TEXT_DIM)).wrap(),
    );
}

/// Studio-style pill toggle switch (exact visual feel of Studio's `ToggleButton`).
fn studio_toggle(ui: &mut egui::Ui, on: &mut bool) -> egui::Response {
    let desired = Vec2::new(36.0, 20.0);
    let (rect, mut resp) = ui.allocate_exact_size(desired, Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    let p = ui.painter();
    let radius = rect.height() * 0.5;
    let bg = if *on {
        if resp.hovered() { STUDIO_BLUE_HOVER } else { STUDIO_BLUE }
    } else if resp.hovered() {
        Color32::from_rgb(76, 80, 92)
    } else {
        Color32::from_rgb(60, 63, 74)
    };
    p.rect_filled(rect, radius, bg);
    let knob_x = if *on { rect.max.x - radius } else { rect.min.x + radius };
    p.circle_filled(Pos2::new(knob_x, rect.center().y), radius - 3.0, Color32::WHITE);
    resp
}

fn link_button(ui: &mut egui::Ui, text: &str) -> bool {
    ui.add(
        egui::Button::new(RichText::new(text).size(11.5).color(STUDIO_BLUE))
            .frame(false),
    )
    .clicked()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secrets_json_roundtrip_matches_studio_format() {
        let rows = vec![
            SecretRow {
                name: "B_TOKEN".into(),
                encoded_value: base64_encode(b"hello"),
                domain: "api.example.com".into(),
                updated: false,
            },
            SecretRow {
                name: "a_key".into(),
                encoded_value: base64_encode(b"world"),
                domain: "*".into(),
                updated: true,
            },
        ];
        let json = encode_secrets_json(&rows);
        assert!(json.contains("\"a_key\""));
        assert!(json.contains("d29ybGQ="));
        let parsed = parse_secrets_json(&json);
        assert_eq!(parsed.len(), 2);
        // Sorted case-insensitively like Studio's SecretUtils.sortAndReindexSecretsTableRows
        assert_eq!(parsed[0].name, "a_key");
        assert_eq!(parsed[0].domain, "*");
        assert_eq!(parsed[1].name, "B_TOKEN");
        assert_eq!(parsed[1].encoded_value, "aGVsbG8=");
    }

    #[test]
    fn transform_secret_value_handles_empty_blank_and_new_secret() {
        assert_eq!(transform_secret_value("", "cHJldg=="), ("cHJldg==".into(), false));
        assert_eq!(transform_secret_value("<blank>", "cHJldg=="), ("".into(), true));
        assert_eq!(transform_secret_value("abc", "cHJldg=="), ("YWJj".into(), true));
    }

    #[test]
    fn build_config_patch_only_includes_changed_fields() {
        let mut st = GameSettingsState::default();
        let mut snap = GameSettingsSnapshot::default();
        snap.can_manage_config = true;
        snap.config.name = "Old Name".into();
        snap.config.description = "Old Desc".into();
        snap.config.genre = "All".into();
        snap.config.playable_devices = vec!["Computer".into()];
        snap.config.is_active = true;
        let mut draft = CloudDraft::from_snapshot(&snap);
        draft.name = "New Name".into();
        draft.allow_third_party_sales = true;
        st.snapshot = Some(snap);
        st.cloud_draft = Some(draft);

        let patch = st.build_config_patch();
        let val = patch.to_json();
        let obj = val.as_object().unwrap();
        assert_eq!(obj.len(), 2);
        assert_eq!(obj["name"], "New Name");
        assert_eq!(obj["permissions"]["IsThirdPartyPurchaseAllowed"], true);
    }
}
