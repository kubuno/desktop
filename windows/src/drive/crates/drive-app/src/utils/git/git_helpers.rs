//! GitHelpers (mirrors GitHelpers.cs)
//!
//! Port of `Files.App/Utils/Git/GitHelpers.cs`: repository detection,
//! current branch, ahead/behind, branch list, checkout/create/delete,
//! and the network operations pull/push/sync. The original relies on
//! LibGit2Sharp; we use `git2` (the Rust bindings for libgit2).
//!
//! The network operations (fetch/pull/push/sync) are blocking: the
//! caller runs them off the UI thread (see `MainWindow` / `GitWorker`).

use git2::{
    AnnotatedCommit, AutotagOption, BranchType, Cred, CredentialType, FetchOptions, MergeAnalysis,
    PushOptions, RemoteCallbacks, Repository,
};

use crate::data::items::branch_item::BranchItem;

/// A folder's git state: the head (current branch) and its divergence
/// from the tracked upstream. `None` outside a repository.
#[derive(Debug, Clone, Default)]
pub struct GitHeadInfo {
    pub branch: String,
    pub ahead: usize,
    pub behind: usize,
}

/// Opens the repository that CONTAINS `path` (walks up the tree, like
/// `GitHelpers.GetGitRepositoryPath`). `None` if `path` isn't versioned.
pub fn discover(path: &str) -> Option<Repository> {
    Repository::discover(path).ok()
}

/// The current branch and its ahead/behind vs the tracked upstream
/// (`DirectoryPropertiesViewModel.UpdateGitInfo` + `GitHelpers`). `None`
/// outside a repo or with a detached HEAD (the original then hides all
/// git UI).
pub fn head_info(path: &str) -> Option<GitHeadInfo> {
    let repo = discover(path)?;
    if repo.head_detached().unwrap_or(true) {
        return None;
    }
    let head = repo.head().ok()?;
    if !head.is_branch() {
        return None;
    }
    let branch = head.shorthand()?.to_string();
    let (ahead, behind) = ahead_behind(&repo, &head).unwrap_or((0, 0));
    Some(GitHeadInfo { branch, ahead, behind })
}

/// `graph_ahead_behind(local, upstream)` of the head vs its tracked upstream.
fn ahead_behind(repo: &Repository, head: &git2::Reference) -> Option<(usize, usize)> {
    let local_oid = head.target()?;
    let local_branch = repo
        .find_branch(head.shorthand()?, BranchType::Local)
        .ok()?;
    let upstream = local_branch.upstream().ok()?;
    let upstream_oid = upstream.get().target()?;
    repo.graph_ahead_behind(local_oid, upstream_oid).ok()
}

/// All local branches then remote ones, head first
/// (`GitHelpers.GetBranchesNames`). Ahead/behind is only populated for
/// the head (like the original, which only computes divergence for the
/// active branch).
pub fn branches(path: &str) -> Vec<BranchItem> {
    let Some(repo) = discover(path) else {
        return Vec::new();
    };
    let head_name = repo
        .head()
        .ok()
        .filter(|h| h.is_branch())
        .and_then(|h| h.shorthand().map(str::to_owned));

    // `GetBranchNames`: per group (local / remote), sorted by head commit
    // date DESCENDING, capped at 30, then the head floated to the top.
    const MAX_NUMBER_OF_BRANCHES: usize = 30;
    let mut items: Vec<BranchItem> = Vec::new();
    for (kind, remote) in [(BranchType::Local, false), (BranchType::Remote, true)] {
        let Ok(iter) = repo.branches(Some(kind)) else {
            continue;
        };
        // (name, seconds-of-the-head-commit) for sorting by recency.
        let mut group: Vec<(BranchItem, i64)> = Vec::new();
        for (branch, _) in iter.flatten() {
            let Ok(Some(name)) = branch.name() else {
                continue;
            };
            // libgit2 names the pseudo-branch "origin/HEAD": we skip it,
            // as the original's selector does.
            if remote && name.ends_with("/HEAD") {
                continue;
            }
            let is_head = !remote && Some(name) == head_name.as_deref();
            let (ahead, behind) = if is_head {
                repo.head()
                    .ok()
                    .and_then(|h| ahead_behind(&repo, &h))
                    .unwrap_or((0, 0))
            } else {
                (0, 0)
            };
            let when = branch
                .get()
                .peel_to_commit()
                .map(|c| c.committer().when().seconds())
                .unwrap_or(0);
            group.push((
                BranchItem { name: name.to_string(), is_head, is_remote: remote, ahead_by: ahead, behind_by: behind },
                when,
            ));
        }
        group.sort_by_key(|g| std::cmp::Reverse(g.1)); // descending head date
        items.extend(group.into_iter().take(MAX_NUMBER_OF_BRANCHES).map(|(b, _)| b));
    }
    // Head first (ACTIVE_BRANCH_INDEX = 0).
    items.sort_by_key(|b| !b.is_head);
    items
}

/// `GitHelpers.CheckoutBranch`: switches the working tree + HEAD to
/// `branch` (local, or a remote one materialized as a tracking local).
pub fn checkout(path: &str, branch: &str) -> Result<(), git2::Error> {
    let repo = discover(path).ok_or_else(no_repo)?;
    // A remote branch "origin/x" is checked out by creating/pointing the
    // local "x" that tracks it — otherwise we point the local directly.
    let (local_name, refname) = match repo.find_branch(branch, BranchType::Local) {
        Ok(b) => (branch.to_string(), b.into_reference().name().unwrap_or("").to_string()),
        Err(_) => {
            // Remote: materializes the tracking local.
            let short = branch.rsplit_once('/').map_or(branch, |(_, s)| s).to_string();
            let remote_ref = repo.find_branch(branch, BranchType::Remote)?;
            let commit = remote_ref.get().peel_to_commit()?;
            let mut local = match repo.find_branch(&short, BranchType::Local) {
                Ok(b) => b,
                Err(_) => {
                    let mut b = repo.branch(&short, &commit, false)?;
                    b.set_upstream(Some(branch))?;
                    b
                }
            };
            let name = local.get().name().unwrap_or("").to_string();
            // avoids a dangling borrow
            let _ = &mut local;
            (short, name)
        }
    };
    let obj = repo.revparse_single(&format!("refs/heads/{local_name}"))?;
    repo.checkout_tree(&obj, None)?;
    repo.set_head(&refname)?;
    Ok(())
}

/// `GitHelpers.CreateNewBranch`: creates `name` on the head and checks it out.
pub fn create_branch(path: &str, name: &str) -> Result<(), git2::Error> {
    let repo = discover(path).ok_or_else(no_repo)?;
    let head = repo.head()?.peel_to_commit()?;
    repo.branch(name, &head, false)?;
    checkout(path, name)
}

/// `GitHelpers.DeleteBranch`: deletes the local branch `name`. Ready on
/// the service side; the UI gesture (the original's custom
/// `BranchesFlyout` per-row trash button) remains to be wired up — the
/// current branch flyout is a menu, not yet the per-row-buttons ListView.
#[allow(dead_code)]
pub fn delete_branch(path: &str, name: &str) -> Result<(), git2::Error> {
    let repo = discover(path).ok_or_else(no_repo)?;
    let mut branch = repo.find_branch(name, BranchType::Local)?;
    branch.delete()
}

/// Authentication callbacks: first the credential helper (stored HTTPS),
/// then the SSH agent, finally the default identity — the range libgit2
/// offers to reproduce the original's provider without its GitHub OAuth.
fn remote_callbacks<'a>() -> RemoteCallbacks<'a> {
    let mut cb = RemoteCallbacks::new();
    cb.credentials(|url, username, allowed| {
        if allowed.contains(CredentialType::SSH_KEY) {
            if let Some(user) = username {
                return Cred::ssh_key_from_agent(user);
            }
        }
        if allowed.contains(CredentialType::USER_PASS_PLAINTEXT) {
            if let Ok(config) = git2::Config::open_default() {
                if let Ok(cred) = Cred::credential_helper(&config, url, username) {
                    return Ok(cred);
                }
            }
        }
        Cred::default()
    });
    cb
}

/// The upstream (remote) of the current branch, otherwise "origin".
fn head_remote(repo: &Repository) -> Result<String, git2::Error> {
    if let Ok(head) = repo.head() {
        if let Some(name) = head.shorthand() {
            let key = format!("branch.{name}.remote");
            if let Ok(cfg) = repo.config() {
                if let Ok(remote) = cfg.get_string(&key) {
                    return Ok(remote);
                }
            }
        }
    }
    Ok("origin".to_string())
}

/// `GitHelpers.FetchOrigin`: fetches the refs from the head's upstream.
pub fn fetch(path: &str) -> Result<(), git2::Error> {
    let repo = discover(path).ok_or_else(no_repo)?;
    let remote_name = head_remote(&repo)?;
    let mut remote = repo.find_remote(&remote_name)?;
    let mut opts = FetchOptions::new();
    opts.remote_callbacks(remote_callbacks());
    opts.download_tags(AutotagOption::All);
    let refspecs: Vec<String> = remote
        .fetch_refspecs()?
        .iter()
        .flatten()
        .map(str::to_owned)
        .collect();
    remote.fetch(&refspecs, Some(&mut opts), None)?;
    Ok(())
}

/// `GitHelpers.PullOrigin`: fetch then integration (fast-forward if
/// possible, otherwise merge) of the tracked upstream into the head.
pub fn pull(path: &str) -> Result<(), git2::Error> {
    fetch(path)?;
    let repo = discover(path).ok_or_else(no_repo)?;
    let head_ref = repo.head()?;
    let branch_name = head_ref.shorthand().ok_or_else(no_repo)?.to_string();
    let upstream = repo
        .find_branch(&branch_name, BranchType::Local)?
        .upstream()?;
    let fetch_commit = repo.reference_to_annotated_commit(upstream.get())?;
    merge(&repo, &branch_name, &fetch_commit)
}

/// `GitHelpers.PushToOrigin`: pushes the head to its upstream.
pub fn push(path: &str) -> Result<(), git2::Error> {
    let repo = discover(path).ok_or_else(no_repo)?;
    let remote_name = head_remote(&repo)?;
    let mut remote = repo.find_remote(&remote_name)?;
    let head = repo.head()?;
    let refname = head.name().ok_or_else(no_repo)?;
    let refspec = format!("{refname}:{refname}");
    let mut opts = PushOptions::new();
    opts.remote_callbacks(remote_callbacks());
    remote.push(&[refspec.as_str()], Some(&mut opts))
}

/// `GitSyncAction`: pull (fetch + integration) THEN push — the push is
/// launched via `ContinueWith`, so unconditionally (even if pull fails).
pub fn sync(path: &str) -> Result<(), git2::Error> {
    let _ = pull(path);
    push(path)
}

/// Merges `fetch_commit` into branch `branch_name`: libgit2 analysis →
/// fast-forward (ref move + checkout) or normal merge.
fn merge(
    repo: &Repository,
    branch_name: &str,
    fetch_commit: &AnnotatedCommit,
) -> Result<(), git2::Error> {
    let (analysis, _) = repo.merge_analysis(&[fetch_commit])?;
    if analysis.contains(MergeAnalysis::ANALYSIS_UP_TO_DATE) {
        return Ok(());
    }
    if analysis.contains(MergeAnalysis::ANALYSIS_FASTFORWARD) {
        let refname = format!("refs/heads/{branch_name}");
        let mut reference = repo.find_reference(&refname)?;
        reference.set_target(fetch_commit.id(), "pull: fast-forward")?;
        repo.set_head(&refname)?;
        repo.checkout_head(Some(git2::build::CheckoutBuilder::default().force()))?;
        return Ok(());
    }
    // Normal merge: indexes the result, creates the merge commit.
    let head_commit = repo.reference_to_annotated_commit(&repo.head()?)?;
    let local_tree = repo.find_commit(head_commit.id())?.tree()?;
    let remote_tree = repo.find_commit(fetch_commit.id())?.tree()?;
    let ancestor = repo
        .find_commit(repo.merge_base(head_commit.id(), fetch_commit.id())?)?
        .tree()?;
    let mut idx = repo.merge_trees(&ancestor, &local_tree, &remote_tree, None)?;
    if idx.has_conflicts() {
        repo.checkout_index(Some(&mut idx), None)?;
        return Ok(());
    }
    let result_tree = repo.find_tree(idx.write_tree_to(repo)?)?;
    let sig = repo.signature()?;
    let local_commit = repo.find_commit(head_commit.id())?;
    let remote_commit = repo.find_commit(fetch_commit.id())?;
    repo.commit(
        Some("HEAD"),
        &sig,
        &sig,
        &format!("Merge branch into {branch_name}"),
        &result_tree,
        &[&local_commit, &remote_commit],
    )?;
    repo.checkout_head(Some(git2::build::CheckoutBuilder::default().force()))?;
    Ok(())
}

fn no_repo() -> git2::Error {
    git2::Error::from_str("not a git repository")
}
