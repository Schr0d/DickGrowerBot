use std::collections::HashMap;
use std::error::Error;
use std::fmt::Display;
use std::ops::Not;
use std::str::FromStr;
use std::sync::{Arc, RwLock};
use anyhow::anyhow;
use reqwest::Url;
use sha2::{Digest, Sha256};
use sha2::digest::core_api::CoreWrapper;
use teloxide::types::Me;
use crate::domain::{LanguageCode, Ratio, SupportedLanguage};
use crate::domain::SupportedLanguage::{EN, RU};
use crate::handlers::perks::HelpPussiesPerk;
use crate::handlers::utils::Incrementor;
use crate::help;

const CACHED_ENV_TOGGLES_POISONED_MSG: &str = "CachedEnvToggles map was poisoned";

#[derive(Clone)]
#[cfg_attr(test, derive(Default))]
pub struct AppConfig {
    pub features: FeatureToggles,
    pub top_limit: u16,
    pub loan_payout_ratio: f32,
    pub dod_rich_exclusion_ratio: Option<Ratio>,
    pub announcements: AnnouncementsConfig,
    pub command_toggles: CachedEnvToggles,
}

#[derive(Clone)]
pub struct DatabaseConfig {
    pub url: Url,
    pub max_connections: u32
}

#[derive(Clone, Copy)]
pub struct FeatureToggles {
    pub chats_merging: bool,
    pub top_unlimited: bool,
    pub dod_selection_mode: DickOfDaySelectionMode,
    pub pvp: BattlesFeatureToggles,
}

#[cfg(test)]
impl Default for FeatureToggles {
    fn default() -> Self {
        Self {
            chats_merging: true,
            top_unlimited: true,
            dod_selection_mode: Default::default(),
            pvp: Default::default(),
        }
    }
}

#[derive(Copy, Clone, Default)]
pub struct BattlesFeatureToggles {
    pub check_acceptor_length: bool,
    pub callback_locks: bool,
    pub show_stats: bool,
    pub show_stats_notice: bool,
}

#[derive(Copy, Clone, Default, derive_more::FromStr, derive_more::Display)]
pub enum DickOfDaySelectionMode {
    WEIGHTS,
    EXCLUSION,
    #[default]
    RANDOM
}

impl AppConfig {
    pub fn from_env() -> Self {
        let top_limit = get_env_value_or_default("TOP_LIMIT", 10);
        let loan_payout_ratio = get_env_value_or_default("LOAN_PAYOUT_COEF", 0.0);
        let dod_selection_mode = get_optional_env_value("DOD_SELECTION_MODE");
        let dod_rich_exclusion_ratio = get_optional_env_ratio("DOD_RICH_EXCLUSION_RATIO");
        let chats_merging = get_env_value_or_default("CHATS_MERGING_ENABLED", false);
        let top_unlimited = get_env_value_or_default("TOP_UNLIMITED_ENABLED", false);
        let check_acceptor_length = get_env_value_or_default("PVP_CHECK_ACCEPTOR_LENGTH", false);
        let callback_locks = get_env_value_or_default("PVP_CALLBACK_LOCKS_ENABLED", true);
        let show_stats = get_env_value_or_default("PVP_STATS_SHOW", true);
        let show_stats_notice = get_env_value_or_default("PVP_STATS_SHOW_NOTICE", true);
        let announcement_max_shows = get_optional_env_value("ANNOUNCEMENT_MAX_SHOWS");
        let announcement_en = get_optional_env_value("ANNOUNCEMENT_EN");
        let announcement_ru = get_optional_env_value("ANNOUNCEMENT_RU");
        Self {
            features: FeatureToggles {
                chats_merging,
                top_unlimited,
                dod_selection_mode,
                pvp: BattlesFeatureToggles {
                    check_acceptor_length,
                    callback_locks,
                    show_stats,
                    show_stats_notice,
                }
            },
            top_limit,
            loan_payout_ratio,
            dod_rich_exclusion_ratio,
            announcements: AnnouncementsConfig {
                max_shows: announcement_max_shows,
                announcements: [
                    (EN, announcement_en),
                    (RU, announcement_ru),
                ].map(|(lc, text)| (lc, Announcement::new(text)))
                 .into_iter()
                 .filter_map(|(lc, mb_ann)| mb_ann.map(|ann| (lc, ann)))
                 .collect()
            },
            command_toggles: Default::default(),
        }
    }
}

impl DatabaseConfig {
    pub fn from_env() -> anyhow::Result<Self> {
        Ok(Self {
            url: get_env_mandatory_value("DATABASE_URL")?,
            max_connections: get_env_value_or_default("DATABASE_MAX_CONNECTIONS", 10)
        })
    }
}

#[derive(Clone, Default)]
pub struct CachedEnvToggles {
    map: Arc<RwLock<HashMap<String, bool>>>
}

impl CachedEnvToggles {
    pub fn enabled(&self, key: &str) -> bool {
        log::debug!("trying to take a read lock for key '{key}'...");
        let maybe_enabled = self.map.read().expect(CACHED_ENV_TOGGLES_POISONED_MSG).get(key).copied();
        // maybe_enabled is required to drop the read lock
        maybe_enabled.unwrap_or_else(|| {
            let enabled = Self::enabled_in_env(key);
            log::debug!("trying to take a write lock for key '{key}'...");
            self.map.write().expect(CACHED_ENV_TOGGLES_POISONED_MSG)
                .insert(key.to_owned(), enabled);
            enabled
        })
    }

    fn enabled_in_env(key: &str) -> bool {
        std::env::var_os(format!("DISABLE_CMD_{}", key.to_uppercase())).is_none()
    }
}

#[derive(Clone, Default)]
pub struct AnnouncementsConfig {
    pub max_shows: usize,
    pub announcements: HashMap<SupportedLanguage, Announcement>,
}

impl AnnouncementsConfig {
    pub fn get(&self, lang_code: &LanguageCode) -> Option<&Announcement> {
        self.announcements.get(&lang_code.to_supported_language())
    }
}

#[derive(Clone)]
pub struct Announcement {
    pub text: Arc<String>,
    pub hash: Arc<Vec<u8>>,
}

impl Announcement {
    fn new(text: String) -> Option<Self> {
        text.is_empty().not().then(|| Self  {
            hash: Arc::new(hash(&text)),
            text: Arc::new(text),
        })
    }
}

pub fn build_context_for_help_messages(me: Me, incr: &Incrementor, competitor_bots: &[&str]) -> anyhow::Result<help::Context> {
    let other_bots = competitor_bots
        .iter()
        .map(|username| ensure_starts_with_at_sign(username.to_string()))
        .collect::<Vec<String>>()
        .join(", ");
    let incr_cfg = incr.get_config();

    Ok(help::Context {
        bot_name: me.username().to_owned(),
        grow_min: incr_cfg.growth_range_min().to_string(),
        grow_max: incr_cfg.growth_range_max().to_string(),
        other_bots,
        admin_username: ensure_starts_with_at_sign(get_env_mandatory_value("HELP_ADMIN_USERNAME")?),
        admin_channel_ru: ensure_starts_with_at_sign(get_env_mandatory_value("HELP_ADMIN_CHANNEL_RU")?),
        admin_channel_en: ensure_starts_with_at_sign(get_env_mandatory_value("HELP_ADMIN_CHANNEL_EN")?),
        git_repo: get_env_mandatory_value("HELP_GIT_REPO")?,
        help_pussies_percentage: incr.find_perk_config::<HelpPussiesPerk>()
            .map(|payout_ratio| payout_ratio * 100.0)
            .unwrap_or(0.0)
    })
}

pub(crate) fn get_env_mandatory_value<T, E>(key: &str) -> anyhow::Result<T>
where
    T: FromStr<Err = E>,
    E: Error + Send + Sync + 'static
{
    std::env::var(key)?
        .parse()
        .map_err(|e: E| anyhow!(e))
}

pub(crate) fn get_env_value_or_default<T, E>(key: &str, default: T) -> T
where
    T: FromStr<Err = E> + Display,
    E: Error + Send + Sync + 'static
{
    std::env::var(key)
        .map_err(|e| {
            log::warn!("no value was found for an optional environment variable {key}, using the default value {default}");
            anyhow!(e)
        })
        .and_then(|v| v.parse()
            .map_err(|e: E| {
                log::warn!("invalid value of the {key} environment variable, using the default value {default}");
                anyhow!(e)
            }))
        .unwrap_or(default)
}

fn get_optional_env_value<T>(key: &str) -> T
where
    T: Default + FromStr + Display,
    <T as FromStr>::Err: Error + Send + Sync + 'static
{
    get_env_value_or_default(key, T::default())
}

fn get_optional_env_ratio(key: &str) -> Option<Ratio> {
    let value = get_env_value_or_default(key, -1.0);
    Ratio::new(value)
        .inspect_err(|_| log::warn!("{key} is disabled due to the invalid value: {value}"))
        .ok()
}

fn ensure_starts_with_at_sign(s: String) -> String {
    if s.starts_with('@') {
        s
    } else {
        format!("@{s}")
    }
}

fn hash(s: &str) -> Vec<u8> {
    let mut hasher = Sha256::new();
    CoreWrapper::update(&mut hasher, s.as_bytes());
    (*hasher.finalize()).to_vec()
}

#[cfg(test)]
mod test {
    use super::ensure_starts_with_at_sign;

    #[test]
    fn test_ensure_starts_with_at_sign() {
        let result = "@test";
        assert_eq!(ensure_starts_with_at_sign("test".to_owned()), result);
        assert_eq!(ensure_starts_with_at_sign("@test".to_owned()), result);
    }
}
