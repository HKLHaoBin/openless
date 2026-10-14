//! Exercises the real wire dispatcher against an isolated, disposable sandbox.
use crate::{credentials::HarmonyCredentials, dispatch, Host};
use openless_core::{BackendConfig, BackendDependencies, OpenLessBackend};
use serde_json::{json, Value};
use std::{
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

pub fn styles(owner: &Host) -> Result<Value, String> {
    let root = owner.backend.config().cache_dir.join(format!(
        "native-selftest-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_nanos()
    ));
    std::fs::create_dir(&root).map_err(|e| e.to_string())?;
    let result = (|| {
        let data = root.join("data");
        std::fs::create_dir(&data).map_err(|e| e.to_string())?;
        let credentials = Arc::new(HarmonyCredentials::new(data.join("channels.json")));
        let mut dependencies = BackendDependencies::unsupported();
        dependencies.credential_store = credentials.clone();
        let config = BackendConfig {
            data_dir: data,
            cache_dir: root.clone(),
            platform: crate::capabilities(),
            ..Default::default()
        };
        let runtime = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;
        let backend =
            OpenLessBackend::new(config.clone(), dependencies).map_err(|e| e.to_string())?;
        runtime
            .block_on(backend.start())
            .map_err(|e| e.to_string())?;
        let test = Host {
            runtime,
            backend,
            credentials,
        };
        // Exercise shared IPC argument names and persistence without touching user data.
        dispatch(&test, "add_learned_vocab", json!({"phrase":"鸿蒙接线测试"}))?;
        dispatch(&test, "add_learned_vocab", json!({"phrase":"鸿蒙接线测试"}))?;
        let vocab = dispatch(&test, "list_vocab", json!({}))?;
        if vocab.as_array().ok_or("invalid vocab list")?.len() != 1 {
            return Err("learned vocabulary did not deduplicate".into());
        }
        let rule = dispatch(
            &test,
            "add_correction_rule",
            json!({"pattern":"测试错词","replacement":"测试正词"}),
        )?;
        let rule_id = rule["id"].as_str().ok_or("missing rule id")?;
        dispatch(
            &test,
            "set_correction_rule_enabled",
            json!({"id":rule_id,"enabled":false}),
        )?;
        let rules = dispatch(&test, "list_correction_rules", json!({}))?;
        if rules[0]["enabled"] != json!(false) {
            return Err("correction rule disable did not persist".into());
        }
        dispatch(&test, "remove_correction_rule", json!({"id":rule_id}))?;
        if dispatch(&test, "list_correction_rules", json!({}))? != json!([]) {
            return Err("correction rule delete failed".into());
        }
        let mut presets = dispatch(&test, "list_vocab_presets", json!({}))?;
        presets["disabledBuiltinPresetIds"] = json!(["harmony-selftest"]);
        dispatch(&test, "save_vocab_presets", json!({"store":presets}))?;
        if dispatch(&test, "list_vocab_presets", json!({}))? != presets {
            return Err("vocabulary presets roundtrip failed".into());
        }
        let list = dispatch(&test, "list_style_packs", json!({}))?;
        let mut template = list
            .as_array()
            .and_then(|a| a.first())
            .cloned()
            .ok_or("no builtin styles")?;
        template["name"] = json!("Harmony isolated self-test");
        let mut created = dispatch(
            &test,
            "create_style_pack_from_template",
            json!({"template":template}),
        )?;
        let id = created["id"].as_str().ok_or("missing style id")?.to_owned();
        created["prompt"] = json!("Harmony persistence and ZIP roundtrip test.");
        dispatch(&test, "save_style_pack", json!({"stylePack":created}))?;
        dispatch(
            &test,
            "preview_style_pack_runtime",
            json!({"stylePack":created}),
        )?;
        dispatch(&test, "set_active_style_pack", json!({"id":id}))?;
        if test.backend.get_preferences().active_style_pack_id != id {
            return Err("activation did not update preferences".into());
        }
        dispatch(
            &test,
            "set_style_pack_enabled",
            json!({"id":id,"enabled":false}),
        )?;
        let zip = root.join("test.zip");
        dispatch(
            &test,
            "export_style_pack_to_zip",
            json!({"id":id,"targetPath":zip}),
        )?;
        let imported = dispatch(&test, "import_style_pack_from_zip", json!({"zipPath":zip}))?;
        if imported["prompt"] != created["prompt"] {
            return Err("ZIP roundtrip changed prompt".into());
        }
        let imported_id = imported["id"]
            .as_str()
            .ok_or("missing imported id")?
            .to_owned();
        dispatch(&test, "delete_style_pack", json!({"id":id}))?;
        if test.backend.get_style_pack(&id).is_ok() {
            return Err("deleted style still present".into());
        }
        let reopened = OpenLessBackend::new(config, BackendDependencies::unsupported())
            .map_err(|e| e.to_string())?;
        if reopened.list_vocabulary().map_err(|e| e.to_string())?.len() != 1
            || json!(reopened
                .list_vocabulary_presets()
                .map_err(|e| e.to_string())?)
                != presets
        {
            return Err("vocabulary data did not survive repository reopen".into());
        }
        if reopened
            .get_style_pack(&imported_id)
            .map_err(|e| e.to_string())?
            .prompt
            != created["prompt"].as_str().ok_or("missing prompt")?
        {
            return Err("saved prompt did not survive repository reopen".into());
        }
        Ok(
            json!({"create":true,"save":true,"preview":true,"activate":true,"enabled":true,"export":true,"import":true,"delete":true,"persistence":true,"learnedVocabulary":true,"correctionRules":true,"vocabularyPresets":true}),
        )
    })();
    if result.is_ok() {
        std::fs::remove_dir_all(root).map_err(|e| e.to_string())?;
    }
    result
}
