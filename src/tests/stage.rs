use super::*;

#[test]
fn staged_file() {
    let mut ctx = setup_clone!();
    run(&ctx.dir, &["touch", "new-file"]);
    run(&ctx.dir, &["git", "add", "new-file"]);

    ctx.init_app();
    insta::assert_snapshot!(ctx.redact_buffer());
}

#[test]
fn stage_all_unstaged() {
    let ctx = setup_clone!();
    commit(&ctx.dir, "firstfile", "testing\ntesttest\n");
    commit(&ctx.dir, "secondfile", "testing\ntesttest\n");

    fs::write(ctx.dir.join("firstfile"), "blahonga\n").unwrap();
    fs::write(ctx.dir.join("secondfile"), "blahonga\n").unwrap();
    snapshot!(ctx, "js");
}

#[test]
fn stage_modified_from_anywhere() {
    let ctx = setup_clone!();
    commit(&ctx.dir, "tracked", "testing\n");

    fs::write(ctx.dir.join("tracked"), "changed\n").unwrap();
    fs::write(ctx.dir.join("untracked"), "new\n").unwrap();
    snapshot!(ctx, "S");
}

#[test]
fn stage_all_untracked() {
    let ctx = setup_clone!();
    run(&ctx.dir, &["touch", "file-a"]);
    run(&ctx.dir, &["touch", "file-b"]);
    snapshot!(ctx, "js");
}

#[test]
fn stage_selected_untracked_files() {
    let mut ctx = setup_clone!();
    run(&ctx.dir, &["touch", "file-a", "file-b", "file-c"]);

    let mut app = ctx.init_app();
    ctx.update(&mut app, keys("jj<ctrl+space>js"));

    assert_eq!(
        run(&ctx.dir, &["git", "diff", "--cached", "--name-only"]),
        "file-a\nfile-b\n"
    );
    assert!(run(&ctx.dir, &["git", "status", "--short"]).contains("?? file-c"));
}

#[test]
fn stage_removed_line() {
    let ctx = setup_clone!();
    commit(&ctx.dir, "firstfile", "testing\ntesttest\n");
    fs::write(ctx.dir.join("firstfile"), "weehooo\nblrergh\n").unwrap();
    snapshot!(ctx, "jj<tab><ctrl+j><ctrl+j>s");
}

#[test]
fn stage_added_line() {
    let ctx = setup_clone!();
    commit(&ctx.dir, "firstfile", "testing\ntesttest\n");
    fs::write(ctx.dir.join("firstfile"), "weehooo\nblrergh\n").unwrap();

    snapshot!(ctx, "jj<tab><ctrl+j><ctrl+j><ctrl+j><ctrl+j>s");
}

#[test]
fn stage_selected_lines() {
    let mut ctx = setup_clone!();
    commit(&ctx.dir, "firstfile", "testing\ntesttest\n");
    fs::write(ctx.dir.join("firstfile"), "weehooo\nblrergh\n").unwrap();

    let mut app = ctx.init_app();
    ctx.update(
        &mut app,
        keys("jj<tab><ctrl+j><ctrl+j><ctrl+j><ctrl+j><ctrl+space><ctrl+j>s"),
    );

    let cached = run(&ctx.dir, &["git", "diff", "--cached", "--", "firstfile"]);
    assert!(cached.contains("+weehooo\n+blrergh\n"));
    assert!(!cached.contains("-testing\n-testtest\n"));

    let unstaged = run(&ctx.dir, &["git", "diff", "--", "firstfile"]);
    assert!(unstaged.contains("-testing\n-testtest\n"));
    assert!(!unstaged.contains("+weehooo\n+blrergh\n"));
}

#[test]
fn stage_selected_unstaged_files() {
    let mut ctx = setup_clone!();
    commit(&ctx.dir, "file-a", "base\n");
    commit(&ctx.dir, "file-b", "base\n");
    commit(&ctx.dir, "file-c", "base\n");
    fs::write(ctx.dir.join("file-a"), "changed\n").unwrap();
    fs::write(ctx.dir.join("file-b"), "changed\n").unwrap();
    fs::write(ctx.dir.join("file-c"), "changed\n").unwrap();

    let mut app = ctx.init_app();
    ctx.update(&mut app, keys("jj<ctrl+space>js"));

    assert_eq!(
        run(&ctx.dir, &["git", "diff", "--cached", "--name-only"]),
        "file-a\nfile-b\n"
    );
    assert_eq!(run(&ctx.dir, &["git", "diff", "--name-only"]), "file-c\n");
}

#[test]
fn marked_unstaged_files_are_visually_distinct() {
    let ctx = setup_clone!();
    commit(&ctx.dir, "file-a", "base\n");
    commit(&ctx.dir, "file-b", "base\n");
    commit(&ctx.dir, "file-c", "base\n");
    fs::write(ctx.dir.join("file-a"), "changed\n").unwrap();
    fs::write(ctx.dir.join("file-b"), "changed\n").unwrap();
    fs::write(ctx.dir.join("file-c"), "changed\n").unwrap();

    snapshot!(ctx, "jj<ctrl+space>j");
}

#[test]
fn clear_mark_cancels_selected_files() {
    let mut ctx = setup_clone!();
    commit(&ctx.dir, "file-a", "base\n");
    commit(&ctx.dir, "file-b", "base\n");
    commit(&ctx.dir, "file-c", "base\n");
    fs::write(ctx.dir.join("file-a"), "changed\n").unwrap();
    fs::write(ctx.dir.join("file-b"), "changed\n").unwrap();
    fs::write(ctx.dir.join("file-c"), "changed\n").unwrap();
    let mut app = ctx.init_app();

    ctx.update(&mut app, keys("jj<ctrl+space>j<ctrl+g>s"));

    assert_eq!(
        run(&ctx.dir, &["git", "diff", "--cached", "--name-only"]),
        "file-b\n"
    );
    assert_eq!(
        run(&ctx.dir, &["git", "diff", "--name-only"]),
        "file-a\nfile-c\n"
    );
}

#[test]
fn marked_lines_are_visually_distinct() {
    let ctx = setup_clone!();
    commit(&ctx.dir, "firstfile", "testing\ntesttest\n");
    fs::write(ctx.dir.join("firstfile"), "weehooo\nblrergh\n").unwrap();

    snapshot!(
        ctx,
        "jj<tab><ctrl+j><ctrl+j><ctrl+j><ctrl+j><ctrl+space><ctrl+j>"
    );
}

#[test]
fn stage_changes_crlf() {
    let ctx = setup_clone!();
    commit(&ctx.dir, "testfile", "testing\r\ntesttest\r\n");
    fs::write(ctx.dir.join("testfile"), "test\r\ntesttest\r\n").expect("error writing to file");

    snapshot!(ctx, "jj<tab>");
}

#[test]
fn stage_deleted_file() {
    let ctx = setup_clone!();
    commit(&ctx.dir, "to-delete", "testing\ntesttest\n");
    run(&ctx.dir, &["rm", "to-delete"]);
    snapshot!(ctx, "jjs");
}

#[test]
#[cfg(not(target_os = "windows"))]
fn stage_deleted_executable_file() {
    let ctx = setup_clone!();
    commit(&ctx.dir, "script.sh", "#!/bin/bash\necho hello\n");
    run(&ctx.dir, &["chmod", "+x", "script.sh"]);
    run(&ctx.dir, &["git", "add", "script.sh"]);
    run(&ctx.dir, &["git", "commit", "-m", "add executable script"]);
    run(&ctx.dir, &["rm", "script.sh"]);
    snapshot!(ctx, "jjs");
}

#[test]
fn stage_file_with_spaces_in_name() {
    let ctx = setup_clone!();
    run(&ctx.dir, &["touch", "file with space.txt"]);
    snapshot!(ctx, "js");
}
