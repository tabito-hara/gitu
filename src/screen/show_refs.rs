use std::{
    collections::{BTreeMap, btree_map::Entry},
    iter,
    rc::Rc,
    sync::Arc,
};

use super::Screen;
use crate::{
    Res,
    config::Config,
    error::Error,
    item_data::{ItemData, Rev, SectionHeader},
    items::{self, Item, hash},
};
use git2::{Reference, Repository};

pub(crate) fn create(config: Arc<Config>, repo: Rc<Repository>, size: (u16, u16)) -> Res<Screen> {
    Screen::new(
        Arc::clone(&config),
        size,
        Box::new(move || {
            Ok(iter::once(Item {
                id: hash("local_branches"),
                data: ItemData::Header(SectionHeader::Branches),
                depth: 0,
                ..Default::default()
            })
            .chain(
                create_reference_items(&repo, |reference| reference.is_branch())?
                    .into_iter()
                    .map(|(_, item)| item),
            )
            .chain(create_remotes_sections(&repo)?)
            .chain(create_tags_section(&repo)?)
            .collect())
        }),
    )
}

fn create_remotes_sections(repo: &Repository) -> Res<impl Iterator<Item = Item>> {
    let all_remotes = create_reference_items(repo, |reference| reference.is_remote())?;
    let mut remotes = BTreeMap::new();
    for (name, remote) in all_remotes {
        let name =
            String::from_utf8_lossy(&repo.branch_remote_name(&name).map_err(Error::GetRemote)?)
                .to_string();

        match remotes.entry(name) {
            Entry::Vacant(entry) => {
                entry.insert(vec![remote]);
            }
            Entry::Occupied(mut entry) => {
                entry.get_mut().push(remote);
            }
        }
    }

    Ok(remotes.into_iter().flat_map(move |(name, items)| {
        vec![
            items::blank_line(),
            Item {
                id: hash(&name),
                depth: 0,
                data: ItemData::Header(SectionHeader::Remote(name)),
                ..Default::default()
            },
        ]
        .into_iter()
        .chain(items)
    }))
}

fn create_tags_section(repo: &Repository) -> Res<impl Iterator<Item = Item>> {
    let tags = create_reference_items(repo, |reference| reference.is_tag())?;
    Ok(match tags.first().cloned() {
        Some((_name, item)) => vec![
            items::blank_line(),
            Item {
                id: hash("tags"),
                depth: 0,
                data: ItemData::Header(SectionHeader::Tags),
                ..Default::default()
            },
            item,
        ],
        None => vec![],
    }
    .into_iter()
    .chain(tags.into_iter().skip(1).map(|(_name, item)| item)))
}

fn create_reference_items<F>(repo: &Repository, filter: F) -> Res<Vec<(String, Item)>>
where
    F: FnMut(&Reference<'_>) -> bool,
{
    let mut refs = repo
        .references()
        .map_err(Error::ListGitReferences)?
        .filter_map(Result::ok)
        .filter(filter)
        .map(move |reference| {
            let name = reference.name().unwrap().to_owned();
            let Rev::Ref(ref_kind) = Rev::from_reference(&reference).unwrap() else {
                unreachable!("This should be a reference")
            };

            let prefix = create_prefix(repo, &reference);
            let (short_id, summary) = create_tip_info(&reference);

            let item = Item {
                id: hash(&name),
                depth: 1,
                data: ItemData::Reference {
                    prefix,
                    kind: ref_kind,
                    short_id,
                    summary,
                },
                ..Default::default()
            };
            (name, item)
        })
        .collect::<Vec<_>>();
    refs.sort_by(|(left, _), (right, _)| left.cmp(right));
    Ok(refs)
}

fn create_prefix(repo: &Repository, reference: &Reference) -> &'static str {
    let head = repo.head().ok();

    let head_detached = repo.head_detached().unwrap_or(false);
    let reference_targets_match = reference.target() == head.as_ref().and_then(Reference::target);
    if head_detached && reference_targets_match {
        return "? ";
    }

    let reference_names_match = reference.name() == head.as_ref().and_then(Reference::name);

    if reference_names_match {
        return "* ";
    }

    "  "
}

fn create_tip_info(reference: &Reference) -> (String, String) {
    let Ok(commit) = reference.peel_to_commit() else {
        return (String::new(), String::new());
    };

    let short_id = commit
        .as_object()
        .short_id()
        .map(|buf| String::from_utf8_lossy(&buf).to_string())
        .unwrap_or_default();
    let summary = commit.summary().unwrap_or("").to_string();

    (short_id, summary)
}
