use super::*;

fn input(trigger: AutomationTrigger) -> NewAutomation {
    NewAutomation { name: "edited".into(), trigger, action: AutomationAction::Broadcast { body: "new prompt".into() }, max_runs: None, max_consecutive_failures: Some(4), overlap_policy: OverlapPolicy::Skip }
}

#[test]
fn edit_preserves_running_receipts_counters_and_original_dispatch() {
    let manager = AutomationManager::new();
    let mut original = input(AutomationTrigger::Interval { minutes: 10 });
    original.action = AutomationAction::Broadcast { body: "original prompt".into() };
    let item = manager.create(original, 100).unwrap();
    let dispatched = manager.collect_due(600_100).unwrap();
    let before = serde_json::to_value(&manager.list()[0]).unwrap();
    let updated = manager.update(&item.id, input(AutomationTrigger::Interval { minutes: 10 }), 610_100).unwrap();
    let after = serde_json::to_value(&updated).unwrap();
    for field in ["id","enabled","runCount","consecutiveFailures","createdAt","lastRunAt","nextRunAt","recentRuns","requestReceipts","activeSessionIds"] { assert_eq!(before[field], after[field], "{field}"); }
    assert_eq!(manager.list().len(), 1);
    assert!(matches!(&dispatched[0].2, AutomationAction::Broadcast { body } if body == "original prompt"));
    assert!(matches!(updated.action, AutomationAction::Broadcast { body } if body == "new prompt"));
}

#[test]
fn edit_once_rearms_only_for_a_new_future_time_without_resetting_history() {
    let manager = AutomationManager::new();
    let item = manager.create(input(AutomationTrigger::Once { at: 1000 }), 1).unwrap();
    assert_eq!(manager.collect_due(1000).unwrap().len(), 1);
    manager.report_result(&item.id, true, "done".into(), 1001).unwrap();
    let edited = manager.update(&item.id, input(AutomationTrigger::Once { at: 1000 }), 2000).unwrap();
    assert_eq!(edited.next_run_at, None);
    assert!(manager.collect_due(2001).unwrap().is_empty());
    assert!(manager.update(&item.id, input(AutomationTrigger::Once { at: 1500 }), 2000).is_err());
    let next = manager.update(&item.id, input(AutomationTrigger::Once { at: 5000 }), 2000).unwrap();
    assert_eq!(next.run_count, 1); assert!(!next.recent_runs.is_empty());
    manager.set_enabled(&item.id, false, 2100).unwrap();
    assert_eq!(manager.set_enabled(&item.id, true, 2200).unwrap().next_run_at, Some(5000));
    assert!(manager.collect_due(4999).unwrap().is_empty());
    assert_eq!(manager.collect_due(5000).unwrap().len(), 1);
    assert!(manager.collect_due(6000).unwrap().is_empty());
}

#[test]
fn edit_recalculates_schedule_limits_and_keeps_disabled_state() {
    let manager = AutomationManager::new();
    let item = manager.create(input(AutomationTrigger::Interval { minutes: 1 }), 1).unwrap();
    manager.collect_due(60_001).unwrap();
    let mut edited = input(AutomationTrigger::Interval { minutes: 2 }); edited.max_runs = Some(1);
    assert_eq!(manager.update(&item.id, edited.clone(), 61_000).unwrap().next_run_at, None);
    edited.max_runs = Some(3);
    assert_eq!(manager.update(&item.id, edited.clone(), 62_000).unwrap().next_run_at, Some(182_000));
    manager.set_enabled(&item.id, false, 63_000).unwrap();
    edited.trigger = AutomationTrigger::Daily { hour: 9, minute: 0 };
    let saved = manager.update(&item.id, edited, 64_000).unwrap();
    assert!(!saved.enabled); assert_eq!(saved.next_run_at, None); assert_eq!(saved.run_count, 1);
}

#[test]
fn edit_persists_and_failed_save_or_invalid_input_never_replaces_the_plan() {
    let root = std::env::temp_dir().join(format!("automation-edit-{}", uuid::Uuid::new_v4()));
    let path = root.join("automations.json");
    let manager = AutomationManager::open_at_time(path.clone(), 1).unwrap();
    let item = manager.create(input(AutomationTrigger::Daily { hour: 9, minute: 0 }), 1).unwrap();
    let mut edited = input(AutomationTrigger::Weekly { weekdays: vec![1,3,5], hour: 11, minute: 30 });
    edited.name = "saved edit".into();
    let saved = manager.update(&item.id, edited.clone(), 2000).unwrap();
    let reopened = AutomationManager::open_at_time(path.clone(), 2001).unwrap().list();
    assert_eq!(serde_json::to_value(&saved).unwrap(), serde_json::to_value(&reopened[0]).unwrap());
    let before = serde_json::to_value(manager.list()).unwrap();
    edited.name.clear(); assert!(manager.update(&item.id, edited.clone(), 3000).is_err());
    edited.name = "must not replace".into(); assert!(manager.update("deleted", edited.clone(), 3000).is_err());
    std::fs::remove_file(&path).unwrap(); std::fs::create_dir(&path).unwrap();
    assert!(manager.update(&item.id, edited, 3000).is_err());
    assert_eq!(serde_json::to_value(manager.list()).unwrap(), before);
    std::fs::remove_dir_all(root).unwrap();
}
