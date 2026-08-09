use std::{collections::BTreeMap, path::PathBuf};

use crate::style::{Color, Modifier, Style};
use crate::{Bindings, Res, error::Error, key_parser, menu::Menu, ops::Op};
use crossterm::event::{KeyCode, KeyModifiers};
use etcetera::{BaseStrategy, choose_base_strategy};
use figment::{
    Figment,
    providers::{Format, Toml},
};
use serde::Deserialize;

const DEFAULT_CONFIG: &str = include_str!("default_config.toml");

pub struct Config {
    pub general: GeneralConfig,
    pub style: StyleConfig,
    pub ai: AiConfig,
    pub bindings: Bindings,
    pub picker_bindings: PickerBindings,
}

#[derive(Default, Deserialize)]
pub(crate) struct PickerBindingsConfig {
    #[serde(default)]
    pub next: Vec<String>,
    #[serde(default)]
    pub previous: Vec<String>,
    #[serde(default)]
    pub done: Vec<String>,
    #[serde(default)]
    pub cancel: Vec<String>,
}

#[derive(Default, Deserialize)]
pub(crate) struct BindingsConfig {
    #[serde(flatten)]
    pub menus: BTreeMap<Menu, BTreeMap<Op, Vec<String>>>,
    #[serde(default)]
    pub picker: PickerBindingsConfig,
}

#[derive(Default, Deserialize)]
/// Only used to deserialise configurations with `figment`. This should be
/// parsed to be turned into a useful [`Config`].
pub(crate) struct FigmentConfig {
    pub general: GeneralConfig,
    pub style: StyleConfig,
    #[serde(default)]
    pub ai: AiConfig,
    pub bindings: BindingsConfig,
}

#[derive(Default, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AiBackend {
    /// Call an OpenAI-compatible chat-completions endpoint over HTTP.
    #[default]
    Api,
    /// Run a local command (e.g. the `claude` or `codex` CLI) that reads the
    /// diff on stdin and prints the commit message on stdout.
    Command,
}

#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct AiConfig {
    /// Whether the AI commit-message action is available.
    pub enabled: bool,
    /// Which backend generates the message.
    pub backend: AiBackend,
    /// Command to run for `backend = "command"`. The first element is the
    /// program, the rest are arguments; every `{prompt}` occurrence is replaced
    /// with the resolved system prompt, and the staged diff is written to the
    /// process's stdin. Example: `["claude", "-p", "{prompt}"]`.
    pub command: Vec<String>,
    /// Base URL of an OpenAI-compatible chat-completions API.
    pub base_url: String,
    /// Name of the environment variable holding the API key. Empty means no
    /// `Authorization` header is sent.
    pub api_key_env: String,
    pub model: String,
    /// Staged diffs larger than this are truncated before being sent.
    pub max_diff_bytes: usize,
    /// System prompt guiding the model. Used when no per-repository override
    /// matches (see `repo`).
    pub prompt_template: String,
    /// Per-repository overrides. Keys are matched in preference order:
    /// `owner/repo` derived from the `origin` remote URL first, then the
    /// working directory's base name (e.g. `gitu`). Only the prompt is
    /// overridable here — connection, credential and model settings always
    /// come from the top-level `[ai]` section, so a cloned repository can never
    /// redirect the request.
    #[serde(default)]
    pub repo: BTreeMap<String, RepoAiConfig>,
}

#[derive(Default, Debug, Deserialize)]
#[serde(default)]
pub struct RepoAiConfig {
    /// Overrides [`AiConfig::prompt_template`] for this repository.
    pub prompt_template: Option<String>,
}

impl Default for AiConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            backend: AiBackend::Api,
            command: Vec::new(),
            base_url: "https://api.openai.com/v1".into(),
            api_key_env: "OPENAI_API_KEY".into(),
            model: "gpt-4o-mini".into(),
            max_diff_bytes: 16000,
            prompt_template: String::new(),
            repo: BTreeMap::new(),
        }
    }
}

impl AiConfig {
    /// Resolve the system prompt for a repository, trying each key in `keys` in
    /// order and falling back to the global [`AiConfig::prompt_template`] when
    /// none has a non-empty override.
    pub fn prompt_for(&self, keys: &[&str]) -> &str {
        keys.iter()
            .find_map(|key| {
                self.repo
                    .get(*key)
                    .and_then(|r| r.prompt_template.as_deref())
                    .filter(|p| !p.is_empty())
            })
            .unwrap_or(&self.prompt_template)
    }
}

/// Extract `owner/repo` from a git remote URL, or `None` if it can't be parsed.
///
/// Handles the common forms: `https://host/owner/repo(.git)`,
/// `ssh://git@host/owner/repo(.git)`, and scp-style `git@host:owner/repo(.git)`.
/// The last two path segments are used, so hosts with nested groups collapse to
/// their final `group/repo` pair.
pub(crate) fn owner_repo_from_url(url: &str) -> Option<String> {
    let url = url.trim();

    let path = if let Some(idx) = url.find("://") {
        // scheme://[user@]host[:port]/path
        let after = &url[idx + 3..];
        &after[after.find('/')? + 1..]
    } else if let Some(colon) = url.find(':') {
        // scp-like: [user@]host:path
        &url[colon + 1..]
    } else {
        url
    };

    let path = path
        .trim_end_matches('/')
        .trim_end_matches(".git")
        .trim_end_matches('/');

    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    match segments.as_slice() {
        [.., owner, repo] => Some(format!("{owner}/{repo}")),
        _ => None,
    }
}

#[derive(Default, Debug, Deserialize)]
pub struct GeneralConfig {
    pub always_show_help: BoolConfigEntry,
    pub confirm_quit: BoolConfigEntry,
    pub refresh_on_file_change: BoolConfigEntry,
    pub confirm_discard: ConfirmDiscardOption,
    pub collapsed_sections: Vec<String>,
    pub stash_list_limit: usize,
    pub recent_commits_limit: usize,
    pub log_author_width: usize,
    pub mouse_support: bool,
    pub mouse_scroll_lines: usize,
}

#[derive(Default, Debug, Deserialize)]
pub struct BoolConfigEntry {
    #[serde(default)]
    pub enabled: bool,
}

#[derive(Default, Debug, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum ConfirmDiscardOption {
    #[default]
    Line,
    Hunk,
    File,
    Never,
}

#[derive(Default, Debug, Deserialize)]
pub struct StyleConfig {
    pub separator: StyleConfigEntry,

    pub info_msg: StyleConfigEntry,
    pub error_msg: StyleConfigEntry,
    pub command: StyleConfigEntry,

    #[serde(default)]
    pub menu: MenuStyleConfig,

    pub prompt: StyleConfigEntry,

    pub section_header: StyleConfigEntry,
    pub file_header: StyleConfigEntry,
    pub hunk_header: StyleConfigEntry,

    #[serde(default)]
    pub diff_highlight: DiffHighlightConfig,

    #[serde(default)]
    pub syntax_highlight: SyntaxHighlightConfig,

    #[serde(default)]
    pub picker: PickerStyleConfig,

    pub cursor: SymbolStyleConfigEntry,
    pub selection_bar: SymbolStyleConfigEntry,
    pub mark_bar: SymbolStyleConfigEntry,
    pub selection_line: StyleConfigEntry,
    pub selection_area: StyleConfigEntry,
    pub mark_area: StyleConfigEntry,

    pub hash: StyleConfigEntry,
    pub branch: StyleConfigEntry,
    pub remote: StyleConfigEntry,
    pub tag: StyleConfigEntry,
    pub author: StyleConfigEntry,
    pub age: StyleConfigEntry,

    #[serde(default)]
    pub blame: BlameStyleConfig,
}

#[derive(Default, Debug, Deserialize)]
pub struct BlameStyleConfig {
    #[serde(default)]
    pub line_num: StyleConfigEntry,
    #[serde(default)]
    pub code_line: StyleConfigEntry,
}

#[derive(Default, Debug, Deserialize)]
pub struct MenuStyleConfig {
    #[serde(default)]
    pub heading: StyleConfigEntry,
    #[serde(default)]
    pub key: StyleConfigEntry,
    /// Active argument value display (e.g., "--interactive")
    #[serde(default)]
    pub active_arg: StyleConfigEntry,
    /// Inactive argument value display
    #[serde(default)]
    pub inactive_arg: StyleConfigEntry,
}

#[derive(Default, Debug, Deserialize)]
pub struct DiffHighlightConfig {
    #[serde(default)]
    pub tag_old: StyleConfigEntry,
    #[serde(default)]
    pub tag_new: StyleConfigEntry,
    #[serde(default)]
    pub unchanged_old: StyleConfigEntry,
    #[serde(default)]
    pub unchanged_new: StyleConfigEntry,
    #[serde(default)]
    pub changed_old: StyleConfigEntry,
    #[serde(default)]
    pub changed_new: StyleConfigEntry,
}

#[derive(Default, Debug, Deserialize)]
pub struct SyntaxHighlightConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub attribute: StyleConfigEntry,
    #[serde(default)]
    pub comment: StyleConfigEntry,
    #[serde(default)]
    pub constant_builtin: StyleConfigEntry,
    #[serde(default)]
    pub constant: StyleConfigEntry,
    #[serde(default)]
    pub constructor: StyleConfigEntry,
    #[serde(default)]
    pub embedded: StyleConfigEntry,
    #[serde(default)]
    pub function_builtin: StyleConfigEntry,
    #[serde(default)]
    pub function: StyleConfigEntry,
    #[serde(default)]
    pub keyword: StyleConfigEntry,
    #[serde(default)]
    pub number: StyleConfigEntry,
    #[serde(default)]
    pub module: StyleConfigEntry,
    #[serde(default)]
    pub property: StyleConfigEntry,
    #[serde(default)]
    pub operator: StyleConfigEntry,
    #[serde(default)]
    pub punctuation_bracket: StyleConfigEntry,
    #[serde(default)]
    pub punctuation_delimiter: StyleConfigEntry,
    #[serde(default)]
    pub string_special: StyleConfigEntry,
    #[serde(default)]
    pub string: StyleConfigEntry,
    #[serde(default)]
    pub tag: StyleConfigEntry,
    #[serde(default)]
    #[serde(rename = "type")]
    pub type_regular: StyleConfigEntry,
    #[serde(default)]
    pub type_builtin: StyleConfigEntry,
    #[serde(default)]
    pub variable_builtin: StyleConfigEntry,
    #[serde(default)]
    pub variable_parameter: StyleConfigEntry,
}

#[derive(Default, Debug, Deserialize)]
pub struct PickerStyleConfig {
    #[serde(default)]
    pub prompt: StyleConfigEntry,
    #[serde(default)]
    pub info: StyleConfigEntry,
    #[serde(default)]
    pub selection_line: StyleConfigEntry,
    #[serde(default)]
    pub matched: StyleConfigEntry,
}

#[derive(Default, Debug, Deserialize)]
pub struct StyleConfigEntry {
    #[serde(default)]
    fg: Option<Color>,
    #[serde(default)]
    bg: Option<Color>,
    #[serde(default)]
    mods: Option<Modifier>,
}

#[derive(Default, Debug, Deserialize)]
pub struct SymbolStyleConfigEntry {
    #[serde(default)]
    pub symbol: char,
    #[serde(default)]
    fg: Option<Color>,
    #[serde(default)]
    bg: Option<Color>,
    #[serde(default)]
    mods: Option<Modifier>,
}

impl From<&StyleConfigEntry> for Style {
    fn from(val: &StyleConfigEntry) -> Self {
        Style {
            fg: val.fg,
            bg: val.bg,
            add_modifier: val.mods.unwrap_or(Modifier::empty()),
            sub_modifier: Modifier::empty(),
        }
    }
}

impl From<&SymbolStyleConfigEntry> for Style {
    fn from(val: &SymbolStyleConfigEntry) -> Self {
        Style {
            fg: val.fg,
            bg: val.bg,
            add_modifier: val.mods.unwrap_or(Modifier::empty()),
            sub_modifier: Modifier::empty(),
        }
    }
}

pub struct PickerBindings {
    pub next: Vec<Vec<(KeyModifiers, KeyCode)>>,
    pub previous: Vec<Vec<(KeyModifiers, KeyCode)>>,
    pub done: Vec<Vec<(KeyModifiers, KeyCode)>>,
    pub cancel: Vec<Vec<(KeyModifiers, KeyCode)>>,
}

impl TryFrom<PickerBindingsConfig> for PickerBindings {
    type Error = crate::error::Error;

    fn try_from(config: PickerBindingsConfig) -> Result<Self, Self::Error> {
        let mut bad_bindings = Vec::new();

        let next = parse_picker_keys(&config.next, "picker.next", &mut bad_bindings);
        let previous = parse_picker_keys(&config.previous, "picker.previous", &mut bad_bindings);
        let done = parse_picker_keys(&config.done, "picker.done", &mut bad_bindings);
        let cancel = parse_picker_keys(&config.cancel, "picker.cancel", &mut bad_bindings);

        if !bad_bindings.is_empty() {
            return Err(Error::Bindings {
                bad_key_bindings: bad_bindings,
            });
        }

        Ok(Self {
            next,
            previous,
            done,
            cancel,
        })
    }
}

fn parse_picker_keys(
    raw_keys: &[String],
    action_name: &str,
    bad_bindings: &mut Vec<String>,
) -> Vec<Vec<(KeyModifiers, KeyCode)>> {
    raw_keys
        .iter()
        .filter_map(|keys| {
            if let Ok(("", parsed)) = key_parser::parse_config_keys(keys) {
                Some(parsed)
            } else {
                bad_bindings.push(format!("- {} = {}", action_name, keys));
                None
            }
        })
        .collect()
}

pub fn init_config(path: Option<PathBuf>) -> Res<Config> {
    let config_path = path.unwrap_or_else(config_path);

    if config_path.exists() {
        log::info!("Loading config file at {config_path:?}");
    } else {
        log::info!("No config file at {config_path:?}");
    }

    let FigmentConfig {
        general,
        style,
        ai,
        bindings: bindings_config,
    } = Figment::new()
        .merge(Toml::string(DEFAULT_CONFIG))
        .merge(Toml::file(config_path))
        .extract()
        .map_err(Box::new)
        .map_err(Error::Config)?;
    let bindings = Bindings::try_from(bindings_config.menus)?;
    let picker_bindings = PickerBindings::try_from(bindings_config.picker)?;

    Ok(Config {
        general,
        style,
        ai,
        bindings,
        picker_bindings,
    })
}

pub fn config_path() -> PathBuf {
    choose_base_strategy()
        .expect("Unable to find the config directory!")
        .config_dir()
        .join("gitu/config.toml")
}

#[cfg(test)]
pub(crate) fn init_test_config() -> Res<Config> {
    let FigmentConfig {
        mut general,
        style,
        ai,
        bindings: bindings_config,
    } = Figment::new()
        .merge(Toml::string(DEFAULT_CONFIG))
        .extract()
        .map_err(Box::new)
        .map_err(Error::Config)?;

    general.always_show_help.enabled = false;
    general.refresh_on_file_change.enabled = false;

    Ok(Config {
        general,
        style,
        ai,
        bindings: Bindings::try_from(bindings_config.menus).unwrap(),
        picker_bindings: PickerBindings::try_from(bindings_config.picker).unwrap(),
    })
}

#[cfg(test)]
mod tests {
    use crate::style::Color;
    use figment::{
        Figment,
        providers::{Format, Toml},
    };

    use super::{DEFAULT_CONFIG, FigmentConfig, owner_repo_from_url};

    #[test]
    fn ai_prompt_per_repo_override() {
        let config: FigmentConfig = Figment::new()
            .merge(Toml::string(DEFAULT_CONFIG))
            .merge(Toml::string(
                r#"
                [ai]
                prompt_template = "global"

                [ai.repo."altsem/gitu"]
                prompt_template = "owner-repo-specific"

                [ai.repo.gitu]
                prompt_template = "name-specific"
                "#,
            ))
            .extract()
            .unwrap();

        // owner/repo is preferred over the base name.
        assert_eq!(
            config.ai.prompt_for(&["altsem/gitu", "gitu"]),
            "owner-repo-specific"
        );
        // Falls through to the base name when owner/repo is unlisted.
        assert_eq!(config.ai.prompt_for(&["other/gitu", "gitu"]), "name-specific");
        // Unlisted repo falls back to the global template.
        assert_eq!(config.ai.prompt_for(&["x/y", "z"]), "global");
    }

    #[test]
    fn ai_prompt_empty_override_falls_back() {
        let config: FigmentConfig = Figment::new()
            .merge(Toml::string(DEFAULT_CONFIG))
            .merge(Toml::string(
                r#"
                [ai]
                prompt_template = "global"

                [ai.repo."altsem/gitu"]
                prompt_template = ""
                "#,
            ))
            .extract()
            .unwrap();

        // An empty override is skipped rather than sending an empty prompt.
        assert_eq!(config.ai.prompt_for(&["altsem/gitu", "gitu"]), "global");
    }

    #[test]
    fn parses_owner_repo_from_remote_urls() {
        let cases = [
            ("https://github.com/altsem/gitu.git", "altsem/gitu"),
            ("https://github.com/altsem/gitu", "altsem/gitu"),
            ("git@github.com:altsem/gitu.git", "altsem/gitu"),
            ("ssh://git@github.com/altsem/gitu.git", "altsem/gitu"),
            ("https://github.com:443/altsem/gitu.git", "altsem/gitu"),
            ("https://gitlab.com/group/sub/proj.git", "sub/proj"),
        ];
        for (url, expected) in cases {
            assert_eq!(owner_repo_from_url(url).as_deref(), Some(expected), "{url}");
        }
    }

    #[test]
    fn config_merges() {
        let config: FigmentConfig = Figment::new()
            .merge(Toml::string(DEFAULT_CONFIG))
            .merge(Toml::string(
                r#"
                [style]
                hunk_header.bg = "light green"
                "#,
            ))
            .extract()
            .unwrap();

        assert_eq!(config.style.hunk_header.bg, Some(Color::LightGreen));
        assert_eq!(config.style.hunk_header.fg, Some(Color::Blue));
    }
}
