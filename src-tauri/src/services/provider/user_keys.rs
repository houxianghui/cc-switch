//! Claude Code 用户级配置键（live-owned keys）
//!
//! 这组键归 `~/.claude/settings.json` 现有内容所有：供应商卡片与通用配置
//! 片段都不再捕获或注入它们。写入 live 时把现有值原样带过，回填卡片时剥掉，
//! 保证切换/保存/同步任何路径都不会丢失或复活用户在 live 里维护的配置
//! （典型如 hooks）。

use serde_json::Value;

use crate::config::{get_claude_settings_path, read_json_file};

/// 用户级键清单。新增键前先确认它**不**携带供应商路由/凭据语义：
/// `env` / `model` / 端点类键归供应商卡片所有，不得加入。
pub(crate) const CLAUDE_USER_OWNED_KEYS: &[&str] = &[
    "hooks",
    "statusLine",
    "permissions",
    "enabledPlugins",
    "includeCoAuthoredBy",
    "outputStyle",
    "theme",
    "verbose",
    "autoUpdates",
    "cleanupPeriodDays",
    "preferredNotifChannel",
    "alwaysThinkingEnabled",
    "spinnerTipsEnabled",
    "terminalProgressBarEnabled",
];

/// 从配置里剥掉全部用户级键（原地修改，非对象时 no-op）。
pub(crate) fn strip_user_owned_keys(value: &mut Value) {
    let Some(obj) = value.as_object_mut() else {
        return;
    };
    for key in CLAUDE_USER_OWNED_KEYS {
        obj.remove(*key);
    }
}

/// 把 `live` 里存在的用户级键覆盖到 `outgoing` 上（原地修改）。
pub(crate) fn merge_user_owned_keys_from(outgoing: &mut Value, live: &Value) {
    let Some(live_obj) = live.as_object() else {
        return;
    };
    let Some(out_obj) = outgoing.as_object_mut() else {
        return;
    };
    for key in CLAUDE_USER_OWNED_KEYS {
        if let Some(value) = live_obj.get(*key) {
            out_obj.insert((*key).to_string(), value.clone());
        }
    }
}

/// 写 `~/.claude/settings.json` 前调用：读出现有 live（best-effort，读不到
/// 就当没有），把其中的用户级键带到即将写入的内容上。live 里已被用户删除
/// 的键不会被带回——删除语义因此成立。
pub(crate) fn preserve_claude_user_owned_keys_from_live(outgoing: &mut Value) {
    let path = get_claude_settings_path();
    if !path.exists() {
        return;
    }
    match read_json_file::<Value>(&path) {
        Ok(live) => merge_user_owned_keys_from(outgoing, &live),
        Err(err) => {
            log::warn!("读取 Claude live 配置以保留用户级键失败，按无保留键处理: {err}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use serial_test::serial;
    use std::env;

    #[test]
    fn strip_removes_all_user_owned_keys() {
        let mut value = json!({
            "hooks": {"PreToolUse": []},
            "statusLine": {"type": "command", "command": "st"},
            "permissions": {"allow": ["Bash(ls)"]},
            "enabledPlugins": {"a": true},
            "includeCoAuthoredBy": false,
            "outputStyle": "Explanatory",
            "theme": "dark",
            "verbose": true,
            "autoUpdates": false,
            "cleanupPeriodDays": 30,
            "preferredNotifChannel": "iterm2",
            "alwaysThinkingEnabled": true,
            "spinnerTipsEnabled": false,
            "terminalProgressBarEnabled": true,
            "env": {"ANTHROPIC_AUTH_TOKEN": "sk-xxx"},
            "model": "claude-sonnet-5"
        });
        strip_user_owned_keys(&mut value);
        let obj = value.as_object().expect("object");
        assert_eq!(obj.len(), 2, "only provider-owned keys should remain");
        assert!(obj.contains_key("env"));
        assert!(obj.contains_key("model"));
    }

    #[test]
    fn strip_tolerates_non_object() {
        let mut value = json!(["hooks"]);
        strip_user_owned_keys(&mut value);
        assert_eq!(value, json!(["hooks"]));
    }

    #[test]
    fn merge_overlays_only_keys_present_in_live() {
        let mut outgoing = json!({
            "env": {"ANTHROPIC_BASE_URL": "https://api.example"},
            "hooks": {"SessionStart": []}
        });
        let live = json!({
            "hooks": {"PreToolUse": [{"matcher": "Bash", "hooks": []}]},
            "theme": "light"
        });
        merge_user_owned_keys_from(&mut outgoing, &live);
        assert_eq!(
            outgoing["hooks"],
            json!({"PreToolUse": [{"matcher": "Bash", "hooks": []}]}),
            "live hooks must replace card hooks"
        );
        assert_eq!(outgoing["theme"], json!("light"));
        assert!(
            outgoing.get("statusLine").is_none(),
            "keys absent from live must not appear"
        );
        assert_eq!(
            outgoing["env"]["ANTHROPIC_BASE_URL"],
            json!("https://api.example"),
            "provider-owned keys are untouched"
        );
    }

    #[test]
    fn merge_tolerates_non_object_live() {
        let mut outgoing = json!({"hooks": {"SessionStart": []}});
        merge_user_owned_keys_from(&mut outgoing, &json!("garbage"));
        assert_eq!(outgoing, json!({"hooks": {"SessionStart": []}}));
    }

    #[test]
    #[serial]
    fn preserve_reads_keys_from_live_file() {
        let temp = tempfile::tempdir().expect("tempdir");
        let old_test_home = env::var_os("CC_SWITCH_TEST_HOME");
        let old_home = env::var_os("HOME");
        env::set_var("CC_SWITCH_TEST_HOME", temp.path());
        env::set_var("HOME", temp.path());
        crate::settings::reload_settings().expect("reload settings");

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let claude_dir = temp.path().join(".claude");
            std::fs::create_dir_all(&claude_dir).expect("create .claude");
            std::fs::write(
                claude_dir.join("settings.json"),
                serde_json::to_string(&json!({
                    "hooks": {"PreToolUse": []},
                    "env": {"ANTHROPIC_AUTH_TOKEN": "live-token"}
                }))
                .expect("serialize"),
            )
            .expect("write live");

            let mut outgoing = json!({"env": {"ANTHROPIC_AUTH_TOKEN": "card-token"}});
            preserve_claude_user_owned_keys_from_live(&mut outgoing);

            assert_eq!(outgoing["hooks"], json!({"PreToolUse": []}));
            assert_eq!(
                outgoing["env"]["ANTHROPIC_AUTH_TOKEN"],
                json!("card-token"),
                "live env must NOT leak into outgoing (provider-owned)"
            );
        }));

        match old_test_home {
            Some(value) => env::set_var("CC_SWITCH_TEST_HOME", value),
            None => env::remove_var("CC_SWITCH_TEST_HOME"),
        }
        match old_home {
            Some(value) => env::set_var("HOME", value),
            None => env::remove_var("HOME"),
        }
        if let Err(payload) = result {
            std::panic::resume_unwind(payload);
        }
    }

    #[test]
    #[serial]
    fn preserve_is_noop_when_live_missing() {
        let temp = tempfile::tempdir().expect("tempdir");
        let old_test_home = env::var_os("CC_SWITCH_TEST_HOME");
        let old_home = env::var_os("HOME");
        env::set_var("CC_SWITCH_TEST_HOME", temp.path());
        env::set_var("HOME", temp.path());
        crate::settings::reload_settings().expect("reload settings");

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut outgoing = json!({"hooks": {"SessionStart": []}});
            preserve_claude_user_owned_keys_from_live(&mut outgoing);
            assert_eq!(outgoing, json!({"hooks": {"SessionStart": []}}));
        }));

        match old_test_home {
            Some(value) => env::set_var("CC_SWITCH_TEST_HOME", value),
            None => env::remove_var("CC_SWITCH_TEST_HOME"),
        }
        match old_home {
            Some(value) => env::set_var("HOME", value),
            None => env::remove_var("HOME"),
        }
        if let Err(payload) = result {
            std::panic::resume_unwind(payload);
        }
    }
}
