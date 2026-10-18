use std::{fs, io, path::Path};

use git2::{Oid, Repository, Sort};

use crate::{Res, error::Error};

#[derive(Debug, Clone)]
pub(crate) struct RebaseStatus {
    pub onto: String,
    pub head_name: String,
    pub steps: Vec<RebaseStep>,
}

#[derive(Debug, Clone)]
pub(crate) struct RebaseStep {
    pub action: String,
    pub oid: Option<String>,
    pub short_id: String,
    pub summary: String,
}

impl RebaseStep {
    fn commit(repo: &Repository, action: &str, rev: &str) -> Res<Self> {
        let commit = repo
            .revparse_single(rev)
            .and_then(|object| object.peel_to_commit())
            .map_err(Error::ReadLog)?;
        let short_id = commit.as_object().short_id().map_err(Error::ReadOid)?;
        Ok(Self {
            action: action.into(),
            oid: Some(commit.id().to_string()),
            short_id: String::from_utf8_lossy(&short_id).into_owned(),
            summary: commit.summary().unwrap_or("").into(),
        })
    }
}

// The administrative directory can be outside the worktree (and .git can be a file).
pub(crate) fn read(repo: &Repository) -> Res<Option<RebaseStatus>> {
    let merge_dir = repo.path().join("rebase-merge");
    let apply_dir = repo.path().join("rebase-apply");
    let (dir, merge) = if merge_dir.is_dir() {
        (merge_dir, true)
    } else if apply_dir.is_dir() && !apply_dir.join("applying").exists() {
        (apply_dir, false)
    } else {
        return Ok(None);
    };
    let onto = read_file(&dir.join("onto"))?;
    let head_name = read_file(&dir.join("head-name"))?;
    let mut steps = Vec::new();
    let mut stop = None;
    if merge {
        let comment = repo
            .config()
            .ok()
            .and_then(|config| config.get_string("core.commentChar").ok())
            .unwrap_or_else(|| "#".into());
        let todo = read_file(&dir.join("git-rebase-todo"))?;
        steps.extend(
            todo.lines()
                .rev()
                .filter_map(|line| parse_step(repo, line, &comment)),
        );
        let last = read_file(&dir.join("done"))?
            .lines()
            .rev()
            .find_map(|line| parse_step(repo, line, &comment));
        let stopped = read_file(&dir.join("stopped-sha"))?;
        if let Some(mut step) = last
            && matches!(step.action.as_str(), "exec" | "break")
        {
            // stopped-sha can still describe an earlier, resolved conflict.
            step.summary = format!("{} {}", step.action, step.summary).trim().into();
            step.action = "stop".into();
            steps.push(step);
        } else if !stopped.is_empty() {
            // At an edit stop HEAD is the rewritten commit, while a conflict
            // still refers to the original commit in stopped-sha.
            let rev = if dir.join("amend").exists() {
                "HEAD"
            } else {
                &stopped
            };
            let step = RebaseStep::commit(repo, "stop", rev)?;
            stop = step.oid.clone();
            steps.push(step);
        }
    } else {
        let next = read_file(&dir.join("next"))?.parse::<usize>().unwrap_or(0);
        let last = read_file(&dir.join("last"))?.parse::<usize>().unwrap_or(0);
        for n in ((next + 1)..=last).rev() {
            let patch = read_file(&dir.join(format!("{n:04}")))?;
            if let Some(oid) = patch
                .lines()
                .next()
                .and_then(|line| line.split_whitespace().nth(1))
            {
                steps.push(RebaseStep::commit(repo, "pick", oid)?);
            }
        }
        let stopped = read_file(&dir.join("original-commit"))?;
        if !stopped.is_empty() {
            let step = RebaseStep::commit(repo, "stop", &stopped)?;
            stop = step.oid.clone();
            steps.push(step);
        }
    }

    // Use the rewritten history, not the 'done' file: Git moves an action to
    // 'done' before attempting it, and squash/fixup can combine several actions.
    let mut walk = repo.revwalk().map_err(Error::ReadLog)?;
    walk.set_sorting(Sort::TOPOLOGICAL)
        .map_err(Error::ReadLog)?;
    walk.push_head().map_err(Error::ReadLog)?;
    walk.hide(Oid::from_str(&onto).map_err(Error::ReadOid)?)
        .map_err(Error::ReadLog)?;
    for oid in walk {
        let oid = oid.map_err(Error::ReadLog)?.to_string();
        if stop.as_deref() != Some(&oid) {
            steps.push(RebaseStep::commit(repo, "done", &oid)?);
        }
    }
    steps.push(RebaseStep::commit(repo, "onto", &onto)?);
    let onto_name = super::branch_name_lossy(repo.workdir().ok_or(Error::NoRepoWorkdir)?, &onto)?
        .unwrap_or_else(|| onto.chars().take(7).collect());
    Ok(Some(RebaseStatus {
        onto: onto_name,
        head_name: head_name
            .strip_prefix("refs/heads/")
            .unwrap_or(&head_name)
            .into(),
        steps,
    }))
}

fn read_file(path: &Path) -> Res<String> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(text.trim().into()),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(String::new()),
        Err(err) => Err(Error::ReadRebaseStatusFile(err)),
    }
}

fn parse_step(repo: &Repository, line: &str, comment: &str) -> Option<RebaseStep> {
    let line = line.trim();
    if line.is_empty() || line.starts_with(comment) {
        return None;
    }
    let (action, rest) = line.split_once(char::is_whitespace).unwrap_or((line, ""));
    let action = match action {
        "p" => "pick",
        "r" => "reword",
        "e" => "edit",
        "s" => "squash",
        "f" => "fixup",
        "x" => "exec",
        "b" => "break",
        "d" => "drop",
        "l" => "label",
        "t" => "reset",
        "m" => "merge",
        "u" => "update-ref",
        action => action,
    };
    let mut words = rest.split_whitespace();
    let first = words.next();
    let rev = match action {
        "pick" | "reword" | "edit" | "squash" | "drop" => first,
        "fixup" | "merge" if matches!(first, Some("-C" | "-c")) => words.next(),
        "fixup" => first,
        _ => None,
    };
    Some(
        rev.and_then(|rev| RebaseStep::commit(repo, action, rev).ok())
            .unwrap_or_else(|| RebaseStep {
                action: action.into(),
                oid: None,
                short_id: String::new(),
                summary: rest.trim().into(),
            }),
    )
}
