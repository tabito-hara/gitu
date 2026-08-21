use std::{
    collections::BTreeSet,
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    process::Command,
    rc::Rc,
};

use crate::{
    Res,
    app::{App, PromptParams, State},
    error::Error,
    gitu_diff::Status,
    item_data::ItemData,
    picker::{PickerData, PickerItem, PickerState},
    term::Term,
};

use super::{Action, OpTrait};

pub(crate) struct GitignoreInTopdir;
impl OpTrait for GitignoreInTopdir {
    fn get_action(&self, target: &ItemData) -> Option<Action> {
        let current_file = current_file(target);
        Some(Rc::new(move |app: &mut App, term: &mut Term| {
            let rule = read_pattern(app, term, current_file.as_deref(), None)?;
            let file = workdir(app)?.join(".gitignore");
            append_gitignore_rule(app, term, &rule, &file, true)
        }))
    }

    fn display(&self, _state: &State) -> String {
        "shared at top level (.gitignore)".into()
    }
}

pub(crate) struct GitignoreInSubdir;
impl OpTrait for GitignoreInSubdir {
    fn get_action(&self, target: &ItemData) -> Option<Action> {
        let current_file = current_file(target);
        Some(Rc::new(move |app: &mut App, term: &mut Term| {
            let directory = read_directory(app, term, current_file.as_deref())?;
            let rule = read_pattern(app, term, current_file.as_deref(), Some(&directory))?;
            let file = workdir(app)?.join(&directory).join(".gitignore");
            append_gitignore_rule(app, term, &rule, &file, true)
        }))
    }

    fn display(&self, _state: &State) -> String {
        "shared in subdirectory (path/to/.gitignore)".into()
    }
}

pub(crate) struct GitignoreInGitdir;
impl OpTrait for GitignoreInGitdir {
    fn get_action(&self, target: &ItemData) -> Option<Action> {
        let current_file = current_file(target);
        Some(Rc::new(move |app: &mut App, term: &mut Term| {
            let rule = read_pattern(app, term, current_file.as_deref(), None)?;
            let file = app.state.repo.path().join("info").join("exclude");
            append_gitignore_rule(app, term, &rule, &file, false)
        }))
    }

    fn display(&self, _state: &State) -> String {
        "private for this repository (.git/info/exclude)".into()
    }
}

pub(crate) struct GitignoreOnSystem;
impl OpTrait for GitignoreOnSystem {
    fn get_action(&self, target: &ItemData) -> Option<Action> {
        let current_file = current_file(target);
        Some(Rc::new(move |app: &mut App, term: &mut Term| {
            let rule = read_pattern(app, term, current_file.as_deref(), None)?;
            let file = system_excludes_file(app)?;
            append_gitignore_rule(app, term, &rule, &file, false)
        }))
    }

    fn display(&self, _state: &State) -> String {
        "private for all repositories (core.excludesFile)".into()
    }
}

fn read_directory(app: &mut App, term: &mut Term, current_file: Option<&Path>) -> Res<PathBuf> {
    let default = current_file
        .and_then(Path::parent)
        .filter(|path| !path.as_os_str().is_empty())
        .map(|path| path.to_string_lossy().to_string())
        .unwrap_or_else(|| ".".into());

    let directory = app.prompt(
        term,
        &PromptParams {
            prompt: "Limit rule to files in",
            create_default_value: Box::new(move |_| Some(default.clone())),
            ..Default::default()
        },
    )?;

    Ok(PathBuf::from(directory))
}

fn read_pattern(
    app: &mut App,
    term: &mut Term,
    current_file: Option<&Path>,
    directory: Option<&Path>,
) -> Res<String> {
    let choices = pattern_choices(app, current_file, directory)?;
    let result = app.pick(
        term,
        PickerState::new(
            "File or pattern to ignore",
            choices
                .into_iter()
                .map(|choice| PickerItem::new(choice.clone(), PickerData::Item(choice)))
                .collect(),
            true,
        ),
    )?;

    Ok(result
        .map(|data| data.display().to_string())
        .unwrap_or_default())
}

fn append_gitignore_rule(
    app: &mut App,
    term: &mut Term,
    rule: &str,
    file: &Path,
    stage: bool,
) -> Res<()> {
    if rule.is_empty() {
        return Ok(());
    }

    if let Some(parent) = file.parent() {
        fs::create_dir_all(parent).map_err(Error::Gitignore)?;
    }

    let needs_newline = fs::read(file)
        .map(|content| content.last().is_some_and(|byte| *byte != b'\n'))
        .unwrap_or(false);

    let mut out = OpenOptions::new()
        .create(true)
        .append(true)
        .open(file)
        .map_err(Error::Gitignore)?;

    if needs_newline {
        writeln!(out).map_err(Error::Gitignore)?;
    }
    writeln!(out, "{}", rule.replace('\\', "\\\\")).map_err(Error::Gitignore)?;

    if stage {
        let mut cmd = Command::new("git");
        cmd.args(["add", "--"]).arg(file);
        app.run_cmd(term, &[], cmd)?;
    } else {
        app.update_screens()?;
        app.display_info(format!("Updated {}", file.display()));
    }

    Ok(())
}

fn pattern_choices(
    app: &App,
    current_file: Option<&Path>,
    directory: Option<&Path>,
) -> Res<Vec<String>> {
    let files = untracked_files(app, false, directory)?;
    let dirs = untracked_files(app, true, directory)?;
    let mut choices = BTreeSet::new();

    for file in &files {
        if let Some(ext) = Path::new(file).extension().and_then(|ext| ext.to_str()) {
            choices.insert(format!("*.{ext}"));
            if let Some(parent) = Path::new(file).parent()
                && !parent.as_os_str().is_empty()
            {
                choices.insert(format!("{}/*.{ext}", parent.to_string_lossy()));
            }
        }
    }

    choices.extend(
        files
            .iter()
            .chain(dirs.iter())
            .map(|path| format!("/{path}")),
    );

    let mut choices = choices.into_iter().collect::<Vec<_>>();
    if let Some(default) = default_pattern(current_file, directory)
        && choices.contains(&default)
    {
        choices.retain(|choice| choice != &default);
        choices.insert(0, default);
    }

    Ok(choices)
}

fn default_pattern(current_file: Option<&Path>, directory: Option<&Path>) -> Option<String> {
    let current_file = current_file?;
    let path = if let Some(directory) = directory {
        current_file.strip_prefix(directory).ok()?
    } else {
        current_file
    };
    Some(format!("/{}", path.to_string_lossy()))
}

fn untracked_files(app: &App, directories: bool, directory: Option<&Path>) -> Res<Vec<String>> {
    let mut cmd = Command::new("git");
    cmd.args(["ls-files", "--others", "--exclude-standard"]);
    if directories {
        cmd.arg("--directory");
    }
    if let Some(directory) = directory {
        cmd.arg("--").arg(directory);
    }

    let out = cmd
        .current_dir(workdir(app)?)
        .output()
        .map_err(Error::Gitignore)?;

    let prefix = directory
        .filter(|path| *path != Path::new("."))
        .map(|path| {
            let mut prefix = path.to_string_lossy().trim_end_matches('/').to_string();
            prefix.push('/');
            prefix
        });

    Ok(String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|line| match &prefix {
            Some(prefix) => line.strip_prefix(prefix).map(ToOwned::to_owned),
            None => Some(line.to_string()),
        })
        .collect())
}

fn current_file(target: &ItemData) -> Option<PathBuf> {
    match target {
        ItemData::AllUntracked(files) => files.first().cloned(),
        ItemData::Untracked(path) => Some(path.clone()),
        ItemData::Delta { diff, file_i, .. }
        | ItemData::Hunk { diff, file_i, .. }
        | ItemData::HunkLine { diff, file_i, .. } => {
            let header = &diff.file_diffs[*file_i].header;
            let path = if header.status == Status::Deleted {
                &header.old_file
            } else {
                &header.new_file
            };
            Some(PathBuf::from(path.fmt(&diff.text).as_ref()))
        }
        _ => None,
    }
}

fn system_excludes_file(app: &App) -> Res<PathBuf> {
    let out = Command::new("git")
        .args(["config", "--get", "--path", "core.excludesFile"])
        .current_dir(workdir(app)?)
        .output()
        .map_err(Error::GitignoreConfig)?;

    if !out.status.success() {
        return Err(Error::GitignoreConfigUnset);
    }

    let path = String::from_utf8(out.stdout)
        .map_err(Error::GitignoreConfigUtf8)?
        .trim()
        .to_string();
    if path.is_empty() {
        return Err(Error::GitignoreConfigUnset);
    }

    Ok(PathBuf::from(path))
}

fn workdir(app: &App) -> Res<&Path> {
    app.state.repo.workdir().ok_or(Error::NoRepoWorkdir)
}
