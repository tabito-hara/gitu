use super::*;

fn setup(ctx: TestContext) -> TestContext {
    run(&ctx.dir, &["git", "checkout", "-b", "other-branch"]);
    run(&ctx.dir, &["git", "checkout", "main"]);
    commit(&ctx.dir, "new-file", "hello");
    run(&ctx.dir, &["git", "checkout", "other-branch"]);
    ctx
}

#[test]
fn rebase_menu() {
    snapshot!(setup(setup_clone!()), "r");
}

#[test]
fn rebase_elsewhere_prompt() {
    snapshot!(setup(setup_clone!()), "re");
}

#[test]
fn rebase_elsewhere() {
    snapshot!(setup(setup_clone!()), "remain<enter>");
}

// The first commit applies, the second conflicts, and the third is still pending.
fn setup_progress(ctx: TestContext, apply: bool) -> TestContext {
    run(&ctx.dir, &["git", "checkout", "-b", "other-branch"]);
    commit(&ctx.dir, "first", "first");
    commit(&ctx.dir, "initial-file", "feature");
    commit(&ctx.dir, "last", "last");
    run(&ctx.dir, &["git", "checkout", "main"]);
    commit(&ctx.dir, "initial-file", "main");
    run(&ctx.dir, &["git", "checkout", "other-branch"]);
    run_ignore_status(
        &ctx.dir,
        &[
            "git",
            "rebase",
            if apply { "--apply" } else { "--merge" },
            "main",
        ],
    );
    ctx
}

fn progress(ctx: &TestContext) -> crate::git::rebase_status::RebaseStatus {
    let repo = git2::Repository::open(&ctx.dir).unwrap();
    crate::git::rebase_status(&repo)
        .unwrap()
        .expect("rebase in progress")
}

#[test]
fn rebase_progress() {
    let mut ctx = setup_progress(setup_clone!(), false);
    let status = progress(&ctx);
    assert_eq!(status.head_name, "other-branch");
    assert_eq!(status.onto, "main");
    assert_eq!(
        status
            .steps
            .iter()
            .map(|step| (step.action.as_str(), step.summary.as_str()))
            .collect::<Vec<_>>(),
        [
            ("pick", "add last"),
            ("stop", "modify initial-file"),
            ("done", "add first"),
            ("onto", "modify initial-file")
        ]
    );
    // The completed row refers to the rewritten commit, not the old object.
    assert_eq!(
        status.steps[2].oid.as_deref(),
        Some(run(&ctx.dir, &["git", "rev-parse", "HEAD"]).trim())
    );
    let mut app = ctx.init_app();
    insta::assert_snapshot!(ctx.redact_buffer());

    // Resolve the conflict; refreshing removes the sequence once rebase finishes.
    fs::write(ctx.dir.join("initial-file"), "resolved").unwrap();
    run(&ctx.dir, &["git", "add", "initial-file"]);
    run(
        &ctx.dir,
        &["git", "-c", "core.editor=true", "rebase", "--continue"],
    );
    app.update_screens().unwrap();
    app.redraw_now(&mut ctx.term).unwrap();
    assert!(!ctx.redact_buffer().contains("Rebasing"));
    assert!(
        crate::git::rebase_status(&git2::Repository::open(&ctx.dir).unwrap())
            .unwrap()
            .is_none()
    );
}

#[test]
fn rebase_apply_progress() {
    let mut ctx = setup_progress(setup_clone!(), true);
    assert_eq!(
        progress(&ctx)
            .steps
            .iter()
            .map(|step| step.action.as_str())
            .collect::<Vec<_>>(),
        ["pick", "stop", "done", "onto"]
    );
    ctx.init_app();
    insta::assert_snapshot!(ctx.redact_buffer());
    run(&ctx.dir, &["git", "rebase", "--abort"]);
    assert!(
        crate::git::rebase_status(&git2::Repository::open(&ctx.dir).unwrap())
            .unwrap()
            .is_none()
    );
}

#[test]
fn rebase_progress_show_commit() {
    let mut ctx = setup_progress(setup_clone!(), false);
    let mut app = ctx.init_app();
    // The pending commit is selectable and opens the usual commit view.
    ctx.update(&mut app, keys("j<enter>"));
    insta::assert_snapshot!(ctx.redact_buffer());
}

#[test]
fn rebase_edit_progress() {
    let mut ctx = setup(setup_clone!());
    commit(&ctx.dir, "first", "first");
    commit(&ctx.dir, "last", "last");
    run(
        &ctx.dir,
        &[
            "git",
            "-c",
            "sequence.editor=sed -i.bak '1s/^pick/edit/'",
            "rebase",
            "-i",
            "main",
        ],
    );
    let status = progress(&ctx);
    assert_eq!(
        status
            .steps
            .iter()
            .map(|step| step.action.as_str())
            .collect::<Vec<_>>(),
        ["pick", "stop", "onto"]
    );
    ctx.init_app();
    insta::assert_snapshot!(ctx.redact_buffer());
}

#[test]
fn rebase_exec_progress() {
    let mut ctx = setup(setup_clone!());
    commit(&ctx.dir, "first", "first");
    commit(&ctx.dir, "last", "last");
    run_ignore_status(&ctx.dir, &["git", "rebase", "--exec", "false", "main"]);
    let status = progress(&ctx);
    assert_eq!(
        status
            .steps
            .iter()
            .map(|step| step.action.as_str())
            .collect::<Vec<_>>(),
        ["exec", "pick", "stop", "done", "onto"]
    );
    assert_eq!(status.steps[2].summary, "exec false");
    ctx.init_app();
    insta::assert_snapshot!(ctx.redact_buffer());
}

#[test]
fn rebase_progress_in_worktree() {
    let mut ctx = setup_clone!();
    let worktree = ctx.dir.parent().unwrap().join("worktree");
    run(
        &ctx.dir,
        &[
            "git",
            "worktree",
            "add",
            "-b",
            "other-branch",
            worktree.to_str().unwrap(),
        ],
    );
    commit(&worktree, "initial-file", "feature");
    commit(&ctx.dir, "initial-file", "main");
    run_ignore_status(&worktree, &["git", "rebase", "main"]);
    let repo = git2::Repository::open(&worktree).unwrap();
    assert_eq!(
        crate::git::rebase_status(&repo).unwrap().unwrap().steps[0].action,
        "stop"
    );
    ctx.init_app_at_path(worktree);
    insta::assert_snapshot!(ctx.redact_buffer());
}

#[test]
fn rebase_exec_after_conflict() {
    let ctx = setup_progress(setup_clone!(), false);
    let todo = ctx.dir.join(".git/rebase-merge/git-rebase-todo");
    let remaining = fs::read_to_string(&todo).unwrap();
    fs::write(todo, format!("exec false\n{remaining}")).unwrap();
    fs::write(ctx.dir.join("initial-file"), "resolved").unwrap();
    run(&ctx.dir, &["git", "add", "initial-file"]);
    run_ignore_status(
        &ctx.dir,
        &["git", "-c", "core.editor=true", "rebase", "--continue"],
    );
    let status = progress(&ctx);
    let stopped = status
        .steps
        .iter()
        .find(|step| step.action == "stop")
        .unwrap();
    assert_eq!(stopped.summary, "exec false");
    assert!(stopped.oid.is_none());
    assert_eq!(
        status
            .steps
            .iter()
            .filter(|step| step.action == "done")
            .count(),
        2
    );
}

#[test]
fn rebase_squash_progress() {
    let ctx = setup(setup_clone!());
    commit(&ctx.dir, "first", "first");
    commit(&ctx.dir, "last", "last");
    run_ignore_status(
        &ctx.dir,
        &[
            "git",
            "-c",
            "sequence.editor=true",
            "rebase",
            "-i",
            "--exec",
            "false",
            "main",
        ],
    );
    // --exec inserts its first command after the first pick, so rewrite the
    // remaining todo explicitly to combine the next commit before stopping.
    let repo = git2::Repository::open(&ctx.dir).unwrap();
    let todo = repo.path().join("rebase-merge/git-rebase-todo");
    let remaining = fs::read_to_string(&todo).unwrap();
    fs::write(todo, remaining.replace("pick ", "fixup ")).unwrap();
    run_ignore_status(
        &ctx.dir,
        &["git", "-c", "core.editor=true", "rebase", "--continue"],
    );
    let status = progress(&ctx);
    assert_eq!(
        status
            .steps
            .iter()
            .filter(|step| step.action == "done")
            .count(),
        1
    );
    let done = status
        .steps
        .iter()
        .find(|step| step.action == "done")
        .unwrap();
    assert_eq!(
        done.oid.as_deref(),
        Some(run(&ctx.dir, &["git", "rev-parse", "HEAD"]).trim())
    );
}

#[test]
fn rebase_todo_actions() {
    let ctx = setup_progress(setup_clone!(), false);
    let repo = git2::Repository::open(&ctx.dir).unwrap();
    run(&ctx.dir, &["git", "config", "core.commentChar", ";"]);
    let oid = run(&ctx.dir, &["git", "rev-parse", "other-branch"]);
    let oid = oid.trim();
    fs::write(repo.path().join("rebase-merge/git-rebase-todo"), format!(
        "; comment\n\np {oid} last\nr {oid} last\ne {oid} last\ns {oid} last\nf -C {oid} last\nd {oid} last\nm -C {oid} branch\nl branch\nt branch\nu refs/heads/topic\nx echo hello\nb\n"
    )).unwrap();
    let status = progress(&ctx);
    assert_eq!(
        status
            .steps
            .iter()
            .map(|step| step.action.as_str())
            .collect::<Vec<_>>(),
        [
            "break",
            "exec",
            "update-ref",
            "reset",
            "label",
            "merge",
            "drop",
            "fixup",
            "squash",
            "edit",
            "reword",
            "pick",
            "stop",
            "done",
            "onto"
        ]
    );
    assert_eq!(status.steps[1].summary, "echo hello");
    assert!(status.steps[..5].iter().all(|step| step.oid.is_none()));
    assert!(
        status.steps[5..12]
            .iter()
            .all(|step| step.oid.as_deref() == Some(oid))
    );
}

#[test]
fn rebase_detached_head() {
    let ctx = setup(setup_clone!());
    commit(&ctx.dir, "first", "first");
    run(&ctx.dir, &["git", "checkout", "--detach"]);
    run_ignore_status(&ctx.dir, &["git", "rebase", "--exec", "false", "main"]);
    assert_eq!(progress(&ctx).head_name, "detached HEAD");
}
