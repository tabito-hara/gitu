use super::*;

#[test]
pub(crate) fn quit() {
    let app = snapshot!(setup_clone!(), QUIT);
    assert!(app.state.quit);
}

#[test]
pub(crate) fn quit_from_menu() {
    let app = snapshot!(setup_clone!(), "hq");
    assert!(!app.state.quit);
}

#[test]
pub(crate) fn confirm_quit_prompt() {
    let mut ctx = setup_clone!();
    ctx.config().general.confirm_quit.enabled = true;

    let app = snapshot!(ctx, QUIT);
    assert!(!app.state.quit);
}

#[test]
pub(crate) fn confirm_quit() {
    let mut ctx = setup_clone!();
    ctx.config().general.confirm_quit.enabled = true;

    let app = snapshot!(ctx, &format!("{QUIT}y"));
    assert!(app.state.quit);
}

#[test]
fn q_and_escape_do_not_quit_status() {
    let mut ctx = setup_clone!();
    let mut app = ctx.init_app();
    ctx.update(&mut app, keys("q<esc>"));
    assert!(!app.state.quit);
    assert_eq!(app.state.screens.len(), 1);
}

#[test]
fn q_closes_log_without_quitting() {
    let mut ctx = setup_clone!();
    let mut app = ctx.init_app();
    ctx.update(&mut app, keys("ll"));
    assert_eq!(app.state.screens.len(), 2);
    ctx.update(&mut app, keys("q"));
    assert!(!app.state.quit);
    assert_eq!(app.state.screens.len(), 1);
}

#[test]
fn quit_from_log() {
    let mut ctx = setup_clone!();
    let mut app = ctx.init_app();
    ctx.update(&mut app, keys("ll"));
    assert_eq!(app.state.screens.len(), 2);
    ctx.update(&mut app, keys(QUIT));
    assert!(app.state.quit);
}

#[test]
fn quit_requires_both_control_keys() {
    let mut ctx = setup_clone!();
    let mut app = ctx.init_app();
    ctx.update(&mut app, keys("<ctrl+c><ctrl+x>"));
    assert!(!app.state.quit);
    ctx.update(&mut app, keys("<ctrl+c>"));
    assert!(app.state.quit);
}

#[test]
fn closing_menu_keeps_underlying_view() {
    let mut ctx = setup_clone!();
    let mut app = ctx.init_app();
    ctx.update(&mut app, keys("llbq"));
    assert!(!app.state.quit);
    assert!(app.state.pending_menu.is_none());
    assert_eq!(app.state.screens.len(), 2);
    ctx.update(&mut app, keys("<esc>"));
    assert!(!app.state.quit);
    assert_eq!(app.state.screens.len(), 1);
}

#[test]
fn quit_while_menu_is_open() {
    let mut ctx = setup_clone!();
    let mut app = ctx.init_app();
    ctx.update(&mut app, keys("b"));
    assert!(app.state.pending_menu.is_some());
    ctx.update(&mut app, keys(QUIT));
    assert!(app.state.quit);
}
