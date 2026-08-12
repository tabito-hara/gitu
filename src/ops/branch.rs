use super::{Action, OpTrait};
use crate::{
    Res,
    app::{App, PromptParams, State},
    error::Error,
    git::{
        self, does_branch_exist, get_current_branch, get_current_branch_name, is_branch_merged,
        remote::get_branch_upstream,
    },
    item_data::{ItemData, Ref},
    menu::arg::Arg,
    picker::{PickerParams, PickerState},
    screen::NavMode,
    term::Term,
};
use std::{process::Command, rc::Rc};

pub(crate) fn init_args() -> Vec<Arg> {
    vec![]
}

pub(crate) struct Checkout;
impl OpTrait for Checkout {
    fn get_action(&self, _target: &ItemData) -> Option<Action> {
        Some(Rc::new(move |app: &mut App, term: &mut Term| {
            let result = app.pick(
                term,
                PickerState::with_refs(PickerParams {
                    prompt: "Checkout".into(),
                    refs: &git::branches_tags(&app.state.repo)?,
                    exclude_ref: git::head_ref(&app.state.repo)?,
                    default: app.selected_rev(),
                    allow_custom_input: true,
                }),
            )?;

            if let Some(data) = result {
                let rev = data.display();
                checkout(app, term, rev)?;
            }

            Ok(())
        }))
    }

    fn display(&self, _state: &State) -> String {
        "Checkout branch/revision".into()
    }
}

fn checkout(app: &mut App, term: &mut Term, rev: &str) -> Res<()> {
    let mut cmd = Command::new("git");
    cmd.args(["checkout", rev]);

    app.run_cmd(term, &[], cmd)?;
    Ok(())
}

pub(crate) struct CheckoutNewBranch;
impl OpTrait for CheckoutNewBranch {
    fn get_action(&self, _target: &ItemData) -> Option<Action> {
        Some(Rc::new(|app: &mut App, term: &mut Term| {
            // Like Magit (default `magit-branch-read-upstream-first`), ask for
            // the starting point first — defaulting to the current branch and
            // selectable from the same list as Checkout — then the branch name.
            let start_point = app.pick(
                term,
                PickerState::with_refs(PickerParams {
                    prompt: "Starting branch".into(),
                    refs: &git::branches_tags(&app.state.repo)?,
                    exclude_ref: None,
                    default: Some(git::head(&app.state.repo)?),
                    allow_custom_input: true,
                }),
            )?;

            let Some(data) = start_point else {
                return Ok(());
            };
            let start_point = data.display().to_string();

            let branch_name = app.prompt(
                term,
                &PromptParams {
                    prompt: "Create and checkout branch:",
                    ..Default::default()
                },
            )?;

            if branch_name.is_empty() {
                return Err(Error::BranchNameRequired);
            }

            checkout_new_branch(app, term, &branch_name, &start_point)?;
            Ok(())
        }))
    }

    fn display(&self, _state: &State) -> String {
        "Checkout new branch".into()
    }
}

fn checkout_new_branch(
    app: &mut App,
    term: &mut Term,
    branch_name: &str,
    start_point: &str,
) -> Res<()> {
    let mut cmd = Command::new("git");
    cmd.args(["checkout", "-b", branch_name, start_point]);
    app.run_cmd(term, &[], cmd)?;
    Ok(())
}

pub(crate) struct Delete;
impl OpTrait for Delete {
    fn get_action(&self, target: &ItemData) -> Option<Action> {
        let default = target.rev();

        Some(Rc::new(move |app: &mut App, term: &mut Term| {
            if let Some(branches) = app.screen().selected_branches() {
                delete_many(app, term, &branches)?;
                return Ok(());
            }

            let result = app.pick(
                term,
                PickerState::with_branches(PickerParams {
                    prompt: "Delete".into(),
                    refs: &git::branches(&app.state.repo, None)?,
                    exclude_ref: git::head_ref(&app.state.repo)?,
                    default: default.clone(),
                    allow_custom_input: false,
                }),
            )?;

            if let Some(data) = result {
                let branch_name = data.display();
                delete(app, term, branch_name)?;
            }

            Ok(())
        }))
    }

    fn display(&self, _state: &State) -> String {
        "Delete branch".into()
    }
}

pub(crate) struct MarkDelete;
impl OpTrait for MarkDelete {
    fn get_action(&self, target: &ItemData) -> Option<Action> {
        if !matches!(
            target,
            ItemData::Reference {
                kind: Ref::Head(_),
                ..
            }
        ) {
            return None;
        }

        Some(Rc::new(move |app: &mut App, _term: &mut Term| {
            if let Some(branches) = app.screen().selected_branches() {
                app.screen_mut().mark_branches_for_delete(&branches);
                app.screen_mut().clear_mark();
                return Ok(());
            }

            if app.screen_mut().mark_selected_branch_for_delete() {
                app.screen_mut().select_next(NavMode::Normal);
            }
            Ok(())
        }))
    }

    fn is_target_op(&self) -> bool {
        true
    }

    fn display(&self, _state: &State) -> String {
        "Mark branch for deletion".into()
    }
}

pub(crate) struct ExecuteDeletes;
impl OpTrait for ExecuteDeletes {
    fn get_action(&self, _target: &ItemData) -> Option<Action> {
        Some(Rc::new(move |app: &mut App, term: &mut Term| {
            let branches = app.screen().delete_marked_branches();
            if !branches.is_empty() {
                app.confirm(term, "Really delete marked branches? (y or n)")?;
                delete_many(app, term, &branches)?;
                app.screen_mut().clear_mark();
                app.screen_mut().clear_delete_marks();
            }
            Ok(())
        }))
    }

    fn display(&self, _state: &State) -> String {
        "Delete marked branches".into()
    }
}

pub fn delete(app: &mut App, term: &mut Term, branch_name: &str) -> Res<()> {
    delete_many(app, term, &[branch_name.to_string()])
}

fn delete_many(app: &mut App, term: &mut Term, branch_names: &[String]) -> Res<()> {
    if branch_names.is_empty() {
        return Err(Error::BranchNameRequired);
    }

    let current_branch = get_current_branch_name(&app.state.repo).unwrap();
    for branch_name in branch_names {
        if branch_name.is_empty() {
            return Err(Error::BranchNameRequired);
        }

        if current_branch == *branch_name {
            return Err(Error::CannotDeleteCurrentBranch);
        }
    }

    let has_unmerged_branch = branch_names
        .iter()
        .any(|branch_name| !is_branch_merged(&app.state.repo, branch_name).unwrap_or(false));

    let mut cmd = Command::new("git");
    cmd.args(["branch", "-d"]);

    if has_unmerged_branch {
        app.confirm(term, "Branch is not fully merged. Really delete? (y or n)")?;
        cmd.arg("-f");
    }

    cmd.args(branch_names.iter().map(String::as_str));

    app.run_cmd(term, &[], cmd)?;
    Ok(())
}

pub(crate) struct Rename;
impl OpTrait for Rename {
    fn get_action(&self, target: &ItemData) -> Option<Action> {
        let default = target.rev();

        Some(Rc::new(move |app: &mut App, term: &mut Term| {
            let result = app.pick(
                term,
                PickerState::with_branches(PickerParams {
                    prompt: "Rename branch".into(),
                    refs: &git::branches(&app.state.repo, None)?,
                    exclude_ref: None,
                    default: default.clone(),
                    allow_custom_input: false,
                }),
            )?;

            if let Some(data) = result {
                let old_name = data.display().to_string();

                let new_name = app.prompt(
                    term,
                    &PromptParams {
                        prompt: "Rename branch to",
                        ..Default::default()
                    },
                )?;

                if new_name.is_empty() {
                    return Err(Error::BranchNameRequired);
                }

                rename(app, term, &old_name, &new_name)?;
            }

            Ok(())
        }))
    }

    fn display(&self, _state: &State) -> String {
        "Rename branch".into()
    }
}

pub fn rename(app: &mut App, term: &mut Term, old_name: &str, new_name: &str) -> Res<()> {
    let mut cmd = Command::new("git");
    cmd.args(["branch", "-m", old_name, new_name]);

    app.run_cmd(term, &[], cmd)?;
    Ok(())
}

pub(crate) struct Spinoff;
impl OpTrait for Spinoff {
    fn get_action(&self, target: &ItemData) -> Option<Action> {
        let default = match target {
            ItemData::Reference {
                kind: Ref::Head(branch),
                ..
            } => Some(branch.clone()),
            _ => None,
        };

        Some(Rc::new(move |app: &mut App, term: &mut Term| {
            let default = default.clone();

            let new_branch_name = app.prompt(
                term,
                &PromptParams {
                    prompt: "Name for new branch",
                    create_default_value: Box::new(move |_| default.clone()),
                    ..Default::default()
                },
            )?;

            if new_branch_name.is_empty() {
                return Err(Error::BranchNameRequired);
            }

            if does_branch_exist(&app.state.repo, &new_branch_name)? {
                return Err(Error::SpinoffBranchExists(new_branch_name.to_string()));
            }

            let current_branch = get_current_branch(&app.state.repo)?;
            let current_branch_name = get_current_branch_name(&app.state.repo)?;

            if current_branch_name == new_branch_name {
                return Err(Error::CannotSpinoffCurrentBranch);
            }

            let base_commit_oid = app.state.repo.head().map_err(Error::GetHead)?.target();

            let upstream_branch_commit_oid = get_branch_upstream(&current_branch)?
                .map(|branch| branch.into_reference())
                .map(|x| x.target());

            drop(current_branch);

            // Checkout new branch
            let mut cmd = Command::new("git");
            cmd.args(["checkout", "-b", &new_branch_name]);
            app.run_cmd(term, &[], cmd)?;

            let Some(upstream_branch_commit_oid) = upstream_branch_commit_oid else {
                app.display_info(format!("Branch {current_branch_name} not changed"));
                return Ok(());
            };

            if base_commit_oid == upstream_branch_commit_oid {
                app.display_info(format!("Branch {current_branch_name} not changed"));
                return Ok(());
            }

            let base_oid = base_commit_oid.ok_or(Error::BaseCommitOid)?;
            let upstream_oid = upstream_branch_commit_oid.ok_or(Error::UpstreamCommitOid)?;
            let merge_base = &app.state.repo.merge_base(base_oid, upstream_oid).unwrap();

            let mut cmd = Command::new("git");
            cmd.args([
                "update-ref",
                "-m",
                &format!(r##""reset: moving to {merge_base}""##),
                &format!("refs/heads/{current_branch_name}"),
                &merge_base.to_string(),
            ]);
            app.run_cmd(term, &[], cmd)?;

            app.display_info(format!(
                "Branch {current_branch_name} was reset to {merge_base}"
            ));

            Ok(())
        }))
    }

    fn display(&self, _state: &State) -> String {
        "Spinoff branch".into()
    }
}
