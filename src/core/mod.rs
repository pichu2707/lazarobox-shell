//! Pure, IO-free core of the terminal pane (emulator, key encoding, modal input).

pub mod ai;
pub mod build_jobs;
pub mod cli;
pub mod clipboard;
pub mod config;
pub mod config_append;
pub mod config_edit;
pub mod copy;
pub mod desktop;
pub mod keys;
pub mod layout;
pub mod link_list;
pub mod links;
pub mod menu;
pub mod mouse;
pub mod nesting;
pub mod opener;
pub mod osc7;
pub mod pane;
pub mod prefix;
pub mod probe;
pub mod pty;
pub mod star;
pub mod start;
pub mod update;
