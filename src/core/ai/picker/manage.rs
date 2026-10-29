//! The manage rows of llamacpp's model level: install when there is no
//! llama-server, remove when the one found is the install lazarobox owns.
//! A system binary gets neither (it wins on the PATH and is never touched).

use std::path::Path;

use super::install::{InstallAsk, Wizard};
use super::{Level, Outcome, PickKey, PickRow, Picker};
use crate::core::ai::install::{self, Platform};
use crate::core::ai::{Detection, Status};

/// The entry that opens the install wizard.
pub const INSTALL_ROW: &str = "install llama-server…";

/// The entry that asks to delete the owned install.
pub const REMOVE_ROW: &str = "remove the llama.cpp install";

/// The llamacpp binary when it lives in the owned folder; `None` for a system
/// binary, a missing one, or an unknown data home.
pub(super) fn owned_binary(detection: &Detection) -> Option<&Path> {
    let data_home = detection.data_home.as_deref()?;
    detection
        .engines
        .iter()
        .find(|s| s.id == "llamacpp")?
        .binary
        .as_deref()
        .filter(|binary| install::is_owned(binary, data_home))
}

/// The manage row llamacpp offers now, if any.
pub(super) fn manage_row(detection: &Detection) -> Option<PickRow> {
    let (key, label) = if owned_binary(detection).is_some() {
        (PickKey::RemoveLlama, REMOVE_ROW)
    } else if detection
        .engines
        .iter()
        .any(|s| s.id == "llamacpp" && s.status == Status::NotInstalled)
    {
        (PickKey::InstallLlama, INSTALL_ROW)
    } else {
        return None;
    };
    Some(PickRow {
        key,
        label: label.to_owned(),
        mark: super::Mark::Plain,
        enabled: true,
        current: false,
    })
}

impl Picker {
    /// Enter on a manage row: open the install plan, or the remove prompt.
    pub(super) fn open_manage(&mut self, key: &PickKey, detection: Option<&Detection>) {
        let Some(detection) = detection else { return };
        match key {
            PickKey::InstallLlama => {
                let variants = install::variants(Platform::CURRENT, detection.vulkan_icd);
                self.open_install(variants);
            }
            PickKey::RemoveLlama => {
                if let Some(binary) = owned_binary(detection) {
                    self.level = Level::Install(Wizard::remove(binary.to_path_buf()));
                    self.cursor = 0;
                    self.offset = 0;
                }
            }
            _ => {}
        }
    }

    /// The newer-build check an owned install asks for when llamacpp's model
    /// level opens. The caller decides whether it runs (once per run, and
    /// not when updates are off).
    pub(super) fn newer_check(engine: &str, detection: Option<&Detection>) -> Outcome {
        if engine == "llamacpp" && detection.is_some_and(|d| owned_binary(d).is_some()) {
            Outcome::Install(InstallAsk::CheckNewer)
        } else {
            Outcome::Stay
        }
    }
}
