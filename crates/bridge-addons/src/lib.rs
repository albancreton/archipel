//! The addons the library ships.
//!
//! One file per addon, in a folder named after what it is about: `acp` for any adapter,
//! `claude`, `pi` and so on for one harness. To remove an addon, delete its file and its line
//! in [`all`].
//!
//! Each addon's [`Info`](bridge_addon_api::Info) says what it stands in for and what has to
//! happen upstream before it can go. `bridged addons` prints them.

pub mod acp;
pub mod pi;

use bridge_addon_api::{Addon, Kind};

/// Every addon in this crate, in the order they should see messages.
#[must_use]
pub fn all() -> Vec<Box<dyn Addon>> {
    vec![
        Box::new(acp::restore_settings::RestoreSettings),
        Box::new(pi::check_setting_values::CheckSettingValues),
    ]
}

/// The addons that should be on unless there is a reason not to: the fixes.
/// Features are a matter of taste and are switched on one by one.
#[must_use]
pub fn recommended() -> Vec<Box<dyn Addon>> {
    all()
        .into_iter()
        .filter(|addon| addon.info().kind == Kind::Fix)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn every_addon_has_a_unique_well_formed_id_and_says_why_it_exists() {
        let mut ids = BTreeSet::new();
        for addon in all() {
            let info = addon.info();
            let (scope, name) = info.id.split_once('.').expect("<scope>.<name>");
            let lower = |part: &str| {
                !part.is_empty()
                    && part
                        .chars()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
            };
            assert!(lower(scope) && lower(name), "{}", info.id);
            assert!(ids.insert(info.id), "{} is used twice", info.id);
            assert!(
                !info.summary.is_empty() && !info.because.is_empty(),
                "{}",
                info.id
            );
            if info.kind == Kind::Fix {
                assert!(
                    !info.until.is_empty(),
                    "{}: a fix says when it can go",
                    info.id
                );
            }
        }
    }
}
