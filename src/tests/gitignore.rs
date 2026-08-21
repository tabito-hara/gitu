use std::fs;

use super::*;

#[test]
fn gitignore_menu() {
    let ctx = setup_clone!();
    snapshot!(ctx, "i");
}

#[test]
fn gitignore_in_topdir() {
    let mut ctx = setup_clone!();
    fs::write(ctx.dir.join("ignored.log"), "ignore me\n").unwrap();

    let mut app = ctx.init_app();
    ctx.update(&mut app, keys("jit<enter>"));

    assert_eq!(
        fs::read_to_string(ctx.dir.join(".gitignore")).unwrap(),
        "/ignored.log\n"
    );
    assert_eq!(
        run(&ctx.dir, &["git", "diff", "--cached", "--name-only"]),
        ".gitignore\n"
    );
}

#[test]
fn gitignore_in_subdir() {
    let mut ctx = setup_clone!();
    fs::create_dir(ctx.dir.join("nested")).unwrap();
    fs::write(ctx.dir.join("nested/ignored.log"), "ignore me\n").unwrap();

    let mut app = ctx.init_app();
    ctx.update(&mut app, keys("isnested<enter>ignored.log<enter>"));

    assert_eq!(
        fs::read_to_string(ctx.dir.join("nested/.gitignore")).unwrap(),
        "/ignored.log\n"
    );
    assert_eq!(
        run(&ctx.dir, &["git", "diff", "--cached", "--name-only"]),
        "nested/.gitignore\n"
    );
}

#[test]
fn gitignore_in_gitdir() {
    let mut ctx = setup_clone!();
    fs::write(ctx.dir.join("private.log"), "ignore me\n").unwrap();

    let mut app = ctx.init_app();
    ctx.update(&mut app, keys("jip<enter>"));

    assert!(
        fs::read_to_string(ctx.dir.join(".git/info/exclude"))
            .unwrap()
            .ends_with("/private.log\n")
    );
    assert_eq!(
        run(&ctx.dir, &["git", "diff", "--cached", "--name-only"]),
        ""
    );
}

#[test]
fn gitignore_on_system() {
    let mut ctx = setup_clone!();
    let excludes_file = ctx.dir.join("global-ignore");
    run(
        &ctx.dir,
        &[
            "git",
            "config",
            "core.excludesFile",
            excludes_file.to_str().unwrap(),
        ],
    );
    fs::write(ctx.dir.join("global.log"), "ignore me\n").unwrap();

    let mut app = ctx.init_app();
    ctx.update(&mut app, keys("jig<enter>"));

    assert_eq!(fs::read_to_string(excludes_file).unwrap(), "/global.log\n");
    assert_eq!(
        run(&ctx.dir, &["git", "diff", "--cached", "--name-only"]),
        ""
    );
}
