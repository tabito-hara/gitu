use std::collections::BTreeMap;

use crate::{
    error::Error,
    key_parser,
    menu::{Menu, PendingMenu},
    ops::Op,
};
use crossterm::event::{KeyCode, KeyModifiers};

pub struct Bindings {
    vec: Vec<Binding>,
}

// Used to parse the bindings coming out of `figment`. If there are any issues,
// these are collected so the user can be informed.
impl TryFrom<BTreeMap<Menu, BTreeMap<Op, Vec<String>>>> for Bindings {
    type Error = crate::error::Error;

    fn try_from(value: BTreeMap<Menu, BTreeMap<Op, Vec<String>>>) -> Result<Self, Self::Error> {
        let mut bindings = Vec::new();
        let mut bad_bindings = Vec::new();
        let mut legacy_root_quit = false;
        for (menu, ops) in value {
            let close_bindings: Vec<_> = ops
                .get(&Op::Close)
                .into_iter()
                .flatten()
                .filter_map(|keys| Binding::parse(menu, keys, Op::Close))
                .collect();
            for (op, binds) in ops {
                for keys in binds {
                    if let Some(binding) = Binding::parse(menu, &keys, op.clone()) {
                        // Older configs used quit for both exiting and going back.
                        // Prefer close when those entries overlap the new bindings.
                        if op == Op::Quit
                            && close_bindings
                                .iter()
                                .any(|close| close.keys == binding.keys)
                        {
                            legacy_root_quit |= menu == Menu::Root;
                            continue;
                        }
                        bindings.push(binding);
                    } else {
                        // Format bad key bindings like " - {menu}.{op} = {keys}"
                        bad_bindings.push(format!(
                            "- {}.{} = {}",
                            menu.as_ref(),
                            op.as_ref(),
                            keys
                        ));
                    }
                }
            }
        }

        // A legacy root.quit array replaces the default quit shortcut on merge.
        if legacy_root_quit
            && !bindings
                .iter()
                .any(|binding| binding.menu == Menu::Root && binding.op == Op::Quit)
        {
            bindings.push(Binding::parse(Menu::Root, "<ctrl+x>+<ctrl+c>", Op::Quit).unwrap());
        }

        // If any bindings are bad, present them all to the user as an error.
        if !bad_bindings.is_empty() {
            return Err(Error::Bindings {
                bad_key_bindings: bad_bindings,
            });
        }

        Ok(Self { vec: bindings })
    }
}

impl Bindings {
    pub(crate) fn match_bindings<'a>(
        &'a self,
        pending: &'a Menu,
        events: &'a [(KeyModifiers, KeyCode)],
    ) -> impl Iterator<Item = &'a Binding> + 'a {
        self.vec
            .iter()
            .filter(move |binding| &binding.menu == pending)
            .filter(|binding| binding.keys.starts_with(events))
    }

    pub(crate) fn list<'a>(&'a self, pending: &Menu) -> impl Iterator<Item = &'a Binding> {
        let expected = if pending == &Menu::Help {
            Menu::Root
        } else {
            *pending
        };

        self.vec
            .iter()
            .filter(|keybind| !matches!(keybind.op, Op::ToggleArg(_)))
            .filter(move |keybind| keybind.menu == expected)
    }

    pub(crate) fn arg_list<'a>(
        &'a self,
        pending: &'a PendingMenu,
    ) -> impl Iterator<Item = &'a Binding> {
        let expected = if pending.menu == Menu::Help {
            Menu::Root
        } else {
            pending.menu
        };

        self.vec
            .iter()
            .filter(|keybind| {
                if let Op::ToggleArg(ref arg) = keybind.op {
                    pending.args.contains_key(arg.as_str())
                } else {
                    false
                }
            })
            .filter(move |keybind| keybind.menu == expected)
    }
}

pub(crate) struct Binding {
    pub menu: Menu,
    pub raw: String,
    pub keys: Vec<(KeyModifiers, KeyCode)>,
    pub op: Op,
}

impl Binding {
    /// Attempt to parse the key combination passed in. If it fails, `None` is
    /// returned; the caller should report this to the user.
    pub fn parse(menu: Menu, raw_keys: &str, op: Op) -> Option<Self> {
        if let Ok(("", keys)) = key_parser::parse_config_keys(raw_keys) {
            Some(Self {
                menu,
                raw: raw_keys.to_string(),
                keys,
                op,
            })
        } else {
            None
        }
    }
}
