use super::OpTrait;
use crate::{
    Action,
    app::{App, State},
    git::{self, diff::PatchMode},
    gitu_diff::Status,
    item_data::ItemData,
    screen::{FileSelection, HunkLineSelection},
    term::Term,
};
use std::{path::PathBuf, process::Command, rc::Rc};

pub(crate) struct Unstage;
impl OpTrait for Unstage {
    fn get_action(&self, target: &ItemData) -> Option<Action> {
        let target = target.clone();
        target_unstage_action(&target).map(|mut target_action| {
            Rc::new(move |app: &mut App, term: &mut Term| {
                if let Some(selection) = app.screen().selected_hunk_line_range() {
                    app.screen_mut().clear_mark();
                    let mut action = unstage_line_range(selection);
                    return Rc::get_mut(&mut action).unwrap()(app, term);
                }
                if let Some(selection) = app.screen().selected_file_range() {
                    let paths = unstageable_files(selection);
                    if !paths.is_empty() {
                        app.screen_mut().clear_mark();
                        let mut action = unstage_files(paths);
                        return Rc::get_mut(&mut action).unwrap()(app, term);
                    }
                }

                Rc::get_mut(&mut target_action).unwrap()(app, term)
            }) as Action
        })
    }
    fn is_target_op(&self) -> bool {
        true
    }

    fn display(&self, _state: &State) -> String {
        "Unstage".into()
    }
}

fn target_unstage_action(target: &ItemData) -> Option<Action> {
    let action = match target {
        ItemData::AllStaged(_) => unstage_staged(),
        ItemData::Delta { diff, file_i, .. } => {
            let diff_header = &diff.file_diffs[*file_i].header;
            let file_path = match diff_header.status {
                Status::Deleted => &diff_header.old_file,
                _ => &diff_header.new_file,
            };

            unstage_file(file_path.fmt(&diff.text).into_owned().into())
        }
        ItemData::Hunk {
            diff,
            file_i,
            hunk_i,
        } => unstage_patch(diff.format_hunk_patch(*file_i, *hunk_i).into_bytes()),
        ItemData::HunkLine {
            diff,
            file_i,
            hunk_i,
            line_i,
            ..
        } => unstage_line(
            diff.format_line_patch(*file_i, *hunk_i, *line_i..(*line_i + 1), PatchMode::Reverse)
                .into_bytes(),
        ),
        _ => return None,
    };

    Some(action)
}

pub(crate) struct UnstageAll;
impl OpTrait for UnstageAll {
    fn get_action(&self, _target: &ItemData) -> Option<Action> {
        Some(unstage_staged())
    }

    fn display(&self, _state: &State) -> String {
        "Unstage all".into()
    }
}

fn unstage_staged() -> Action {
    Rc::new(move |app: &mut App, term: &mut Term| {
        let mut cmd = Command::new("git");
        cmd.args(["reset", "HEAD", "--"]);

        app.run_cmd(term, &[], cmd)
    })
}

fn unstage_file(file: PathBuf) -> Action {
    Rc::new(move |app: &mut App, term: &mut Term| app.run_cmd(term, &[], git::restore_index(&file)))
}

fn unstage_files(files: Vec<PathBuf>) -> Action {
    Rc::new(move |app: &mut App, term: &mut Term| {
        let mut cmd = Command::new("git");
        cmd.args(["restore", "--staged"]);
        cmd.args(files.clone());

        app.run_cmd(term, &[], cmd)
    })
}

fn unstageable_files(selection: FileSelection) -> Vec<PathBuf> {
    selection.staged
}

fn unstage_patch(input: Vec<u8>) -> Action {
    Rc::new(move |app: &mut App, term: &mut Term| {
        let mut cmd = Command::new("git");
        cmd.args(["apply", "--cached", "--reverse"]);

        app.run_cmd(term, &input, cmd)
    })
}

fn unstage_line(input: Vec<u8>) -> Action {
    Rc::new(move |app: &mut App, term: &mut Term| {
        let mut cmd = Command::new("git");
        cmd.args(["apply", "--cached", "--reverse", "--recount"]);

        app.run_cmd(term, &input, cmd)
    })
}

fn unstage_line_range(selection: HunkLineSelection) -> Action {
    Rc::new(move |app: &mut App, term: &mut Term| {
        let mut cmd = Command::new("git");
        cmd.args(["apply", "--cached", "--reverse", "--recount"]);

        let input = selection
            .diff
            .format_line_patch(
                selection.file_i,
                selection.hunk_i,
                selection.line_range.clone(),
                PatchMode::Reverse,
            )
            .into_bytes();

        app.run_cmd(term, &input, cmd)
    })
}
