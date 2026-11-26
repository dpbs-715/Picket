//! C-Push merge conflict, Agent merge, and manual IDE reconciliation localization.

use crate::i18n::{LanguageKind, join_list, tr};

pub const fn cpush_conflict_badge(locale: LanguageKind) -> &'static str {
    tr(locale, "Conflict", "冲突")
}

pub const fn cpush_option_agent_merge(locale: LanguageKind) -> &'static str {
    tr(locale, "Merge with Agent", "让 Agent 合并")
}

pub const fn cpush_option_manual_merge(locale: LanguageKind) -> &'static str {
    tr(locale, "Merge manually", "我手动合并")
}

pub const fn cpush_option_abort_rebase(locale: LanguageKind, has_commits: bool) -> &'static str {
    match (locale, has_commits) {
        (LanguageKind::En, true) => "Abort rebase (keep local commits)",
        (LanguageKind::En, false) => "Abort rebase and restore",
        (LanguageKind::Zh, true) => "中止变基（保留本地提交）",
        (LanguageKind::Zh, false) => "中止变基并恢复原状",
    }
}

pub fn cpush_timeline_sync_conflict(
    locale: LanguageKind,
    upstream: &str,
    names: &[&str],
) -> String {
    let joined = join_list(locale, names);
    match locale {
        LanguageKind::En => format!("Conflict while syncing {upstream}: {joined}"),
        LanguageKind::Zh => format!("同步 {upstream} 时遇到冲突：{joined}"),
    }
}

pub fn cpush_conflict_description(
    locale: LanguageKind,
    upstream: &str,
    names: &[&str],
    extra_reason: Option<&str>,
) -> String {
    let joined = join_list(locale, names);
    match locale {
        LanguageKind::En => {
            let base = format!(
                "Code conflict in {joined} while rebasing onto {upstream}. Click any file to inspect in your editor, then choose an action or enter merge instructions:"
            );
            match extra_reason.map(str::trim).filter(|s| !s.is_empty()) {
                Some(reason) => format!("{reason}. {base}"),
                None => base,
            }
        }
        LanguageKind::Zh => {
            let base = format!(
                "变基至 {upstream} 时 {joined} 发生代码冲突。可点击文件在编辑器中查看冲突，选择处理方式或输入合并要求："
            );
            match extra_reason.map(str::trim).filter(|s| !s.is_empty()) {
                Some(reason) => format!("{reason}。{base}"),
                None => base,
            }
        }
    }
}

pub const fn cpush_conflict_title(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Merge conflict while syncing remote changes",
        "同步远端改动遇到代码冲突",
    )
}

pub fn cpush_conflict_scope(locale: LanguageKind, upstream: &str) -> String {
    match locale {
        LanguageKind::En => format!("Stage: rebase {upstream}"),
        LanguageKind::Zh => format!("阶段：rebase {upstream}"),
    }
}

pub const fn cpush_restart_conflict_reason(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Unresolved merge conflicts were found after restart; writes did not continue automatically",
        "应用重启后检测到未完成的合并冲突，未自动继续写入",
    )
}

pub fn cpush_manual_still_conflicted_desc(locale: LanguageKind, names: &[&str]) -> String {
    let joined = join_list(locale, names);
    match locale {
        LanguageKind::En => format!(
            "Unresolved conflicts remain in {joined}. Continue resolving in your external editor and finish rebase before switching back."
        ),
        LanguageKind::Zh => {
            format!("检测到 {joined} 仍存在未解决冲突，请在外部编辑器继续处理并完成变基后切回。")
        }
    }
}

pub const fn cpush_manual_rebase_incomplete_desc(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Conflict markers are cleared, but rebase is still in progress; finish rebase (`git rebase --continue`) in your IDE or terminal and switch back.",
        "冲突标记已清除，但变基尚未结束；请在 IDE 或终端完成变基（git rebase --continue）后切回。",
    )
}

pub const fn cpush_qa_manual_ext_question(locale: LanguageKind) -> &'static str {
    tr(locale, "Merge conflict resolution", "合并冲突处理")
}

pub const fn cpush_qa_manual_ext_answer(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Merged externally and pushed in terminal",
        "在外部完成合并并在终端推送",
    )
}

pub fn cpush_timeline_manual_resolved_and_pushed(locale: LanguageKind, display: &str) -> String {
    match locale {
        LanguageKind::En => {
            format!("Detected conflicts resolved and pushed to {display} in terminal")
        }
        LanguageKind::Zh => format!("检测到冲突已解决且终端已推送至 {display}"),
    }
}

pub const fn cpush_timeline_manual_rebase_done(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Detected manual conflict merge and rebase completed",
        "检测到手动冲突合并与变基已完成",
    )
}

pub const fn cpush_manual_rebase_done_title(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Conflict resolved, rebase completed",
        "冲突已解决，变基已完成",
    )
}

pub const fn cpush_manual_rebase_done_desc(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Manual conflict merge and rebase completed externally; standalone pull does not push automatically. Trigger push explicitly when ready.",
        "已在外部完成冲突合并与变基；单独发起的拉取不包含自动推送权限，如需推送请明确触发。",
    )
}

pub const fn cpush_aborted_rebase_reason(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Rebase aborted; local commits were kept intact",
        "已中止变基，本地提交保持原状",
    )
}

pub const fn cpush_aborted_rebase_title(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Rebase aborted, local commits kept",
        "已中止变基，保留本地提交",
    )
}

pub const fn cpush_timeline_chose_manual_merge(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Selected: resolve merge conflicts manually in external editor",
        "已选择：前往外部编辑器手动合并冲突",
    )
}

pub const fn cpush_timeline_waiting_return_reconcile(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Writes paused; waiting to reconcile Git and remote status when you return to Picket",
        "已暂停写入操作，等待切回 Picket 后自动核对 Git 与远端状态",
    )
}

pub const fn cpush_waiting_manual_title(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Waiting for manual merge in IDE",
        "等待你在 IDE 完成手动合并",
    )
}

pub const fn cpush_waiting_manual_desc(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Resolve conflicts and finish rebase in your external editor, then switch back to Picket to reconcile status automatically.",
        "请在外部编辑器解决冲突并完成变基后切回 Picket，桌面层将自动核对状态。",
    )
}

pub const fn cpush_waiting_manual_scope(locale: LanguageKind) -> &'static str {
    tr(locale, "Stage: merge conflict", "阶段：合并冲突")
}

pub const fn cpush_enter_merge_instruction_error(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Enter merge instructions or choose a resolution option above",
        "请输入具体的冲突合并要求或选择上方处理方式",
    )
}

pub const fn cpush_agent_merge_no_model_error(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "No model is configured or selected for Agent merge; add a model first or choose \"Merge manually\".",
        "当前尚未配置或选择模型，无法执行 Agent 合并；请先添加模型或选择「我手动合并」。",
    )
}

pub fn cpush_timeline_authorized_agent_merge(locale: LanguageKind, answer_text: &str) -> String {
    match locale {
        LanguageKind::En => format!("Authorized Agent conflict merge: {answer_text}"),
        LanguageKind::Zh => format!("已授权 Agent 处理冲突：{answer_text}"),
    }
}

pub fn cpush_timeline_merging_files(locale: LanguageKind, count: usize) -> String {
    match locale {
        LanguageKind::En => {
            let noun = if count == 1 { "file" } else { "files" };
            format!("Merging {count} conflicted {noun} and validating markers…")
        }
        LanguageKind::Zh => format!("正在合并 {count} 个冲突文件并校验结果…"),
    }
}

pub const fn cpush_progress_merging_title(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Agent is merging conflicted files…",
        "Agent 正在合并冲突文件…",
    )
}

pub fn cpush_progress_merging_desc(locale: LanguageKind, instruction: &str) -> String {
    if instruction.is_empty() {
        tr(
            locale,
            "Modifying only the conflicted files; conflict markers will be verified before continuing rebase.",
            "仅修改当前冲突文件，合并后将校验冲突标记并继续变基。",
        )
        .into()
    } else {
        match locale {
            LanguageKind::En => {
                format!("Merging conflicted files with instruction \"{instruction}\"…")
            }
            LanguageKind::Zh => format!("正在按要求「{instruction}」合并冲突文件…"),
        }
    }
}

pub fn cpush_merging_scope(locale: LanguageKind, upstream: &str) -> String {
    match locale {
        LanguageKind::En => format!("Stage: merge {upstream}"),
        LanguageKind::Zh => format!("阶段：合并 {upstream}"),
    }
}

pub const fn cpush_progress_merging_steps(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Merge conflict -> Verify markers -> Continue rebase",
        "冲突合并 → 校验标记 → 完成变基",
    )
}

pub fn cpush_merge_resolver_failed_desc(locale: LanguageKind, err: &str) -> String {
    match locale {
        LanguageKind::En => format!(
            "{err}. Conflicted files remain unchanged; adjust instructions to retry or choose \"Merge manually\"."
        ),
        LanguageKind::Zh => {
            format!("{err}。冲突文件保持原状，可调整要求重试或选择「我手动合并」。")
        }
    }
}

pub fn cpush_merge_apply_failed_desc(locale: LanguageKind, err: &str) -> String {
    match locale {
        LanguageKind::En => format!(
            "{err}. Subsequent steps were paused; inspect conflict state in your editor or merge manually."
        ),
        LanguageKind::Zh => {
            format!("{err}。未自动继续后续步骤，请在编辑器核对冲突状态或手动合并。")
        }
    }
}

pub const fn cpush_next_commit_conflict_reason(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Current commit conflict was merged; rebasing the next commit encountered another conflict",
        "当前提交冲突已合并，下一笔提交变基时遇到新的冲突",
    )
}

pub fn cpush_timeline_agent_merge_done(locale: LanguageKind, summary: &str) -> String {
    match locale {
        LanguageKind::En => format!("Agent merged conflicts: {summary}"),
        LanguageKind::Zh => format!("Agent 已完成冲突合并：{summary}"),
    }
}

pub fn cpush_timeline_rebase_done_no_push(locale: LanguageKind, upstream: &str) -> String {
    match locale {
        LanguageKind::En => format!("Rebased onto {upstream} (not auto-pushed)"),
        LanguageKind::Zh => format!("已完成变基至 {upstream}（未自动推送）"),
    }
}

pub fn cpush_done_merge_pull_title(locale: LanguageKind, upstream: &str) -> String {
    match locale {
        LanguageKind::En => format!("Merged conflicts and rebased onto {upstream}"),
        LanguageKind::Zh => format!("已完成冲突合并并变基至 {upstream}"),
    }
}

pub fn cpush_done_merge_pull_desc(locale: LanguageKind, summary: &str) -> String {
    match locale {
        LanguageKind::En => format!(
            "{summary}. Standalone pull does not push automatically; remaining working tree drafts were preserved."
        ),
        LanguageKind::Zh => {
            format!("{summary}。单独发起的拉取不自动推送远端，工作区其余草稿均完整保留。")
        }
    }
}

pub const fn cpush_conflict_cancelled_error(locale: LanguageKind) -> &'static str {
    tr(
        locale,
        "Agent conflict merge cancelled",
        "已取消 Agent 冲突合并",
    )
}
