//! A menu save says where it wrote: users learn where `config.toml` lives.

use std::path::PathBuf;

use super::*;

fn at_home(config: Option<&str>) -> App {
    App::new(80, 25)
        .with_env(
            Some("/bin/zsh"),
            Some("/home/ana".into()),
            "/home/ana".into(),
        )
        .with_tool_engines(&[])
        .with_config_path(config.map(PathBuf::from))
}

fn saved(a: &mut App) -> Vec<Effect> {
    a.menu = Some(MenuState::open(&a.config));
    a.input = InputMode::Menu;
    a.update(AppEvent::ConfigSaved)
}

#[test]
fn a_save_names_the_file_it_wrote() {
    let mut a = at_home(Some("/home/ana/.config/lazarobox/config.toml"));
    saved(&mut a);
    assert_eq!(a.notice(), Some("saved to ~/.config/lazarobox/config.toml"));
    let mut a = at_home(Some("/etc/xdg/lazarobox/config.toml"));
    saved(&mut a);
    assert_eq!(a.notice(), Some("saved to /etc/xdg/lazarobox/config.toml"));
}

#[test]
fn a_stale_save_result_or_no_path_says_nothing() {
    let mut a = at_home(None);
    saved(&mut a);
    assert_eq!(a.notice(), None);
    // The menu is gone: the result is stale.
    let mut a = at_home(Some("/home/ana/.config/lazarobox/config.toml"));
    a.update(AppEvent::ConfigSaved);
    assert_eq!(a.notice(), None);
}

#[test]
fn starting_the_managed_server_takes_the_notice() {
    let mut a = at_home(Some("/home/ana/.config/lazarobox/config.toml"));
    let mut ai = AiConfig::default();
    ai.manage(PathBuf::from("/m/q.gguf"));
    a.menu = Some(MenuState::open(&a.config).with_ai(ai, None));
    a.input = InputMode::Menu;
    a.update(AppEvent::ConfigSaved);
    assert_eq!(a.notice(), Some("starting llama-server with q.gguf…"));
}
