use std::{ops::Range, path::PathBuf, rc::Rc};

use crate::{Res, error::Error, git::diff::Diff, gitu_diff::Status, highlight::BlameHighlights};

#[derive(Clone, Debug)]
pub(crate) enum ItemData {
    Raw(String),
    AllUnstaged(usize),
    AllStaged(usize),
    AllUntracked(Vec<PathBuf>),
    Reference {
        prefix: &'static str,
        kind: Ref,
        short_id: String,
        summary: String,
        merge_status: Option<BranchMergeStatus>,
    },
    Commit {
        oid: String,
        short_id: String,
        associated_references: Vec<Ref>,
        summary: String,
        author: String,
        age: String,
    },
    Untracked(PathBuf),
    Delta {
        diff: Rc<Diff>,
        file_i: usize,
        commit: Option<String>,
    },
    Hunk {
        diff: Rc<Diff>,
        file_i: usize,
        hunk_i: usize,
    },
    HunkLine {
        diff: Rc<Diff>,
        file_i: usize,
        hunk_i: usize,
        line_i: usize,
        line_range: Range<usize>,
    },
    Stash {
        message: String,
        stash_ref: String,
        id: usize,
    },
    Header(SectionHeader),
    BranchStatus(String, u32, u32),
    Error(String),
    BlameHeader {
        commit_hash: String,
        short_hash: String,
        _author: String,
        _author_time: i64,
        summary: String,
        file_path: String,
        line_num: u32, // orig line in the introducing commit (for show-screen nav)
        blamed_line_num: u32, // line number in the blamed file (for blame-view nav)
    },
    BlameCodeLine {
        blame_file: Rc<BlameFile>,
        line_i: usize,
        line_num: u32,
        orig_line_num: u32,
        content: String,
        commit_hash: String,
        file_path: String,
    },
}

#[derive(Clone, Debug)]
pub(crate) enum BranchMergeStatus {
    Merged,
    Unmerged,
}

#[derive(Debug)]
pub(crate) struct BlameFile {
    pub highlights: BlameHighlights,
}

impl ItemData {
    pub(crate) fn is_section(&self) -> bool {
        matches!(
            self,
            ItemData::AllUnstaged(_)
                | ItemData::AllStaged(_)
                | ItemData::AllUntracked(_)
                | ItemData::Untracked(_)
                | ItemData::Delta { .. }
                | ItemData::Hunk { .. }
                | ItemData::Header(_)
                | ItemData::BranchStatus(_, _, _)
        )
    }

    pub(crate) fn rev(&self) -> Option<Rev> {
        match &self {
            ItemData::Reference { kind, .. } => Some(Rev::Ref(kind.clone())),
            ItemData::Commit {
                oid,
                associated_references,
                ..
            } => associated_references
                .first()
                .cloned()
                .map(Rev::Ref)
                .or_else(|| Some(Rev::Commit(oid.to_owned()))),
            ItemData::BlameHeader { commit_hash, .. }
            | ItemData::BlameCodeLine { commit_hash, .. } => Some(Rev::Commit(commit_hash.clone())),
            _ => None,
        }
    }

    pub(crate) fn display_text(&self) -> String {
        match self {
            ItemData::Raw(content) => content.clone(),
            ItemData::AllUnstaged(count) => format!("Unstaged changes ({count})"),
            ItemData::AllStaged(count) => format!("Staged changes ({count})"),
            ItemData::AllUntracked(_) => "Untracked files".into(),
            ItemData::Reference {
                kind,
                prefix,
                short_id,
                summary,
                merge_status,
            } => {
                let mut text = format!("{prefix}{}", kind.shorthand());
                if let Some(merge_status) = merge_status {
                    text.push_str(match merge_status {
                        BranchMergeStatus::Merged => " merged",
                        BranchMergeStatus::Unmerged => " unmerged",
                    });
                }
                if !summary.is_empty() {
                    text.push(' ');
                    text.push_str(summary);
                }
                if !short_id.is_empty() {
                    text.push(' ');
                    text.push_str(short_id);
                }
                text
            }
            ItemData::Commit {
                short_id,
                associated_references,
                summary,
                author,
                age,
                ..
            } => {
                let refs = associated_references
                    .iter()
                    .map(Ref::shorthand)
                    .collect::<Vec<_>>()
                    .join(" ");
                format!("{short_id} {refs} {summary} {author} {age}")
            }
            ItemData::Untracked(path) => path.to_string_lossy().into_owned(),
            ItemData::Delta { diff, file_i, .. } => {
                let file_diff = &diff.file_diffs[*file_i];
                let path = match file_diff.header.status {
                    Status::Renamed | Status::Copied => format!(
                        "{} -> {}",
                        file_diff.header.old_file.fmt(&diff.text),
                        file_diff.header.new_file.fmt(&diff.text)
                    ),
                    Status::Deleted => file_diff.header.old_file.fmt(&diff.text).to_string(),
                    _ => file_diff.header.new_file.fmt(&diff.text).to_string(),
                };

                format!(
                    "{:8}   {path}",
                    format!("{:?}", file_diff.header.status).to_lowercase()
                )
            }
            ItemData::Hunk {
                diff,
                file_i,
                hunk_i,
            } => {
                let hunk = &diff.file_diffs[*file_i].hunks[*hunk_i];
                diff.text[hunk.header.range.clone()].to_string()
            }
            ItemData::HunkLine {
                diff,
                file_i,
                hunk_i,
                line_range,
                ..
            } => diff.hunk_content(*file_i, *hunk_i)[line_range.clone()].replace('\t', "    "),
            ItemData::Stash { message, id, .. } => format!("stash@{id} {message}"),
            ItemData::Header(header) => header.display_text(),
            ItemData::BranchStatus(upstream, ahead, behind) => {
                if *ahead == 0 && *behind == 0 {
                    format!("Your branch is up to date with '{upstream}'.")
                } else if *ahead > 0 && *behind == 0 {
                    format!("Your branch is ahead of '{upstream}' by {ahead} commit(s).")
                } else if *ahead == 0 && *behind > 0 {
                    format!("Your branch is behind '{upstream}' by {behind} commit(s).")
                } else {
                    format!(
                        "Your branch and '{upstream}' have diverged,\nand have {ahead} and {behind} different commits each, respectively."
                    )
                }
            }
            ItemData::Error(err) => err.clone(),
            ItemData::BlameHeader {
                short_hash,
                summary,
                ..
            } => format!("{short_hash:<8} {summary}"),
            ItemData::BlameCodeLine {
                line_num, content, ..
            } => format!("{line_num:>4} {}", content.replace('\t', "    ")),
        }
    }
}

impl SectionHeader {
    pub(crate) fn display_text(&self) -> String {
        match self {
            SectionHeader::Remote(remote) => format!("Remote {remote}"),
            SectionHeader::Tags => "Tags".into(),
            SectionHeader::Branches => "Branches".into(),
            SectionHeader::NoBranch => "No branch".into(),
            SectionHeader::OnBranch(branch) => format!("On branch {branch}"),
            SectionHeader::Rebase(head, onto) => format!("Rebasing {head} onto {onto}"),
            SectionHeader::Merge(head) => format!("Merging {head}"),
            SectionHeader::Revert(head) => format!("Reverting {head}"),
            SectionHeader::CherryPick(head) => format!("Cherry-picking {head}"),
            SectionHeader::Stashes => "Stashes".into(),
            SectionHeader::RecentCommits => "Recent commits".into(),
            SectionHeader::Commit(oid) => format!("commit {oid}"),
            SectionHeader::StashRef(stash_ref) => stash_ref.clone(),
            SectionHeader::StagedChanges(count) => format!("Staged changes ({count})"),
            SectionHeader::UnstagedChanges(count) => format!("Unstaged changes ({count})"),
            SectionHeader::UntrackedFiles(count) => format!("Untracked files ({count})"),
            SectionHeader::Blame(file, commit) => format!("Blame {file} @ {commit}"),
        }
    }
}

impl Default for ItemData {
    fn default() -> Self {
        ItemData::Raw(String::new())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Rev {
    Ref(Ref),
    Commit(String),
}

impl Rev {
    pub(crate) fn from_reference(reference: &git2::Reference<'_>) -> Res<Self> {
        let shorthand = String::from_utf8_lossy(reference.shorthand_bytes()).to_string();

        if reference.is_branch() {
            Ok(Rev::Ref(Ref::Head(shorthand)))
        } else if reference.is_tag() {
            Ok(Rev::Ref(Ref::Tag(shorthand)))
        } else if reference.is_remote() {
            Ok(Rev::Ref(Ref::Remote(shorthand)))
        } else {
            let commit = reference.peel_to_commit().map_err(Error::ReadOid)?;
            Ok(Rev::Commit(commit.id().to_string()))
        }
    }

    pub(crate) fn shorthand(&self) -> &str {
        match self {
            Rev::Ref(r) => r.shorthand(),
            Rev::Commit(c) => c,
        }
    }
}

/// Represent a reference in git, as found in `.git/refs/heads`, `.git/refs/tags`, etc.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Ref {
    Tag(String),
    Head(String),
    Remote(String),
}

impl Ref {
    /// Convert to fully qualified refname (e.g., "refs/heads/main", "refs/tags/v1.0.0")
    pub(crate) fn to_full_refname(&self) -> String {
        match self {
            Ref::Head(name) => format!("refs/heads/{}", name),
            Ref::Tag(name) => format!("refs/tags/{}", name),
            Ref::Remote(name) => format!("refs/remotes/{}", name),
        }
    }

    /// Get the shorthand name without refs/ prefix
    pub(crate) fn shorthand(&self) -> &str {
        match self {
            Ref::Head(name) | Ref::Tag(name) | Ref::Remote(name) => name,
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) enum SectionHeader {
    Remote(String),
    Tags,
    Branches,
    NoBranch,
    OnBranch(String),
    Rebase(String, String),
    Merge(String),
    Revert(String),
    CherryPick(String),
    Stashes,
    RecentCommits,
    Commit(String),
    StashRef(String),
    StagedChanges(usize),
    UnstagedChanges(usize),
    UntrackedFiles(usize),
    Blame(String, String),
}
