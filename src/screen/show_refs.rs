use std::{
    collections::{BTreeMap, HashSet, btree_map::Entry},
    iter,
    rc::Rc,
    sync::Arc,
};

use super::Screen;
use crate::{
    Res,
    config::Config,
    error::Error,
    item_data::{BranchMergeStatus, ItemData, Ref, Rev, SectionHeader},
    items::{self, Item, hash},
};
use git2::{Oid, Reference, Repository};

pub(crate) fn create(config: Arc<Config>, repo: Rc<Repository>, size: (u16, u16)) -> Res<Screen> {
    Screen::new(
        Arc::clone(&config),
        size,
        Box::new(move || {
            let context = ReferenceContext::new(&repo);
            Ok(iter::once(Item {
                id: hash("local_branches"),
                data: ItemData::Header(SectionHeader::Branches),
                depth: 0,
                ..Default::default()
            })
            .chain(
                create_reference_items(&repo, &context, |reference| reference.is_branch())?
                    .into_iter()
                    .map(|(_, item)| item),
            )
            .chain(create_remotes_sections(&repo, &context)?)
            .chain(create_tags_section(&repo, &context)?)
            .collect())
        }),
    )
}

fn create_remotes_sections(
    repo: &Repository,
    context: &ReferenceContext,
) -> Res<impl Iterator<Item = Item>> {
    let all_remotes = create_reference_items(repo, context, |reference| reference.is_remote())?;
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

fn create_tags_section(
    repo: &Repository,
    context: &ReferenceContext,
) -> Res<impl Iterator<Item = Item>> {
    let tags = create_reference_items(repo, context, |reference| reference.is_tag())?;
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

fn create_reference_items<F>(
    repo: &Repository,
    context: &ReferenceContext,
    filter: F,
) -> Res<Vec<(String, Item)>>
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

            let prefix = create_prefix(context, &reference);
            let (short_id, summary) = create_tip_info(&reference);
            let merge_status = create_merge_status(context, &reference, &ref_kind);

            let item = Item {
                id: hash(&name),
                depth: 1,
                data: ItemData::Reference {
                    prefix,
                    kind: ref_kind,
                    short_id,
                    summary,
                    merge_status,
                },
                ..Default::default()
            };
            (name, item)
        })
        .collect::<Vec<_>>();
    refs.sort_by(|(left, _), (right, _)| left.cmp(right));
    Ok(refs)
}

fn create_prefix(context: &ReferenceContext, reference: &Reference) -> &'static str {
    let reference_targets_match = reference.target() == context.head_target;
    if context.head_detached && reference_targets_match {
        return "? ";
    }

    let reference_names_match = reference.name() == context.head_name.as_deref();

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

fn create_merge_status(
    context: &ReferenceContext,
    reference: &Reference,
    ref_kind: &Ref,
) -> Option<BranchMergeStatus> {
    let is_merged = match ref_kind {
        Ref::Head(_) | Ref::Remote(_) => is_reference_merged_to_head(context, reference)?,
        Ref::Tag(_) => return None,
    };

    Some(if is_merged {
        BranchMergeStatus::Merged
    } else {
        BranchMergeStatus::Unmerged
    })
}

fn is_reference_merged_to_head(context: &ReferenceContext, reference: &Reference) -> Option<bool> {
    let commit = reference.peel_to_commit().ok()?;
    Some(context.merged_oids.contains(&commit.id()))
}

struct ReferenceContext {
    head_detached: bool,
    head_name: Option<String>,
    head_target: Option<Oid>,
    merged_oids: HashSet<Oid>,
}

impl ReferenceContext {
    fn new(repo: &Repository) -> Self {
        let head = repo.head().ok();
        let head_target = head.as_ref().and_then(Reference::target);
        let head_name = head
            .as_ref()
            .and_then(|reference| reference.name())
            .map(ToOwned::to_owned);
        let head_detached = repo.head_detached().unwrap_or(false);
        let merged_oids = merged_oids(repo, head.as_ref());

        Self {
            head_detached,
            head_name,
            head_target,
            merged_oids,
        }
    }
}

fn merged_oids(repo: &Repository, head: Option<&Reference>) -> HashSet<Oid> {
    let Some(head_commit) = head.and_then(|reference| reference.peel_to_commit().ok()) else {
        return HashSet::new();
    };

    let mut merged_oids = HashSet::from([head_commit.id()]);
    let Ok(mut revwalk) = repo.revwalk() else {
        return merged_oids;
    };
    if revwalk.push(head_commit.id()).is_err() {
        return merged_oids;
    }

    merged_oids.extend(revwalk.filter_map(Result::ok));
    merged_oids
}
