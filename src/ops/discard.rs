use super::{Action, OpTrait, confirm};
use crate::{
    Res,
    app::{App, State},
    config::ConfirmDiscardOption,
    git::diff::{DiffType, PatchMode},
    item_data::{ItemData, Ref},
    screen::{FileSelection, HunkLineSelection},
    term::Term,
};
use std::{path::PathBuf, process::Command, rc::Rc};

pub(crate) struct Discard;
impl OpTrait for Discard {
    fn get_action(&self, target: &ItemData) -> Option<Action> {
        let target = target.clone();
        target_discard_action(&target).map(|mut target_action| {
            Rc::new(move |app: &mut App, term: &mut Term| {
                if let Some(selection) = app.screen().selected_hunk_line_range() {
                    let mut action = discard_line_range(selection);
                    return Rc::get_mut(&mut action).unwrap()(app, term);
                }
                if let Some(selection) = app.screen().selected_file_range() {
                    let mut action = discard_file_selection(selection);
                    return Rc::get_mut(&mut action).unwrap()(app, term);
                }

                Rc::get_mut(&mut target_action).unwrap()(app, term)
            }) as Action
        })
    }

    fn is_target_op(&self) -> bool {
        true
    }

    fn display(&self, _state: &State) -> String {
        "Discard".into()
    }
}

fn target_discard_action(target: &ItemData) -> Option<Action> {
    let action = match target {
        ItemData::Reference {
            kind: Ref::Head(branch),
            ..
        } => discard_branch(branch.clone()),
        ItemData::Untracked(file) => clean_file(file.clone()),
        ItemData::Delta { diff, file_i, .. } => {
            let patch = diff.format_file_patch(*file_i);
            match diff.diff_type {
                DiffType::WorkdirToIndex => reverse_worktree(patch),
                DiffType::IndexToTree => reverse_index_and_worktree(patch),
                DiffType::TreeToTree => reverse_index_and_worktree(patch),
            }
        }
        ItemData::Hunk {
            diff,
            file_i,
            hunk_i,
        } => {
            let patch = diff.format_hunk_patch(*file_i, *hunk_i);
            match diff.diff_type {
                DiffType::WorkdirToIndex => reverse_worktree(patch),
                DiffType::IndexToTree => reverse_index_and_worktree(patch),
                DiffType::TreeToTree => reverse_index_and_worktree(patch),
            }
        }
        ItemData::HunkLine {
            diff,
            file_i,
            hunk_i,
            line_i,
            ..
        } => {
            let patch =
                diff.format_line_patch(*file_i, *hunk_i, *line_i..(line_i + 1), PatchMode::Reverse);

            match diff.diff_type {
                DiffType::WorkdirToIndex => reverse_worktree(patch),
                DiffType::IndexToTree => reverse_index_and_worktree(patch),
                DiffType::TreeToTree => reverse_index_and_worktree(patch),
            }
        }
        _ => return None,
    };

    Some(action)
}

fn discard_branch(branch: String) -> Action {
    Rc::new(move |app, term| {
        confirm(app, term, "Really discard? (y or n)")?;
        super::branch::delete(app, term, &branch)
    })
}

fn clean_file(file: PathBuf) -> Action {
    clean_files(vec![file])
}

fn clean_files(files: Vec<PathBuf>) -> Action {
    Rc::new(move |app, term| {
        confirm_discard(app, term)?;

        let mut cmd = Command::new("git");
        cmd.args(["clean", "--force"]);
        cmd.args(files.clone());

        app.run_cmd(term, &[], cmd)
    })
}

fn reverse_worktree(patch: String) -> Action {
    let patch_bytes = patch.into_bytes();

    Rc::new(move |app, term| {
        confirm_discard(app, term)?;

        run_reverse_worktree(app, term, &patch_bytes)
    })
}

fn reverse_index_and_worktree(patch: String) -> Action {
    let patch_bytes = patch.into_bytes();

    Rc::new(move |app, term| {
        confirm_discard(app, term)?;

        run_reverse_index_and_worktree(app, term, &patch_bytes)
    })
}

fn discard_line_range(selection: HunkLineSelection) -> Action {
    Rc::new(move |app, term| {
        confirm_discard(app, term)?;

        let patch = selection
            .diff
            .format_line_patch(
                selection.file_i,
                selection.hunk_i,
                selection.line_range.clone(),
                PatchMode::Reverse,
            )
            .into_bytes();

        match selection.diff.diff_type {
            DiffType::WorkdirToIndex => run_reverse_worktree(app, term, &patch),
            DiffType::IndexToTree | DiffType::TreeToTree => {
                run_reverse_index_and_worktree(app, term, &patch)
            }
        }?;
        app.screen_mut().clear_mark();
        Ok(())
    })
}

fn discard_file_selection(selection: FileSelection) -> Action {
    let untracked = selection.untracked;
    let unstaged_patch = selection
        .unstaged
        .into_iter()
        .map(|file| file.patch)
        .collect::<String>()
        .into_bytes();
    let staged_patch = selection
        .staged
        .into_iter()
        .map(|file| file.patch)
        .collect::<String>()
        .into_bytes();

    Rc::new(move |app, term| {
        confirm_discard(app, term)?;

        if !untracked.is_empty() {
            let mut cmd = Command::new("git");
            cmd.args(["clean", "--force"]);
            cmd.args(untracked.clone());
            app.run_cmd(term, &[], cmd)?;
        }

        if !unstaged_patch.is_empty() {
            run_reverse_worktree(app, term, &unstaged_patch)?;
        }

        if !staged_patch.is_empty() {
            run_reverse_index_and_worktree(app, term, &staged_patch)?;
        }

        app.screen_mut().clear_mark();
        Ok(())
    })
}

fn run_reverse_worktree(app: &mut App, term: &mut Term, patch: &[u8]) -> Res<()> {
    let mut cmd = Command::new("git");
    cmd.args(["apply", "--reverse", "--recount"]);
    app.run_cmd(term, patch, cmd)
}

fn run_reverse_index_and_worktree(app: &mut App, term: &mut Term, patch: &[u8]) -> Res<()> {
    let mut cmd = Command::new("git");
    cmd.args(["apply", "--reverse", "--index", "--recount"]);
    app.run_cmd(term, patch, cmd)
}

fn confirm_discard(app: &mut App, term: &mut Term) -> Res<()> {
    if app.state.config.general.confirm_discard <= ConfirmDiscardOption::File {
        confirm(app, term, "Really discard? (y or n)")?;
    }
    Ok(())
}
