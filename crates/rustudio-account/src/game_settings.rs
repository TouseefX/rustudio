//! Roblox Studio's Game Settings / Experience Settings API client, reverse
//! engineered directly from Studio 0.748's `GameSettings.rbxm`:
//! - `GameInfoController`: `develop/v2/universes/{id}/configuration`,
//!   `develop/v1/universes/{id}/icon`, `games/v1/games/{id}/media`,
//!   `publish/v1/games/{id}/icon`, `publish/v1/games/{id}/thumbnail/image`,
//!   `develop/v1/universes/{id}/thumbnails/{tid}`, `/thumbnails/order`,
//!   `/thumbnails/alt-text`, `apis/experience-guidelines-service/v1beta1/multi-age-recommendation`,
//!   `apis/experience-genre-api/v1/Creator/ExperienceGenre`
//! - `CommunicationController`: `voice/v1/settings`,
//!   `voice/v1/settings/universe/{id}`, `/avatarvideo/{id}`, `/chat-group-api/{id}`
//! - `SecurityController`: `develop/v2/universes/{id}/configuration`
//!   (`studioAccessToApisAllowed`, `permissions.IsThirdPartyPurchaseAllowed`,
//!   `permissions.IsThirdPartyTeleportAllowed`, `isMeshTextureApiAccessAllowed`),
//!   `apis/access-management/v1/feature-access?featureNames=CanEnableMeshTextureApi`
//! - `GamePermissionsController`: `develop/v1/universes/{id}` (`isActive`),
//!   `POST /activate`, `POST /deactivate`, `isFriendsOnly`
//! - `MonetizationController`: `economy/v1/resale-tax-rate`,
//!   `develop/v1/universes/{id}/configuration/vip-servers`,
//!   `apis/developer-ads-stats-api/v1/universe-suitability-criteria/{id}`,
//!   `apis/developer-products/v2/universes/{id}/developerproducts`,
//!   `apis/developer-products/v1/universes/{id}/developerproducts`,
//!   `badges/v1/universes/{id}/badges`
//! - `PlacesController`: `develop/v1/assets/{rootPlaceId}/published-versions`

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use crate::cloud::{error_message, json as parse_json, CloudApi, CreatorKind, TeamCreateStatus};
use crate::{Error, Result};

/// Studio's exact multipart boundary (`FileUtils.lua:136`).
pub const PUBLISH_MULTIPART_BOUNDARY: &str = "EA0A21C3-8388-4038-9BD5-92C8B1B7BF8E";

/// Playable devices accepted by `develop/v2/universes/{id}/configuration`.
pub const PLAYABLE_DEVICES: [&str; 5] = ["Computer", "Phone", "Tablet", "Console", "VR"];

/// Legacy genres accepted by `develop/v2/universes/{id}/configuration`.
pub const LEGACY_GENRES: [&str; 15] = [
    "All",
    "Adventure",
    "Building",
    "Comedy",
    "Fighting",
    "FPS",
    "Horror",
    "Medieval",
    "Military",
    "Naval",
    "RPG",
    "SciFi",
    "Sports",
    "TownAndCity",
    "Western",
];

/// Playability state exposed on Studio's Permissions page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Playability {
    Public,
    Friends,
    #[default]
    Private,
}

impl Playability {
    pub fn from_flags(is_active: bool, is_friends_only: bool) -> Self {
        if !is_active {
            Playability::Private
        } else if is_friends_only {
            Playability::Friends
        } else {
            Playability::Public
        }
    }

    pub fn is_active(self) -> bool {
        !matches!(self, Playability::Private)
    }

    pub fn is_friends_only(self) -> bool {
        matches!(self, Playability::Friends)
    }

    pub fn label(self) -> &'static str {
        match self {
            Playability::Public => "Public",
            Playability::Friends => "Friends",
            Playability::Private => "Private",
        }
    }
}

/// Access-management status for `CanEnableMeshTextureApi` (`SecurityController`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MeshTextureAmpStatus {
    Granted,
    Actionable,
    Denied,
    #[default]
    Unknown,
}

impl MeshTextureAmpStatus {
    pub fn parse(s: &str) -> Self {
        match s {
            "Granted" => Self::Granted,
            "Actionable" => Self::Actionable,
            "Denied" => Self::Denied,
            _ => Self::Unknown,
        }
    }
}

/// Core configuration from `GET develop/v2/universes/{id}/configuration` +
/// `GET develop/v1/universes/{id}`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct UniverseConfiguration {
    pub universe_id: u64,
    pub root_place_id: u64,
    pub creator_type: CreatorKind,
    pub creator_id: u64,
    pub creator_name: String,
    pub name: String,
    pub description: String,
    pub genre: String,
    pub playable_devices: Vec<String>,
    pub is_active: bool,
    pub is_friends_only: bool,
    pub is_for_sale: bool,
    pub price: Option<u64>,
    pub allow_private_servers: bool,
    pub private_server_price: Option<u64>,
    pub studio_access_to_apis_allowed: bool,
    pub allow_third_party_sales: bool,
    pub allow_third_party_teleports: bool,
    pub allow_mesh_texture_apis: bool,
    pub allow_rewarded_ads: bool,
    pub universe_avatar_type: String,
    pub opt_in_regions: Vec<String>,
}

impl UniverseConfiguration {
    pub fn playability(&self) -> Playability {
        Playability::from_flags(self.is_active, self.is_friends_only)
    }
}

/// Experience icon metadata (`GET develop/v1/universes/{id}/icon`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ExperienceIcon {
    pub image_id: Option<u64>,
    pub is_approved: bool,
}

/// One screenshot or video thumbnail (`GET games/v1/games/{id}/media`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ExperienceThumbnail {
    pub id: u64,
    pub image_id: Option<u64>,
    pub asset_type: String,
    pub approved: bool,
    pub alt_text: String,
    pub video_hash: Option<String>,
    pub video_title: Option<String>,
}

/// Experience content maturity guidelines
/// (`POST apis/experience-guidelines-service/v1beta1/multi-age-recommendation`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AgeGuidelines {
    pub content_maturity: String,
    pub display_name: String,
}

/// Communication / Spatial Voice / Camera settings (`voice.roblox.com`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CommunicationSettings {
    pub user_avatar_chat_enabled: bool,
    pub audio_enabled: bool,
    pub video_enabled: bool,
    pub chat_group_api_enabled: bool,
}

/// One Developer Product (`apis/developer-products/v2/universes/{id}/developerproducts`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DeveloperProduct {
    pub id: u64,
    pub product_id: u64,
    pub name: String,
    pub price_in_robux: u64,
    pub description: String,
}

/// One Experience Badge (`badges/v1/universes/{id}/badges`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ExperienceBadge {
    pub id: u64,
    pub name: String,
    pub description: String,
    pub enabled: bool,
    pub icon_image_id: u64,
}

/// Monetization metadata & items (`economy`, `develop`, `apis`, `badges`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MonetizationSummary {
    /// Marketplace fee fraction (e.g. `0.3` = 30% fee).
    pub tax_rate: f64,
    pub minimum_fee: u64,
    pub active_vip_servers: u64,
    pub active_vip_subscriptions: u64,
    pub rewarded_ads_eligible: Option<bool>,
    pub dev_products: Vec<DeveloperProduct>,
    pub badges: Vec<ExperienceBadge>,
}

/// Complete snapshot fetched when opening Experience Settings for a cloud place.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct GameSettingsSnapshot {
    /// True when the signed-in account can read & edit universe configuration
    /// (`GET develop/v2/universes/{id}/configuration` succeeded). False on
    /// collaborator / group places where the user only has edit/play access.
    pub can_manage_config: bool,
    pub config: UniverseConfiguration,
    pub icon: ExperienceIcon,
    pub thumbnails: Vec<ExperienceThumbnail>,
    pub guidelines: Option<AgeGuidelines>,
    pub experience_genre: Option<String>,
    pub communication: CommunicationSettings,
    pub mesh_texture_amp: MeshTextureAmpStatus,
    pub monetization: MonetizationSummary,
    pub has_published_version: bool,
    pub team_create: TeamCreateStatus,
}

/// Minimal diff sent to `PATCH develop/v2/universes/{id}/configuration`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConfigurationPatch {
    pub name: Option<String>,
    pub description: Option<String>,
    pub genre: Option<String>,
    pub playable_devices: Option<Vec<String>>,
    pub is_friends_only: Option<bool>,
    pub is_for_sale: Option<bool>,
    pub price: Option<u64>,
    pub allow_private_servers: Option<bool>,
    pub private_server_price: Option<u64>,
    pub studio_access_to_apis_allowed: Option<bool>,
    pub allow_third_party_sales: Option<bool>,
    pub allow_third_party_teleports: Option<bool>,
    pub allow_mesh_texture_apis: Option<bool>,
    pub allow_rewarded_ads: Option<bool>,
}

impl ConfigurationPatch {
    pub fn is_empty(&self) -> bool {
        self.name.is_none()
            && self.description.is_none()
            && self.genre.is_none()
            && self.playable_devices.is_none()
            && self.is_friends_only.is_none()
            && self.is_for_sale.is_none()
            && self.price.is_none()
            && self.allow_private_servers.is_none()
            && self.private_server_price.is_none()
            && self.studio_access_to_apis_allowed.is_none()
            && self.allow_third_party_sales.is_none()
            && self.allow_third_party_teleports.is_none()
            && self.allow_mesh_texture_apis.is_none()
            && self.allow_rewarded_ads.is_none()
    }

    pub fn to_json(&self) -> Value {
        let mut m = Map::new();
        if let Some(v) = &self.name {
            m.insert("name".into(), json!(v));
        }
        if let Some(v) = &self.description {
            m.insert("description".into(), json!(v));
        }
        if let Some(v) = &self.genre {
            m.insert("genre".into(), json!(v));
        }
        if let Some(v) = &self.playable_devices {
            m.insert("playableDevices".into(), json!(v));
        }
        if let Some(v) = self.is_friends_only {
            m.insert("isFriendsOnly".into(), json!(v));
        }
        if let Some(v) = self.is_for_sale {
            m.insert("isForSale".into(), json!(v));
        }
        if let Some(v) = self.price {
            m.insert("price".into(), json!(v));
        }
        if let Some(v) = self.allow_private_servers {
            m.insert("allowPrivateServers".into(), json!(v));
        }
        if let Some(v) = self.private_server_price {
            m.insert("privateServerPrice".into(), json!(v));
        }
        if let Some(v) = self.studio_access_to_apis_allowed {
            m.insert("studioAccessToApisAllowed".into(), json!(v));
        }
        if self.allow_third_party_sales.is_some() || self.allow_third_party_teleports.is_some() {
            let mut perms = Map::new();
            if let Some(v) = self.allow_third_party_sales {
                perms.insert("IsThirdPartyPurchaseAllowed".into(), json!(v));
            }
            if let Some(v) = self.allow_third_party_teleports {
                perms.insert("IsThirdPartyTeleportAllowed".into(), json!(v));
            }
            m.insert("permissions".into(), Value::Object(perms));
        }
        if let Some(v) = self.allow_mesh_texture_apis {
            m.insert("isMeshTextureApiAccessAllowed".into(), json!(v));
        }
        if let Some(v) = self.allow_rewarded_ads {
            m.insert("isRewardedOnDemandAdsAllowed".into(), json!(v));
        }
        Value::Object(m)
    }
}

/// Structured error from `PATCH develop/v2/universes/{id}/configuration` so the
/// UI can highlight moderated fields the same way Studio's `GameInfoController` does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigSaveError {
    pub message: String,
    pub name_moderated: bool,
    pub description_moderated: bool,
    pub vip_price_cooldown: Option<String>,
}

impl std::fmt::Display for ConfigSaveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

/// Builds Studio's exact `multipart/form-data` payload for
/// `publish.roblox.com/v1/games/{id}/icon` and `/thumbnail/image`
/// (`FileUtils.GetAssetPublishRequestInfo`).
pub fn build_publish_image_multipart(filename: &str, bytes: &[u8]) -> (String, Vec<u8>) {
    let lower = filename.to_ascii_lowercase();
    let ext = lower.rsplit('.').next().unwrap_or("png");
    let mime_sub = match ext {
        "jpg" | "jpeg" => "jpeg",
        "png" => "png",
        other => other,
    };
    let safe_name = filename.replace('"', "_");
    let content_type = format!("multipart/form-data; boundary={PUBLISH_MULTIPART_BOUNDARY}");
    let mut body = Vec::with_capacity(bytes.len() + 256);
    body.extend_from_slice(
        format!(
            "--{PUBLISH_MULTIPART_BOUNDARY}\r\n\
             Content-Type: image/{mime_sub}\r\n\
             Content-Disposition: form-data; filename=\"{safe_name}\"; name=\"request.files\"\r\n\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(bytes);
    body.extend_from_slice(format!("\r\n--{PUBLISH_MULTIPART_BOUNDARY}--\r\n").as_bytes());
    (content_type, body)
}

/// Official Roblox Creator Hub deep links from `BrowserUtils.lua` (`GameSettings.rbxm`).
pub mod creator_hub {
    pub fn configure(universe_id: u64) -> String {
        format!("https://create.roblox.com/dashboard/creations/experiences/{universe_id}/configure")
    }
    pub fn access(universe_id: u64) -> String {
        format!("https://create.roblox.com/dashboard/creations/experiences/{universe_id}/access")
    }
    pub fn questionnaire(universe_id: u64) -> String {
        format!("https://create.roblox.com/dashboard/creations/experiences/{universe_id}/experience-questionnaire")
    }
    pub fn places(universe_id: u64) -> String {
        format!("https://create.roblox.com/dashboard/creations/experiences/{universe_id}/places")
    }
    pub fn developer_products(universe_id: u64) -> String {
        format!("https://create.roblox.com/dashboard/creations/experiences/{universe_id}/monetization/developer-products")
    }
    pub fn badges(universe_id: u64) -> String {
        format!("https://create.roblox.com/dashboard/creations/experiences/{universe_id}/associated-items?activeTab=Badge")
    }
    pub fn localization(universe_id: u64) -> String {
        format!("https://create.roblox.com/dashboard/creations/experiences/{universe_id}/localization")
    }
    pub fn collaborators(universe_id: u64) -> String {
        format!("https://create.roblox.com/dashboard/creations/experiences/{universe_id}/collaboration")
    }
    pub fn secrets(universe_id: u64) -> String {
        format!("https://create.roblox.com/dashboard/creations/experiences/{universe_id}/secrets")
    }
}

/// Formats Studio's `experience-genre-api` taxonomy slug (e.g.
/// `"game.action.battlegrounds_and_fighting"`) into a clean display path.
pub fn format_genre_slug(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let sep = if trimmed.contains('/') {
        '/'
    } else if trimmed.contains('.') {
        '.'
    } else {
        return title_case_slug(trimmed);
    };
    trimmed
        .split(sep)
        .filter(|s| !s.is_empty())
        .map(title_case_slug)
        .collect::<Vec<_>>()
        .join(" > ")
}

fn title_case_slug(seg: &str) -> String {
    seg.split(['_', '-'])
        .filter(|w| !w.is_empty())
        .map(|w| match w.to_ascii_lowercase().as_str() {
            "and" => "&".to_string(),
            "fps" => "FPS".to_string(),
            "rpg" => "RPG".to_string(),
            "vr" => "VR".to_string(),
            "sim" => "Sim".to_string(),
            "vs" => "vs".to_string(),
            other => {
                let mut ch = other.chars();
                match ch.next() {
                    Some(c) => format!("{}{}", c.to_ascii_uppercase(), ch.as_str()),
                    None => String::new(),
                }
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn str_field(v: &Value, k: &str) -> String {
    v.get(k).and_then(Value::as_str).unwrap_or("").to_string()
}

fn bool_field(v: &Value, k: &str) -> bool {
    v.get(k).and_then(Value::as_bool).unwrap_or(false)
}

fn u64_field(v: &Value, k: &str) -> Option<u64> {
    v.get(k).and_then(Value::as_u64)
}

fn parse_configuration(universe_id: u64, cfg: &Value, univ: Option<&Value>) -> UniverseConfiguration {
    let playable_devices = cfg
        .get("playableDevices")
        .and_then(Value::as_array)
        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
        .unwrap_or_default();

    let perms = cfg.get("permissions");
    let allow_third_party_sales = perms
        .and_then(|p| p.get("IsThirdPartyPurchaseAllowed").or_else(|| p.get("isThirdPartyPurchaseAllowed")))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let allow_third_party_teleports = perms
        .and_then(|p| p.get("IsThirdPartyTeleportAllowed").or_else(|| p.get("isThirdPartyTeleportAllowed")))
        .and_then(Value::as_bool)
        .unwrap_or(false);

    let opt_in_regions = cfg
        .get("optInRegions")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|v| {
                    v.as_str()
                        .or_else(|| v.get("region").and_then(Value::as_str))
                        .map(str::to_string)
                })
                .collect()
        })
        .unwrap_or_default();

    let is_active = univ
        .and_then(|u| u.get("isActive"))
        .and_then(Value::as_bool)
        .or_else(|| cfg.get("isActive").and_then(Value::as_bool))
        .unwrap_or(false);

    let root_place_id = univ
        .and_then(|u| u.get("rootPlaceId"))
        .and_then(Value::as_u64)
        .or_else(|| u64_field(cfg, "rootPlaceId"))
        .unwrap_or(0);

    let creator_type = univ
        .and_then(|u| u.get("creatorType"))
        .and_then(Value::as_str)
        .map(CreatorKind::parse)
        .unwrap_or(CreatorKind::User);
    let creator_id = univ
        .and_then(|u| u.get("creatorTargetId"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let creator_name = univ
        .and_then(|u| u.get("creatorName"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();

    UniverseConfiguration {
        universe_id,
        root_place_id,
        creator_type,
        creator_id,
        creator_name,
        name: str_field(cfg, "name"),
        description: str_field(cfg, "description"),
        genre: str_field(cfg, "genre"),
        playable_devices,
        is_active,
        is_friends_only: bool_field(cfg, "isFriendsOnly"),
        is_for_sale: bool_field(cfg, "isForSale"),
        price: u64_field(cfg, "price"),
        allow_private_servers: bool_field(cfg, "allowPrivateServers"),
        private_server_price: u64_field(cfg, "privateServerPrice"),
        studio_access_to_apis_allowed: bool_field(cfg, "studioAccessToApisAllowed"),
        allow_third_party_sales,
        allow_third_party_teleports,
        allow_mesh_texture_apis: bool_field(cfg, "isMeshTextureApiAccessAllowed"),
        allow_rewarded_ads: bool_field(cfg, "isRewardedOnDemandAdsAllowed"),
        universe_avatar_type: str_field(cfg, "universeAvatarType"),
        opt_in_regions,
    }
}

impl CloudApi {
    /// Fetches the complete Game Settings snapshot for a universe by fanning
    /// out all read endpoints in parallel (mirroring Studio's `GameSettings.rbxm`
    /// page load jobs). Core configuration failure is returned as `Err`;
    /// secondary endpoints (guidelines, genre, voice, AMP, monetization, media)
    /// degrade gracefully when unavailable or forbidden.
    pub fn fetch_game_settings(&self, universe_id: u64, root_place_id: u64) -> Result<GameSettingsSnapshot> {
        if universe_id == 0 {
            return Err(Error::new("Universe ID must be positive"));
        }
        std::thread::scope(|s| {
            let h_cfg = s.spawn(|| {
                self.get_json(
                    &format!("{}/v2/universes/{universe_id}/configuration", self.ep.develop),
                    "develop.roblox.com/v2/universes/{id}/configuration",
                )
                .or_else(|e| {
                    // Fallback to v1 configuration if v2 isn't permitted/available
                    self.get_json(
                        &format!("{}/v1/universes/{universe_id}/configuration", self.ep.develop),
                        "develop.roblox.com/v1/universes/{id}/configuration",
                    )
                    .map_err(|_| e)
                })
            });
            let h_univ = s.spawn(|| {
                self.get_json(
                    &format!("{}/v1/universes/{universe_id}", self.ep.develop),
                    "develop.roblox.com/v1/universes/{id}",
                )
                .ok()
            });
            let h_icon = s.spawn(|| self.fetch_experience_icon(universe_id));
            let h_media = s.spawn(|| self.fetch_experience_thumbnails(universe_id));
            let h_guide = s.spawn(|| self.fetch_age_guidelines(universe_id));
            let h_genre = s.spawn(|| self.fetch_experience_genre(universe_id));
            let h_comm = s.spawn(|| self.fetch_communication_settings(universe_id));
            let h_amp = s.spawn(|| self.fetch_mesh_texture_amp_status());
            let h_mon = s.spawn(|| self.fetch_monetization_summary(universe_id));
            let h_tc = s.spawn(|| self.team_create(universe_id));

            let cfg_res = h_cfg.join().unwrap_or_else(|_| Err(Error::new("config worker panicked")));
            let univ_val = h_univ.join().unwrap_or(None);
            let (can_manage_config, config) = match cfg_res {
                Ok(cfg_val) => (true, parse_configuration(universe_id, &cfg_val, univ_val.as_ref())),
                Err(err) => {
                    // Collaborators without `canManage` receive HTTP 403 on
                    // `/configuration`. Match Studio's view-only fallback by
                    // reading public universe details (`games/v1/games` +
                    // `develop/v1/universes/{id}`) instead of failing the dialog.
                    let is_forbidden = err.0.contains("HTTP 403") || err.0.contains("HTTP 401");
                    let public_game = self
                        .get_json(
                            &format!("{}/v1/games?universeIds={universe_id}", self.ep.games),
                            "games.roblox.com/v1/games",
                        )
                        .ok()
                        .and_then(|v| v.pointer("/data/0").cloned());
                    if is_forbidden && (univ_val.is_some() || public_game.is_some()) {
                        let fallback = public_game
                            .as_ref()
                            .or(univ_val.as_ref())
                            .cloned()
                            .unwrap_or_default();
                        let mut cfg = parse_configuration(universe_id, &fallback, univ_val.as_ref());
                        if let Some(pg) = &public_game {
                            if cfg.name.is_empty() {
                                cfg.name = str_field(pg, "name");
                            }
                            if cfg.description.is_empty() {
                                cfg.description = str_field(pg, "description");
                            }
                            if cfg.genre.is_empty() {
                                cfg.genre = str_field(pg, "genre");
                            }
                            if cfg.price.is_none() {
                                cfg.price = u64_field(pg, "price");
                                cfg.is_for_sale = cfg.price.is_some_and(|p| p > 0);
                            }
                        }
                        (false, cfg)
                    } else {
                        return Err(err);
                    }
                }
            };
            let effective_root = if root_place_id != 0 { root_place_id } else { config.root_place_id };
            let has_published_version = if effective_root != 0 {
                self.has_published_version(effective_root)
            } else {
                false
            };

            Ok(GameSettingsSnapshot {
                can_manage_config,
                config,
                icon: h_icon.join().unwrap_or_default(),
                thumbnails: h_media.join().unwrap_or_default(),
                guidelines: h_guide.join().unwrap_or(None),
                experience_genre: h_genre.join().unwrap_or(None),
                communication: h_comm.join().unwrap_or_default(),
                mesh_texture_amp: h_amp.join().unwrap_or_default(),
                monetization: h_mon.join().unwrap_or_default(),
                has_published_version,
                team_create: h_tc.join().unwrap_or_default(),
            })
        })
    }

    pub fn fetch_experience_icon(&self, universe_id: u64) -> ExperienceIcon {
        let Ok(v) = self.get_json(
            &format!("{}/v1/universes/{universe_id}/icon", self.ep.develop),
            "universe icon",
        ) else {
            return ExperienceIcon::default();
        };
        ExperienceIcon {
            image_id: u64_field(&v, "imageId").filter(|&id| id != 0),
            is_approved: bool_field(&v, "isApproved"),
        }
    }

    pub fn fetch_experience_thumbnails(&self, universe_id: u64) -> Vec<ExperienceThumbnail> {
        let Ok(v) = self.get_json(
            &format!("{}/v1/games/{universe_id}/media", self.ep.games),
            "universe media",
        ) else {
            return Vec::new();
        };
        v.get("data")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(|item| {
                        let id = u64_field(item, "id")?;
                        Some(ExperienceThumbnail {
                            id,
                            image_id: u64_field(item, "imageId").filter(|&x| x != 0),
                            asset_type: str_field(item, "assetType"),
                            approved: bool_field(item, "approved"),
                            alt_text: str_field(item, "altText"),
                            video_hash: item.get("videoHash").and_then(Value::as_str).map(str::to_string),
                            video_title: item.get("videoTitle").and_then(Value::as_str).map(str::to_string),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn fetch_age_guidelines(&self, universe_id: u64) -> Option<AgeGuidelines> {
        let url = format!(
            "{}/experience-guidelines-service/v1beta1/multi-age-recommendation",
            self.ep.apis
        );
        let body = json!({ "universeIds": [universe_id] }).to_string();
        let (status, text) = self
            .session
            .post_csrf(&url, "application/json", body.as_bytes())
            .ok()?;
        if !(200..300).contains(&status) {
            return None;
        }
        let v = parse_json(&text)?;
        let entry = v
            .get("ageRecommendationDetailsByUniverse")
            .and_then(Value::as_array)?
            .iter()
            .find(|e| e.get("universeId").and_then(Value::as_u64) == Some(universe_id))
            .or_else(|| {
                v.get("ageRecommendationDetailsByUniverse")
                    .and_then(Value::as_array)
                    .and_then(|a| a.first())
            })?;
        let rec = entry
            .pointer("/ageRecommendationDetails/ageRecommendationSummary/ageRecommendation")?;
        let content_maturity = rec
            .get("contentMaturity")
            .and_then(Value::as_str)
            .unwrap_or("unrated")
            .to_string();
        let display_name = rec
            .get("displayName")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        Some(AgeGuidelines { content_maturity, display_name })
    }

    pub fn fetch_experience_genre(&self, universe_id: u64) -> Option<String> {
        let url = format!(
            "{}/experience-genre-api/v1/Creator/ExperienceGenre?universeId={universe_id}&genreTaxonomyVersion=1",
            self.ep.apis
        );
        let v = self.get_json(&url, "experience genre").ok()?;
        v.get("genre")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    }

    pub fn has_published_version(&self, place_id: u64) -> bool {
        let Ok(v) = self.get_json(
            &format!("{}/v1/assets/{place_id}/published-versions", self.ep.develop),
            "published versions",
        ) else {
            return false;
        };
        v.get("data")
            .and_then(Value::as_array)
            .is_some_and(|a| !a.is_empty())
    }

    pub fn fetch_communication_settings(&self, universe_id: u64) -> CommunicationSettings {
        let mut out = CommunicationSettings::default();
        if let Ok(v) = self.get_json(&format!("{}/v1/settings", self.ep.voice), "voice user settings") {
            out.user_avatar_chat_enabled = bool_field(&v, "isAvatarChatEnabled");
        }
        if let Ok(v) = self.get_json(
            &format!("{}/v1/settings/universe/{universe_id}", self.ep.voice),
            "voice universe settings",
        ) {
            out.audio_enabled = bool_field(&v, "isUniverseEnabledForVoice");
            out.video_enabled = bool_field(&v, "isUniverseEnabledForAvatarVideo");
            out.chat_group_api_enabled = bool_field(&v, "isChatGroupsApiEnabled");
        }
        out
    }

    pub fn fetch_mesh_texture_amp_status(&self) -> MeshTextureAmpStatus {
        let url = format!(
            "{}/access-management/v1/feature-access?featureNames=CanEnableMeshTextureApi",
            self.ep.apis
        );
        let Ok(v) = self.get_json(&url, "feature access") else {
            return MeshTextureAmpStatus::Unknown;
        };
        v.pointer("/features/0/access")
            .and_then(Value::as_str)
            .map(MeshTextureAmpStatus::parse)
            .unwrap_or(MeshTextureAmpStatus::Unknown)
    }

    pub fn fetch_monetization_summary(&self, universe_id: u64) -> MonetizationSummary {
        let mut out = MonetizationSummary { tax_rate: 0.3, ..Default::default() };
        if let Ok(v) = self.get_json(&format!("{}/v1/resale-tax-rate", self.ep.economy), "resale tax rate") {
            if let Some(r) = v.get("taxRate").and_then(Value::as_f64) {
                out.tax_rate = r;
            }
            if let Some(m) = u64_field(&v, "minimumFee") {
                out.minimum_fee = m;
            }
        }
        if let Ok(v) = self.get_json(
            &format!("{}/v1/universes/{universe_id}/configuration/vip-servers", self.ep.develop),
            "vip servers",
        ) {
            out.active_vip_servers = u64_field(&v, "activeServersCount").unwrap_or(0);
            out.active_vip_subscriptions = u64_field(&v, "activeSubscriptionsCount").unwrap_or(0);
        }
        if let Ok(v) = self.get_json(
            &format!(
                "{}/developer-ads-stats-api/v1/universe-suitability-criteria/{universe_id}",
                self.ep.apis
            ),
            "rewarded ads eligibility",
        ) {
            out.rewarded_ads_eligible = v.get("isUniverseEligible").and_then(Value::as_bool);
        }
        if let Ok(v) = self.get_json(
            &format!("{}/developer-products/v2/universes/{universe_id}/developerproducts", self.ep.apis),
            "developer products",
        ) {
            if let Some(arr) = v.get("developerProducts").and_then(Value::as_array) {
                out.dev_products = arr
                    .iter()
                    .filter_map(|p| {
                        let id = p.get("id").or_else(|| p.get("ProductId")).and_then(Value::as_u64)?;
                        let product_id = p.get("ProductId").or_else(|| p.get("productId")).and_then(Value::as_u64).unwrap_or(id);
                        let name = p
                            .get("Name")
                            .or_else(|| p.get("name"))
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_string();
                        let price = p
                            .get("PriceInRobux")
                            .or_else(|| p.get("priceInRobux"))
                            .and_then(Value::as_u64)
                            .unwrap_or(0);
                        let description = p
                            .get("Description")
                            .or_else(|| p.get("description"))
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_string();
                        Some(DeveloperProduct {
                            id,
                            product_id,
                            name,
                            price_in_robux: price,
                            description,
                        })
                    })
                    .collect();
            }
        }
        if let Ok(v) = self.get_json(
            &format!("{}/v1/universes/{universe_id}/badges?limit=50", self.ep.badges),
            "badges",
        ) {
            if let Some(arr) = v.get("data").and_then(Value::as_array) {
                out.badges = arr
                    .iter()
                    .filter_map(|b| {
                        Some(ExperienceBadge {
                            id: u64_field(b, "id")?,
                            name: str_field(b, "name"),
                            description: str_field(b, "description"),
                            enabled: b.get("enabled").and_then(Value::as_bool).unwrap_or(true),
                            icon_image_id: u64_field(b, "iconImageId").unwrap_or(0),
                        })
                    })
                    .collect();
            }
        }
        out
    }

    /// `PATCH develop/v2/universes/{id}/configuration` with only changed fields.
    /// Parses Studio's moderation codes (`NameModerated`, `DescriptionModerated`)
    /// and VIP server price-change cooldown messages.
    pub fn patch_universe_configuration(
        &self,
        universe_id: u64,
        patch: &ConfigurationPatch,
    ) -> std::result::Result<(), ConfigSaveError> {
        if patch.is_empty() {
            return Ok(());
        }
        let url = format!("{}/v2/universes/{universe_id}/configuration", self.ep.develop);
        let body = patch.to_json().to_string();
        let (status, text) = self
            .session
            .send_csrf("PATCH", &url, "application/json", body.as_bytes())
            .map_err(|e| ConfigSaveError {
                message: format!("Update experience configuration failed: {e}"),
                name_moderated: false,
                description_moderated: false,
                vip_price_cooldown: None,
            })?;
        if (200..300).contains(&status) {
            return Ok(());
        }
        let mut name_moderated = false;
        let mut description_moderated = false;
        let mut vip_price_cooldown = None;
        if let Some(v) = parse_json(&text) {
            if let Some(errs) = v.get("errors").and_then(Value::as_array) {
                for err in errs {
                    let code = err.get("code").and_then(Value::as_i64).unwrap_or(0);
                    let msg = err.get("message").and_then(Value::as_str).unwrap_or("");
                    let user_msg = err.get("userFacingMessage").and_then(Value::as_str).unwrap_or("");
                    // Roblox develop v2 error codes: 7 = NameModerated, 8 = DescriptionModerated
                    if code == 7 || msg.to_ascii_lowercase().contains("name") && msg.to_ascii_lowercase().contains("moderat") {
                        name_moderated = true;
                    }
                    if code == 8 || msg.to_ascii_lowercase().contains("description") && msg.to_ascii_lowercase().contains("moderat") {
                        description_moderated = true;
                    }
                    if user_msg.contains("UTC") || msg.contains("UTC") {
                        vip_price_cooldown = Some(if !user_msg.is_empty() { user_msg } else { msg }.to_string());
                    }
                }
            }
        }
        let why = error_message(&text).map(|m| format!(": {m}")).unwrap_or_default();
        Err(ConfigSaveError {
            message: format!("Update experience configuration returned HTTP {status}{why}"),
            name_moderated,
            description_moderated,
            vip_price_cooldown,
        })
    }

    /// `POST develop/v1/universes/{id}/activate` or `/deactivate`
    /// (`GamePermissionsController`).
    pub fn set_universe_active(&self, universe_id: u64, active: bool) -> Result<()> {
        let verb = if active { "activate" } else { "deactivate" };
        let url = format!("{}/v1/universes/{universe_id}/{verb}", self.ep.develop);
        let (status, text) = self.session.post_csrf(&url, "application/json", b"{}")?;
        if !(200..300).contains(&status) {
            let why = error_message(&text).map(|m| format!(": {m}")).unwrap_or_default();
            return Err(Error::new(format!("Set experience playability ({verb}) returned HTTP {status}{why}")));
        }
        Ok(())
    }

    /// Updates changed Communication flags on `voice.roblox.com`
    /// (`CommunicationController`).
    pub fn set_communication_settings(
        &self,
        universe_id: u64,
        audio: Option<bool>,
        video: Option<bool>,
        chat_group_api: Option<bool>,
    ) -> Result<()> {
        let calls: [(&str, String, Option<bool>); 3] = [
            ("Microphone", format!("{}/v1/settings/universe/{universe_id}", self.ep.voice), audio),
            ("Camera", format!("{}/v1/settings/universe/avatarvideo/{universe_id}", self.ep.voice), video),
            (
                "Chat Group API",
                format!("{}/v1/settings/universe/chat-group-api/{universe_id}", self.ep.voice),
                chat_group_api,
            ),
        ];
        for (label, url, opt_in) in calls {
            let Some(opt_in) = opt_in else { continue };
            let body = json!({ "optIn": opt_in }).to_string();
            let (status, text) = self.session.post_csrf(&url, "application/json", body.as_bytes())?;
            if !(200..300).contains(&status) {
                let why = error_message(&text).map(|m| format!(": {m}")).unwrap_or_default();
                return Err(Error::new(format!("Update {label} returned HTTP {status}{why}")));
            }
        }
        Ok(())
    }

    /// Uploads a new experience icon via `POST publish/v1/games/{id}/icon`.
    pub fn upload_universe_icon(&self, universe_id: u64, filename: &str, bytes: &[u8]) -> Result<u64> {
        let (ct, body) = build_publish_image_multipart(filename, bytes);
        let url = format!("{}/v1/games/{universe_id}/icon", self.ep.publish);
        let (status, text) = self.session.post_csrf(&url, &ct, &body)?;
        if !(200..300).contains(&status) {
            let why = error_message(&text).map(|m| format!(": {m}")).unwrap_or_default();
            return Err(Error::new(format!("Upload experience icon returned HTTP {status}{why}")));
        }
        let target_id = parse_json(&text)
            .and_then(|v| u64_field(&v, "targetId").or_else(|| u64_field(&v, "imageId")))
            .unwrap_or(0);
        Ok(target_id)
    }

    /// Uploads a new screenshot thumbnail via `POST publish/v1/games/{id}/thumbnail/image`.
    pub fn upload_universe_thumbnail(&self, universe_id: u64, filename: &str, bytes: &[u8]) -> Result<u64> {
        let (ct, body) = build_publish_image_multipart(filename, bytes);
        let url = format!("{}/v1/games/{universe_id}/thumbnail/image", self.ep.publish);
        let (status, text) = self.session.post_csrf(&url, &ct, &body)?;
        if !(200..300).contains(&status) {
            let why = error_message(&text).map(|m| format!(": {m}")).unwrap_or_default();
            return Err(Error::new(format!("Upload screenshot returned HTTP {status}{why}")));
        }
        let target_id = parse_json(&text)
            .and_then(|v| u64_field(&v, "targetId"))
            .unwrap_or(0);
        Ok(target_id)
    }

    /// Deletes a screenshot thumbnail via `DELETE develop/v1/universes/{id}/thumbnails/{tid}`.
    pub fn delete_universe_thumbnail(&self, universe_id: u64, thumbnail_id: u64) -> Result<()> {
        let url = format!("{}/v1/universes/{universe_id}/thumbnails/{thumbnail_id}", self.ep.develop);
        let (status, text) = self.session.send_csrf("DELETE", &url, "application/json", b"")?;
        if !(200..300).contains(&status) {
            let why = error_message(&text).map(|m| format!(": {m}")).unwrap_or_default();
            return Err(Error::new(format!("Delete screenshot returned HTTP {status}{why}")));
        }
        Ok(())
    }

    /// Reorders thumbnails via `POST develop/v1/universes/{id}/thumbnails/order`.
    pub fn set_thumbnail_order(&self, universe_id: u64, thumbnail_ids: &[u64]) -> Result<()> {
        let url = format!("{}/v1/universes/{universe_id}/thumbnails/order", self.ep.develop);
        let body = json!({ "thumbnailIds": thumbnail_ids }).to_string();
        let (status, text) = self.session.post_csrf(&url, "application/json", body.as_bytes())?;
        if !(200..300).contains(&status) {
            let why = error_message(&text).map(|m| format!(": {m}")).unwrap_or_default();
            return Err(Error::new(format!("Reorder screenshots returned HTTP {status}{why}")));
        }
        Ok(())
    }

    /// Updates thumbnail alt text via `POST develop/v1/universes/{id}/thumbnails/alt-text`.
    pub fn set_thumbnail_alt_text(&self, universe_id: u64, thumbnail_id: u64, alt_text: &str) -> Result<()> {
        let url = format!("{}/v1/universes/{universe_id}/thumbnails/alt-text", self.ep.develop);
        let body = json!({ "thumbnailId": thumbnail_id, "altText": alt_text }).to_string();
        let (status, text) = self.session.post_csrf(&url, "application/json", body.as_bytes())?;
        if !(200..300).contains(&status) {
            let why = error_message(&text).map(|m| format!(": {m}")).unwrap_or_default();
            return Err(Error::new(format!("Update screenshot alt text returned HTTP {status}{why}")));
        }
        Ok(())
    }

    /// Creates a Developer Product (`POST apis/developer-products/v1/universes/{id}/developerproducts`).
    pub fn create_developer_product(
        &self,
        universe_id: u64,
        name: &str,
        price_in_robux: u64,
        description: &str,
    ) -> Result<DeveloperProduct> {
        let url = format!(
            "{}/developer-products/v1/universes/{universe_id}/developerproducts",
            self.ep.apis
        );
        let body = json!({
            "name": name,
            "price": price_in_robux,
            "priceInRobux": price_in_robux,
            "description": description,
        })
        .to_string();
        let (status, text) = self.session.post_csrf(&url, "application/json", body.as_bytes())?;
        if !(200..300).contains(&status) {
            let why = error_message(&text).map(|m| format!(": {m}")).unwrap_or_default();
            return Err(Error::new(format!("Create developer product returned HTTP {status}{why}")));
        }
        let v = parse_json(&text).unwrap_or_default();
        let id = v.get("id").or_else(|| v.get("productId")).and_then(Value::as_u64).unwrap_or(0);
        let product_id = v.get("productId").or_else(|| v.get("ProductId")).and_then(Value::as_u64).unwrap_or(id);
        Ok(DeveloperProduct {
            id,
            product_id,
            name: name.to_string(),
            price_in_robux,
            description: description.to_string(),
        })
    }

    /// Updates a Developer Product (`POST apis/developer-products/v1/universes/{id}/developerproducts/{pid}/update`).
    pub fn update_developer_product(
        &self,
        universe_id: u64,
        product_id: u64,
        name: &str,
        price_in_robux: u64,
        description: &str,
    ) -> Result<()> {
        let url = format!(
            "{}/developer-products/v1/universes/{universe_id}/developerproducts/{product_id}/update",
            self.ep.apis
        );
        let body = json!({
            "Name": name,
            "PriceInRobux": price_in_robux,
            "Description": description,
        })
        .to_string();
        let (status, text) = self.session.post_csrf(&url, "application/json", body.as_bytes())?;
        if !(200..300).contains(&status) {
            let why = error_message(&text).map(|m| format!(": {m}")).unwrap_or_default();
            return Err(Error::new(format!("Update developer product returned HTTP {status}{why}")));
        }
        Ok(())
    }

    /// Downloads an asset thumbnail PNG (used for Experience Icon & Screenshots preview).
    pub fn fetch_asset_thumbnail_bytes(&self, asset_id: u64, size: &str) -> Result<Vec<u8>> {
        let url = format!(
            "{}/v1/assets?assetIds={asset_id}&returnPolicy=PlaceHolder&size={size}&format=Png&isCircular=false",
            self.ep.thumbnails
        );
        let v = self.get_json(&url, "asset thumbnail")?;
        let image_url = v
            .pointer("/data/0/imageUrl")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| Error::new("Thumbnail image URL missing"))?;
        let (status, bytes) = self.session.fetch_bytes(image_url, false)?;
        if status != 200 {
            return Err(Error::new(format!("Thumbnail download returned HTTP {status}")));
        }
        Ok(bytes)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::cloud::Endpoints;
    use crate::mock::{MockReply, MockRequest, MockServer};
    use crate::session::{Cookie, Profile, Session};

    fn test_api(srv: &MockServer) -> CloudApi {
        let session = Session::unchecked(
            Cookie::new("test-cookie").unwrap(),
            Profile { id: 42, name: "Builder".into(), display_name: "Builder".into() },
        );
        CloudApi::new(session, Endpoints::mock(&srv.base), None)
    }

    #[test]
    fn fetch_game_settings_fans_out_across_all_studio_endpoints() {
        let srv = MockServer::start(Arc::new(|req: &MockRequest| match (req.method.as_str(), req.path.as_str()) {
            ("GET", "/develop/v2/universes/777/configuration") => MockReply::json(
                200,
                r#"{
                    "name": "Galaxy Obby",
                    "description": "Jump across stars",
                    "genre": "Adventure",
                    "playableDevices": ["Computer", "Phone", "Tablet"],
                    "isFriendsOnly": false,
                    "isForSale": true,
                    "price": 25,
                    "allowPrivateServers": true,
                    "privateServerPrice": 100,
                    "studioAccessToApisAllowed": true,
                    "permissions": {
                        "IsThirdPartyPurchaseAllowed": true,
                        "IsThirdPartyTeleportAllowed": false
                    },
                    "isMeshTextureApiAccessAllowed": true,
                    "isRewardedOnDemandAdsAllowed": false,
                    "universeAvatarType": "MorphToR15",
                    "optInRegions": []
                }"#,
            ),
            ("GET", "/develop/v1/universes/777") => MockReply::json(
                200,
                r#"{
                    "id": 777,
                    "rootPlaceId": 888,
                    "creatorType": "User",
                    "creatorTargetId": 42,
                    "creatorName": "Builder",
                    "isActive": true
                }"#,
            ),
            ("GET", "/develop/v1/universes/777/icon") => {
                MockReply::json(200, r#"{"imageId": 555001, "isApproved": true}"#)
            }
            ("GET", "/games/v1/games/777/media") => MockReply::json(
                200,
                r#"{"data":[{"id":9001,"assetType":"Image","imageId":555002,"approved":true,"altText":"Spawn area"}]}"#,
            ),
            ("POST", "/apis/experience-guidelines-service/v1beta1/multi-age-recommendation") => {
                MockReply::json(
                    200,
                    r#"{"ageRecommendationDetailsByUniverse":[{"universeId":777,"ageRecommendationDetails":{"ageRecommendationSummary":{"ageRecommendation":{"contentMaturity":"minimal","displayName":"Minimal"}}}}]}"#,
                )
            }
            ("GET", p) if p.starts_with("/apis/experience-genre-api/v1/Creator/ExperienceGenre") => {
                MockReply::json(200, r#"{"genre":"game.obby_and_platformer.classic_obby"}"#)
            }
            ("GET", "/voice/v1/settings") => MockReply::json(200, r#"{"isAvatarChatEnabled": true}"#),
            ("GET", "/voice/v1/settings/universe/777") => MockReply::json(
                200,
                r#"{"isUniverseEnabledForVoice":true,"isUniverseEnabledForAvatarVideo":false,"isChatGroupsApiEnabled":true}"#,
            ),
            ("GET", p) if p.starts_with("/apis/access-management/v1/feature-access") => {
                MockReply::json(200, r#"{"features":[{"featureName":"CanEnableMeshTextureApi","access":"Granted"}]}"#)
            }
            ("GET", "/economy/v1/resale-tax-rate") => {
                MockReply::json(200, r#"{"taxRate":0.3,"minimumFee":1}"#)
            }
            ("GET", "/develop/v1/universes/777/configuration/vip-servers") => {
                MockReply::json(200, r#"{"activeServersCount":3,"activeSubscriptionsCount":5}"#)
            }
            ("GET", "/apis/developer-ads-stats-api/v1/universe-suitability-criteria/777") => {
                MockReply::json(200, r#"{"isUniverseEligible":true}"#)
            }
            ("GET", "/apis/developer-products/v2/universes/777/developerproducts") => MockReply::json(
                200,
                r#"{"developerProducts":[{"id":11,"ProductId":22,"Name":"2x Coins","PriceInRobux":50,"Description":"Double coins"}]}"#,
            ),
            ("GET", p) if p.starts_with("/badges/v1/universes/777/badges") => MockReply::json(
                200,
                r#"{"data":[{"id":301,"name":"Welcome","description":"Joined once","enabled":true,"iconImageId":401}]}"#,
            ),
            ("GET", "/develop/v1/assets/888/published-versions") => {
                MockReply::json(200, r#"{"data":[{"assetVersionNumber":12}]}"#)
            }
            ("GET", p) if p.starts_with("/develop/v1/universes/multiget/teamcreate") => {
                MockReply::json(200, r#"{"data":[{"id":777,"isEnabled":true}]}"#)
            }
            ("POST", "/apis/team-create-service/v1/GetCanCollaborate") => {
                MockReply::json(200, r#"{"CanCollaborate":true,"Error":"None"}"#)
            }
            ("GET", "/develop/v1/universes/777/permissions") => {
                MockReply::json(200, r#"{"canManage":true,"canCloudEdit":true}"#)
            }
            _ => MockReply::json(404, "{}"),
        }))
        .expect("mock server");

        let api = test_api(&srv);
        let snap = api.fetch_game_settings(777, 888).expect("fetch succeeded");
        assert!(snap.can_manage_config);
        assert_eq!(snap.config.name, "Galaxy Obby");
        assert_eq!(snap.config.description, "Jump across stars");
        assert_eq!(snap.config.playable_devices, vec!["Computer", "Phone", "Tablet"]);
        assert_eq!(snap.config.playability(), Playability::Public);
        assert!(snap.config.is_for_sale);
        assert_eq!(snap.config.price, Some(25));
        assert!(snap.config.allow_private_servers);
        assert_eq!(snap.config.private_server_price, Some(100));
        assert!(snap.config.studio_access_to_apis_allowed);
        assert!(snap.config.allow_third_party_sales);
        assert!(!snap.config.allow_third_party_teleports);
        assert!(snap.config.allow_mesh_texture_apis);
        assert_eq!(snap.icon.image_id, Some(555001));
        assert!(snap.icon.is_approved);
        assert_eq!(snap.thumbnails.len(), 1);
        assert_eq!(snap.thumbnails[0].alt_text, "Spawn area");
        assert_eq!(snap.guidelines.as_ref().unwrap().display_name, "Minimal");
        assert_eq!(
            format_genre_slug(snap.experience_genre.as_deref().unwrap()),
            "Game > Obby & Platformer > Classic Obby"
        );
        assert!(snap.communication.user_avatar_chat_enabled);
        assert!(snap.communication.audio_enabled);
        assert!(!snap.communication.video_enabled);
        assert!(snap.communication.chat_group_api_enabled);
        assert_eq!(snap.mesh_texture_amp, MeshTextureAmpStatus::Granted);
        assert_eq!(snap.monetization.active_vip_servers, 3);
        assert_eq!(snap.monetization.dev_products.len(), 1);
        assert_eq!(snap.monetization.dev_products[0].name, "2x Coins");
        assert_eq!(snap.monetization.badges.len(), 1);
        assert!(snap.has_published_version);
        assert_eq!(snap.team_create.enabled, Some(true));
    }

    #[test]
    fn patch_configuration_sends_minimal_diff_and_parses_moderation_errors() {
        let srv = MockServer::start(Arc::new(|req: &MockRequest| match (req.method.as_str(), req.path.as_str()) {
            ("PATCH", "/develop/v2/universes/10/configuration") => {
                let body: Value = serde_json::from_slice(&req.body).unwrap();
                if body.get("name").and_then(Value::as_str) == Some("BadName") {
                    MockReply::json(400, r#"{"errors":[{"code":7,"message":"Name was moderated"}]}"#)
                } else {
                    assert_eq!(body.as_object().unwrap().len(), 2);
                    assert_eq!(body["name"], "GoodName");
                    assert_eq!(body["permissions"]["IsThirdPartyPurchaseAllowed"], true);
                    MockReply::json(200, "{}")
                }
            }
            _ => MockReply::json(404, "{}"),
        }))
        .expect("mock server");
        let api = test_api(&srv);

        let err = api
            .patch_universe_configuration(
                10,
                &ConfigurationPatch { name: Some("BadName".into()), ..Default::default() },
            )
            .unwrap_err();
        assert!(err.name_moderated);
        assert!(!err.description_moderated);

        api.patch_universe_configuration(
            10,
            &ConfigurationPatch {
                name: Some("GoodName".into()),
                allow_third_party_sales: Some(true),
                ..Default::default()
            },
        )
        .expect("patch ok");
    }

    #[test]
    fn communication_playability_media_and_dev_product_writes() {
        let srv = MockServer::start(Arc::new(|req: &MockRequest| match (req.method.as_str(), req.path.as_str()) {
            ("POST", "/develop/v1/universes/55/activate") => MockReply::json(200, "{}"),
            ("POST", "/develop/v1/universes/55/deactivate") => MockReply::json(200, "{}"),
            ("POST", "/voice/v1/settings/universe/55") => {
                let v: Value = serde_json::from_slice(&req.body).unwrap();
                assert_eq!(v["optIn"], true);
                MockReply::json(200, "{}")
            }
            ("POST", "/voice/v1/settings/universe/avatarvideo/55") => {
                let v: Value = serde_json::from_slice(&req.body).unwrap();
                assert_eq!(v["optIn"], false);
                MockReply::json(200, "{}")
            }
            ("POST", "/publish/v1/games/55/icon") => {
                let body = String::from_utf8_lossy(&req.body);
                assert!(body.contains("name=\"request.files\""));
                assert!(body.contains("Content-Type: image/png"));
                MockReply::json(200, r#"{"targetId": 12345}"#)
            }
            ("POST", "/publish/v1/games/55/thumbnail/image") => {
                MockReply::json(200, r#"{"targetId": 67890}"#)
            }
            ("DELETE", "/develop/v1/universes/55/thumbnails/67890") => MockReply::json(200, "{}"),
            ("POST", "/develop/v1/universes/55/thumbnails/order") => {
                let v: Value = serde_json::from_slice(&req.body).unwrap();
                assert_eq!(v["thumbnailIds"], json!([2, 1]));
                MockReply::json(200, "{}")
            }
            ("POST", "/develop/v1/universes/55/thumbnails/alt-text") => MockReply::json(200, "{}"),
            ("POST", "/apis/developer-products/v1/universes/55/developerproducts") => {
                MockReply::json(200, r#"{"id": 99, "productId": 199}"#)
            }
            ("POST", "/apis/developer-products/v1/universes/55/developerproducts/199/update") => {
                MockReply::json(200, "{}")
            }
            _ => MockReply::json(404, "{}"),
        }))
        .expect("mock server");
        let api = test_api(&srv);

        api.set_universe_active(55, true).unwrap();
        api.set_universe_active(55, false).unwrap();
        api.set_communication_settings(55, Some(true), Some(false), None).unwrap();
        assert_eq!(api.upload_universe_icon(55, "icon.png", b"\x89PNG").unwrap(), 12345);
        assert_eq!(api.upload_universe_thumbnail(55, "shot.png", b"\x89PNG").unwrap(), 67890);
        api.set_thumbnail_order(55, &[2, 1]).unwrap();
        api.set_thumbnail_alt_text(55, 67890, "Lobby").unwrap();
        api.delete_universe_thumbnail(55, 67890).unwrap();
        let prod = api.create_developer_product(55, "VIP", 100, "Perks").unwrap();
        assert_eq!(prod.product_id, 199);
        api.update_developer_product(55, 199, "VIP+", 150, "More perks").unwrap();
    }

    #[test]
    fn fetch_game_settings_falls_back_to_view_only_on_403_collaborator() {
        let srv = MockServer::start(Arc::new(|req: &MockRequest| match (req.method.as_str(), req.path.as_str()) {
            ("GET", "/develop/v2/universes/88/configuration")
            | ("GET", "/develop/v1/universes/88/configuration") => {
                MockReply::json(403, r#"{"errors":[{"code":0,"message":"Not allowed"}]}"#)
            }
            ("GET", "/develop/v1/universes/88") => MockReply::json(
                200,
                r#"{"id":88,"rootPlaceId":99,"creatorType":"Group","creatorTargetId":7,"creatorName":"Studio Team","isActive":true}"#,
            ),
            ("GET", "/games/v1/games?universeIds=88") => MockReply::json(
                200,
                r#"{"data":[{"id":88,"name":"Shared Group Game","description":"Collab world","genre":"All","price":null}]}"#,
            ),
            ("GET", "/develop/v1/universes/88/icon") => {
                MockReply::json(200, r#"{"imageId":1234,"isApproved":true}"#)
            }
            _ => MockReply::json(404, "{}"),
        }))
        .expect("mock server");
        let api = test_api(&srv);
        let snap = api.fetch_game_settings(88, 99).expect("view-only fallback ok");
        assert!(!snap.can_manage_config);
        assert_eq!(snap.config.name, "Shared Group Game");
        assert_eq!(snap.config.description, "Collab world");
        assert_eq!(snap.config.playability(), Playability::Public);
        assert_eq!(snap.icon.image_id, Some(1234));
    }
}
