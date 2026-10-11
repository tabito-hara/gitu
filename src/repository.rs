use std::path::{Path, PathBuf};

use crossterm::event::Event;
use git2::Repository;

use crate::{
    Res,
    config::Config,
    error::Error,
    picker::{PickerData, PickerItem, PickerState},
    term::Term,
    ui,
};

pub(crate) fn expand_home_path(path: &Path) -> PathBuf {
    if let Ok(suffix) = path.strip_prefix("~")
        && let Some(home) = std::env::var_os("HOME")
    {
        PathBuf::from(home).join(suffix)
    } else {
        path.to_path_buf()
    }
}

pub(crate) fn resolve_path(base: &Path, input: &str) -> PathBuf {
    let path = expand_home_path(Path::new(input));
    if path.is_absolute() {
        path
    } else {
        base.join(path)
    }
}

pub(crate) fn directory_completions(base: &Path, input: &str) -> Vec<String> {
    if input == "~" {
        return std::env::var_os("HOME")
            .map(|_| format!("~{}", std::path::MAIN_SEPARATOR))
            .into_iter()
            .collect();
    }
    let path = Path::new(input);
    let (parent, prefix) = if input.is_empty() || input.ends_with(std::path::MAIN_SEPARATOR) {
        (path, "")
    } else {
        (
            path.parent().unwrap_or_else(|| Path::new("")),
            path.file_name()
                .and_then(|name| name.to_str())
                .unwrap_or(""),
        )
    };
    let mut candidates = Vec::new();
    if let Ok(entries) = std::fs::read_dir(resolve_path(base, &parent.display().to_string())) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            if name.to_string_lossy().starts_with(prefix) && entry.path().is_dir() {
                candidates.push(format!(
                    "{}{}",
                    parent.join(name).display(),
                    std::path::MAIN_SEPARATOR
                ));
            }
        }
    }
    candidates.sort();
    candidates
}

fn picker(base: &Path, input: String) -> PickerState {
    let items = directory_completions(base, &input)
        .into_iter()
        .map(|path| PickerItem::new(path.clone(), PickerData::Item(path)))
        .collect();
    let mut picker = PickerState::new("Open repository", items, true);
    picker.input_state.set_value(input);
    picker.update_filter();
    picker
}

pub(crate) fn select_repository(
    term: &mut Term,
    config: &Config,
    base: &Path,
) -> Res<Option<Repository>> {
    let mut selection = picker(base, String::new());
    let mut error = None;
    loop {
        ui::repository_selection(term, config, &selection, error.as_deref())?;
        let previous_input = selection.pattern().to_string();
        if let Event::Key(key) = term.read_event()? {
            selection.handle_key_event(key, &config.picker_bindings);
        }
        if selection.is_cancelled() {
            return Ok(None);
        }
        if selection.is_done() {
            let Some(item) = selection.selected() else {
                selection = picker(base, previous_input);
                continue;
            };
            let input = item.data.display().to_string();
            let path = resolve_path(base, &input);
            match Repository::discover(&path) {
                Ok(repo) if repo.workdir().is_some() => return Ok(Some(repo)),
                Ok(_) => error = Some(Error::NoRepoWorkdir.to_string()),
                Err(err) if err.code() == git2::ErrorCode::NotFound && path.is_dir() => {
                    error = None;
                    selection = picker(
                        base,
                        format!("{}{}", path.display(), std::path::MAIN_SEPARATOR),
                    );
                    continue;
                }
                Err(err) => error = Some(Error::OpenRepo(err).to_string()),
            }
            selection = picker(base, input);
        } else if selection.pattern() != previous_input {
            let input = selection.pattern().to_string();
            selection = picker(base, input);
            error = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::term::{TermBackend, TestBuffer};

    fn terminal(input: &str) -> TermBackend {
        TermBackend::Test {
            buffer: TestBuffer::new(80, 20),
            events: crate::key_parser::parse_test_keys(input)
                .unwrap()
                .1
                .into_iter()
                .rev()
                .map(|(modifiers, code)| {
                    Event::Key(crossterm::event::KeyEvent::new(code, modifiers))
                })
                .collect(),
        }
    }

    #[test]
    fn startup_selection_opens_repository() {
        let base = temp_dir::TempDir::new().unwrap();
        let path = base.path().join("repo");
        Repository::init(&path).unwrap();
        let config = crate::config::init_test_config().unwrap();
        let repo = select_repository(&mut terminal("repo<tab><enter>"), &config, base.path())
            .unwrap()
            .unwrap();
        assert_eq!(repo.workdir().unwrap(), path.canonicalize().unwrap());
    }

    #[test]
    fn startup_selection_navigates_directories() {
        let base = temp_dir::TempDir::new().unwrap();
        let path = base.path().join("group/repo");
        Repository::init(&path).unwrap();
        let config = crate::config::init_test_config().unwrap();
        let repo = select_repository(
            &mut terminal("group<enter>repo<enter>"),
            &config,
            base.path(),
        )
        .unwrap()
        .unwrap();
        assert_eq!(repo.workdir().unwrap(), path.canonicalize().unwrap());
    }

    #[test]
    fn startup_selection_recovers_from_invalid_path() {
        let base = temp_dir::TempDir::new().unwrap();
        let path = base.path().join("repo");
        Repository::init(&path).unwrap();
        let config = crate::config::init_test_config().unwrap();
        let repo = select_repository(
            &mut terminal("missing<enter><ctrl+u>repo<enter>"),
            &config,
            base.path(),
        )
        .unwrap()
        .unwrap();
        assert_eq!(repo.workdir().unwrap(), path.canonicalize().unwrap());
    }

    #[test]
    fn startup_selection_cancels() {
        let base = temp_dir::TempDir::new().unwrap();
        let config = crate::config::init_test_config().unwrap();
        assert!(
            select_repository(&mut terminal("<esc>"), &config, base.path())
                .unwrap()
                .is_none()
        );
    }
}
