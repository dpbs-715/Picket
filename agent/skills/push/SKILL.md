---
name: push
description: Use only when explicitly invoked as push or called by a coordinating skill such as cpush. Push local commits, automatically integrating remote updates when only unpublished local commits need rebasing.
---

# Git Push

Push local commits to the resolved remote branch. Invocation authorizes fetching,
rebasing unpublished local commits under the conditions below, and normal pushes
without another confirmation. Honor explicit restrictions such as "do not rebase"
or "push only". Keep published history and uncommitted work intact.

## Workflow

1. Inspect `git status --short --branch`, the current branch, upstream, remote
   URLs, and push configuration. Stop for detached HEAD or an existing merge,
   rebase, cherry-pick, or revert operation; do not take over that operation.
2. Resolve one push remote and destination branch from the user's request and
   repository configuration, including push-specific remote/refspec settings.
   If the destination is ambiguous, ask before pushing. Do not assume `origin`,
   a same-named branch, or that the upstream and push destination are identical.
3. Record existing remote-tracking tips as publication evidence, then fetch the
   resolved destination and refresh its remote-tracking ref.
4. Push explicitly to the resolved branch when fast-forwardable; apply automatic
   rebase integration only for linear, unpublished local commits.
5. Never use `--force` or `-f` by default. Preserve retained local commits when
   pushing is rejected or blocked, and report the exact blocker.
