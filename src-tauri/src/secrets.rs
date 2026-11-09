use anyhow::Result;
use keyring::{Entry, Error};

const SERVICE_NAME: &str = "CrewVela";

// Do not silently compile Linux against keyring's in-memory mock backend if a
// dependency feature is accidentally removed. This type requires Secret Service
// to be the selected default; declaring it performs no keychain or D-Bus I/O.
#[cfg(target_os = "linux")]
const _: Option<keyring::default::SsCredential> = None;

/// 读取 keychain 中的 API key；不存在返回 None。
pub fn get(account: &str) -> Result<Option<String>> {
    if crate::test_profile::active() { return isolated_get(account); }
    get_from(&SystemStore, account)
}

// First-run test keys are process-local: no keychain read, migration or delete.
fn isolated_keys() -> &'static std::sync::Mutex<std::collections::HashMap<String,String>> {
    static KEYS:std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<String,String>>>=std::sync::OnceLock::new();
    KEYS.get_or_init(Default::default)
}
fn isolated_get(account: &str) -> Result<Option<String>> { Ok(isolated_keys().lock().map_err(|_|anyhow::anyhow!("测试凭据锁损坏"))?.get(account).cloned()) }
fn isolated_set(account: &str, secret: &str) -> Result<()> { isolated_keys().lock().map_err(|_|anyhow::anyhow!("测试凭据锁损坏"))?.insert(account.into(),secret.into());Ok(()) }
fn isolated_delete(account: &str) -> Result<()> { isolated_keys().lock().map_err(|_|anyhow::anyhow!("测试凭据锁损坏"))?.remove(account);Ok(()) }

trait CredentialStore {
    fn get(&self, service: &str, account: &str) -> Result<Option<String>>;
    fn set(&self, service: &str, account: &str, secret: &str) -> Result<()>;
    fn delete(&self, service: &str, account: &str) -> Result<()>;
}

struct SystemStore;
impl CredentialStore for SystemStore {
    fn get(&self, service: &str, account: &str) -> Result<Option<String>> {
    match entry(service, account)?.get_password() {
        Ok(secret) => Ok(Some(secret)),
        Err(Error::NoEntry) => Ok(None),
        Err(e) => Err(anyhow::anyhow!("读取密钥失败: {e}")),
    }
    }
    fn set(&self, service: &str, account: &str, secret: &str) -> Result<()> {
        entry(service, account)?.set_password(secret)?;
        Ok(())
    }
    fn delete(&self, service: &str, account: &str) -> Result<()> {
        match entry(service, account)?.delete_credential() {
            Ok(()) | Err(Error::NoEntry) => Ok(()),
            Err(error) => Err(anyhow::anyhow!("删除密钥失败: {error}")),
        }
    }
}

fn get_from(store: &impl CredentialStore, account: &str) -> Result<Option<String>> {
    if let Some(secret) = store.get(SERVICE_NAME, account)? { return Ok(Some(secret)); }
    let previous = store.get(crate::brand::LEGACY_NAME, account)?;
    if let Some(secret) = &previous {
        if store.set(SERVICE_NAME, account, secret).is_err() {
            // Preserve functionality without exposing the credential or claiming
            // migration succeeded. A later read retries the persistent write.
            eprintln!("[CrewVela] 凭据迁移写入失败，仍使用旧凭据；下次读取将重试");
        }
    }
    Ok(previous)
}

pub fn set(account: &str, secret: &str) -> Result<()> {
    if crate::test_profile::active() { return isolated_set(account,secret); }
    set_in(&SystemStore, account, secret)
}

fn set_in(store: &impl CredentialStore, account: &str, secret: &str) -> Result<()> {
    let legacy_exists = store.get(crate::brand::LEGACY_NAME, account)?.is_some();
    let previous = store.get(SERVICE_NAME, account)?;
    store.set(SERVICE_NAME, account, secret)?;
    if legacy_exists && store.set(crate::brand::LEGACY_NAME, account, secret).is_err() {
        let rollback = match previous {
            Some(previous) => store.set(SERVICE_NAME, account, &previous),
            None => store.delete(SERVICE_NAME, account),
        };
        if rollback.is_err() { anyhow::bail!("旧凭据同步及新凭据回滚失败，请重新检查凭据状态"); }
        anyhow::bail!("旧凭据同步失败，新凭据已回滚，原配置保留");
    }
    Ok(())
}

pub fn delete(account: &str) -> Result<()> {
    if crate::test_profile::active() { return isolated_delete(account); }
    delete_from(&SystemStore, account)
}

fn delete_from(store: &impl CredentialStore, account: &str) -> Result<()> {
    // Remove fallback first; otherwise a failed legacy deletion resurrects a key.
    store.delete(crate::brand::LEGACY_NAME, account)?;
    store.delete(SERVICE_NAME, account)
}

fn entry(service: &str, account: &str) -> Result<Entry> {
    Entry::new(service, account).map_err(|e| anyhow::anyhow!("创建 keychain 条目失败: {e}"))
}

#[cfg(all(test, any(target_os = "linux", target_os = "windows", target_os = "macos")))]
mod tests {
    use super::*;
    use std::{cell::RefCell, collections::HashMap};

    #[test]
    fn isolated_test_keys_have_no_system_store_or_legacy_fallback() {
        let key=format!("isolated-{}",uuid::Uuid::new_v4());
        assert!(isolated_get(&key).unwrap().is_none());
        isolated_set(&key,"synthetic-test-value").unwrap();
        assert_eq!(isolated_get(&key).unwrap().as_deref(),Some("synthetic-test-value"));
        isolated_delete(&key).unwrap();assert!(isolated_get(&key).unwrap().is_none());
    }

    #[derive(Default)]
    struct Store { values: RefCell<HashMap<(String, String), String>>, fail_write: bool, fail_write_old: bool, fail_delete_old: bool, fail_read: bool }
    impl CredentialStore for Store {
        fn get(&self, service: &str, account: &str) -> Result<Option<String>> {
            if self.fail_read { anyhow::bail!("fixture read failure"); }
            Ok(self.values.borrow().get(&(service.into(), account.into())).cloned())
        }
        fn set(&self, service: &str, account: &str, secret: &str) -> Result<()> {
            if self.fail_write || (self.fail_write_old && service == crate::brand::LEGACY_NAME) { anyhow::bail!("fixture write failure"); }
            self.values.borrow_mut().insert((service.into(), account.into()), secret.into()); Ok(())
        }
        fn delete(&self, service: &str, account: &str) -> Result<()> {
            if self.fail_delete_old && service == crate::brand::LEGACY_NAME { anyhow::bail!("fixture delete failure"); }
            self.values.borrow_mut().remove(&(service.into(), account.into())); Ok(())
        }
    }
    #[test]
    fn brand_migration_credentials_are_lazy_idempotent_and_new_values_win() {
        let store = Store::default();
        store.set(crate::brand::LEGACY_NAME, "fixture-account", "fixture-old-value").unwrap();
        assert_eq!(get_from(&store, "fixture-account").unwrap().as_deref(), Some("fixture-old-value"));
        assert_eq!(store.get(SERVICE_NAME, "fixture-account").unwrap().as_deref(), Some("fixture-old-value"));
        store.set(SERVICE_NAME, "fixture-account", "fixture-current-value").unwrap();
        assert_eq!(get_from(&store, "fixture-account").unwrap().as_deref(), Some("fixture-current-value"));
        delete_from(&store, "fixture-account").unwrap();
        assert_eq!(get_from(&store, "fixture-account").unwrap(), None);
    }
    #[test]
    fn brand_migration_credential_failures_preserve_data_and_never_resurrect_deleted_keys() {
        let mut store = Store::default();
        store.set(crate::brand::LEGACY_NAME, "fixture", "fixture-value").unwrap();
        store.fail_write = true;
        assert_eq!(get_from(&store, "fixture").unwrap().as_deref(), Some("fixture-value"));
        assert_eq!(store.get(SERVICE_NAME, "fixture").unwrap(), None);
        store.fail_write = false;
        store.set(SERVICE_NAME, "fixture", "fixture-new").unwrap();
        store.fail_delete_old = true;
        assert!(delete_from(&store, "fixture").is_err());
        assert_eq!(store.get(SERVICE_NAME, "fixture").unwrap().as_deref(), Some("fixture-new"));
        store.fail_read = true;
        assert!(get_from(&store, "fixture").is_err());
    }
    #[test]
    fn brand_migration_updates_existing_aliases_and_rolls_back_failed_legacy_writes() {
        let mut store = Store::default();
        store.set(crate::brand::LEGACY_NAME, "fixture", "fixture-before").unwrap();
        set_in(&store, "fixture", "fixture-after").unwrap();
        assert_eq!(store.get(crate::brand::LEGACY_NAME, "fixture").unwrap().as_deref(), Some("fixture-after"));
        store.fail_write_old = true;
        assert!(set_in(&store, "fixture", "fixture-rejected").is_err());
        assert_eq!(get_from(&store, "fixture").unwrap().as_deref(), Some("fixture-after"));
        assert_eq!(store.get(crate::brand::LEGACY_NAME, "fixture").unwrap().as_deref(), Some("fixture-after"));
        store.fail_write_old = false;
        set_in(&store, "new-fixture", "fixture-new").unwrap();
        assert_eq!(store.get(crate::brand::LEGACY_NAME, "new-fixture").unwrap(), None);
    }
    #[test]
    fn default_backend_is_persistent_without_accessing_any_credentials() {
        // The backend builder is just type metadata. Do not build an Entry or
        // call get/set/delete here: those would touch the user's credential store.
        assert!(matches!(
            keyring::default::default_credential_builder().persistence(),
            keyring::credential::CredentialPersistence::UntilDelete,
        ));
    }
}
