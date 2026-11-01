//! OS-vault key + authenticated encryption. Never fall back to plaintext.
use super::Error;
use octave_provider::account::{Account, Identity};
use ring::{
    aead,
    rand::{SecureRandom, SystemRandom},
};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};
use zeroize::Zeroizing;

pub(super) trait Keys {
    fn read(&self) -> Result<Option<Zeroizing<Vec<u8>>>, Error>;
    fn write(&self, key: &[u8]) -> Result<(), Error>;
    fn remove(&self) -> Result<(), Error>;
}
pub(super) struct NativeKeys {
    name: String,
}
impl NativeKeys {
    pub fn for_path(path: &Path) -> Self {
        let hash = ring::digest::digest(&ring::digest::SHA256, path.to_string_lossy().as_bytes());
        Self {
            name: hash.as_ref().iter().map(|b| format!("{b:02x}")).collect(),
        }
    }
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    fn entry(&self) -> Result<keyring::Entry, Error> {
        keyring::Entry::new("Octave.Account.v1", &self.name).map_err(|_| Error::Vault)
    }
}
impl Keys for NativeKeys {
    fn read(&self) -> Result<Option<Zeroizing<Vec<u8>>>, Error> {
        #[cfg(any(target_os = "windows", target_os = "macos"))]
        {
            match self.entry()?.get_secret() {
                Ok(v) => Ok(Some(Zeroizing::new(v))),
                Err(keyring::Error::NoEntry) => Ok(None),
                Err(_) => Err(Error::Vault),
            }
        }
        #[cfg(not(any(target_os = "windows", target_os = "macos")))]
        {
            let _ = &self.name;
            Err(Error::VaultUnavailable)
        }
    }
    fn write(&self, key: &[u8]) -> Result<(), Error> {
        #[cfg(any(target_os = "windows", target_os = "macos"))]
        {
            self.entry()?.set_secret(key).map_err(|_| Error::Vault)
        }
        #[cfg(not(any(target_os = "windows", target_os = "macos")))]
        {
            let _ = key;
            Err(Error::VaultUnavailable)
        }
    }
    fn remove(&self) -> Result<(), Error> {
        #[cfg(any(target_os = "windows", target_os = "macos"))]
        {
            match self.entry()?.delete_credential() {
                Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
                Err(_) => Err(Error::Vault),
            }
        }
        #[cfg(not(any(target_os = "windows", target_os = "macos")))]
        {
            // This build cannot create a native key. Forget must still clean
            // browser/session files for a memory-only account.
            Ok(())
        }
    }
}
pub(super) struct Store<K = NativeKeys> {
    pub root: PathBuf,
    keys: K,
}
impl Store<NativeKeys> {
    pub fn new(root: PathBuf) -> Self {
        Self {
            keys: NativeKeys::for_path(&root),
            root,
        }
    }
}
impl<K: Keys> Store<K> {
    pub fn exists(&self) -> bool {
        self.root.join("session.sealed").is_file()
    }
    fn prepare(&self) -> Result<(), Error> {
        // Auth data must never be created in a source checkout, even when a
        // caller explicitly supplies --data-dir inside one.
        if self.root.ancestors().any(|p| p.join(".git").exists()) {
            return Err(Error::UnsafeProfile);
        }
        fs::create_dir_all(&self.root).map_err(|_| Error::Storage)?;
        if self
            .root
            .canonicalize()
            .map_err(|_| Error::Storage)?
            .ancestors()
            .any(|p| p.join(".git").exists())
        {
            return Err(Error::UnsafeProfile);
        }
        if fs::symlink_metadata(&self.root)
            .map_err(|_| Error::Storage)?
            .file_type()
            .is_symlink()
        {
            return Err(Error::UnsafeProfile);
        }
        Ok(())
    }
    pub fn save(&self, account: &Account, identity: &Identity) -> Result<(), Error> {
        self.prepare()?;
        let cookie = account.session_cookie().map_err(|_| Error::Cancelled)?;
        let escaped =
            Zeroizing::new(serde_json::to_string(cookie.as_str()).map_err(|_| Error::Storage)?);
        let identity_json = serde_json::to_string(&identity.id).map_err(|_| Error::Storage)?;
        let delegated_json =
            serde_json::to_string(&account.delegated_id()).map_err(|_| Error::Storage)?;
        let text = Zeroizing::new(
            format!(
                "{{\"v\":2,\"cookie\":{},\"index\":{},\"identity\":{},\"delegated\":{}}}",
                escaped.as_str(),
                account.account_index(),
                identity_json,
                delegated_json
            )
            .into_bytes(),
        );
        let prior = self.keys.read()?;
        let fresh = prior.is_none();
        let key = if let Some(key) = prior {
            key
        } else {
            let mut key = Zeroizing::new(vec![0; 32]);
            SystemRandom::new()
                .fill(&mut key)
                .map_err(|_| Error::Vault)?;
            self.keys.write(&key)?;
            key
        };
        let result = (|| {
            let unbound =
                aead::UnboundKey::new(&aead::AES_256_GCM, &key).map_err(|_| Error::Vault)?;
            let mut nonce = [0; 12];
            SystemRandom::new()
                .fill(&mut nonce)
                .map_err(|_| Error::Vault)?;
            let mut sealed = Zeroizing::new(text.to_vec());
            aead::LessSafeKey::new(unbound)
                .seal_in_place_append_tag(
                    aead::Nonce::assume_unique_for_key(nonce),
                    aead::Aad::from(b"Octave.Account.v1"),
                    &mut *sealed,
                )
                .map_err(|_| Error::Vault)?;
            let temp = self.root.join("session.sealed.pending");
            if temp.exists() {
                fs::remove_file(&temp).map_err(|_| Error::Storage)?;
            }
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temp)
                .map_err(|_| Error::Storage)?;
            file.write_all(b"OCTAUTH1")
                .and_then(|_| file.write_all(&nonce))
                .and_then(|_| file.write_all(&sealed))
                .and_then(|_| file.sync_all())
                .map_err(|_| Error::Storage)?;
            drop(file);
            fs::rename(&temp, self.root.join("session.sealed")).map_err(|_| Error::Storage)
        })();
        if result.is_err() && fresh {
            self.keys.remove()?;
        }
        result
    }
    pub fn restore(&self) -> Result<(Account, String), Error> {
        self.prepare()?;
        let file = self.root.join("session.sealed");
        let meta = fs::symlink_metadata(&file).map_err(|_| Error::Storage)?;
        if !meta.is_file() || meta.len() > 32768 || meta.len() < 36 {
            return Err(Error::Storage);
        }
        let mut sealed = Zeroizing::new(fs::read(file).map_err(|_| Error::Storage)?);
        if &sealed[..8] != b"OCTAUTH1" {
            return Err(Error::Storage);
        }
        let nonce: [u8; 12] = sealed[8..20].try_into().map_err(|_| Error::Storage)?;
        let key = self.keys.read()?.ok_or(Error::Vault)?;
        let key = aead::LessSafeKey::new(
            aead::UnboundKey::new(&aead::AES_256_GCM, &key).map_err(|_| Error::Vault)?,
        );
        let opened = key
            .open_in_place(
                aead::Nonce::assume_unique_for_key(nonce),
                aead::Aad::from(b"Octave.Account.v1"),
                &mut sealed[20..],
            )
            .map_err(|_| Error::Vault)?;
        let mut value: serde_json::Value =
            serde_json::from_slice(opened).map_err(|_| Error::Storage)?;
        if value["v"] != 2 {
            return Err(Error::Storage);
        }
        let cookie = match value["cookie"].take() {
            serde_json::Value::String(text) => Zeroizing::new(text),
            _ => return Err(Error::Storage),
        };
        let index = value["index"]
            .as_u64()
            .filter(|i| *i < 100)
            .ok_or(Error::Storage)? as u8;
        let expected = value["identity"]
            .as_str()
            .filter(|s| !s.is_empty() && s.len() <= 128)
            .ok_or(Error::Storage)?
            .to_owned();
        value["cookie"] = serde_json::Value::Null;
        let delegated = match &value["delegated"] {
            serde_json::Value::Null => None,
            serde_json::Value::String(id) => Some(id.as_str()),
            _ => return Err(Error::Storage),
        };
        let account = Account::restore_selection(cookie.to_string(), index, &expected, delegated)
            .map_err(|_| Error::Storage)?;
        Ok((account, expected))
    }
    pub fn forget(&self) -> Result<(), Error> {
        self.prepare()?;
        let mut failed = false;
        for name in ["session.sealed", "session.sealed.pending"] {
            match fs::remove_file(self.root.join(name)) {
                Ok(()) => (),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
                Err(_) => failed = true,
            }
        }
        let key = self.keys.remove();
        if failed { Err(Error::Storage) } else { key }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    };
    #[derive(Default)]
    struct FakeKeys {
        key: Mutex<Option<Vec<u8>>>,
        fail: AtomicBool,
    }
    impl Keys for FakeKeys {
        fn read(&self) -> Result<Option<Zeroizing<Vec<u8>>>, Error> {
            Ok(self.key.lock().unwrap().clone().map(Zeroizing::new))
        }
        fn write(&self, k: &[u8]) -> Result<(), Error> {
            if self.fail.load(Ordering::Relaxed) {
                return Err(Error::Vault);
            }
            *self.key.lock().unwrap() = Some(k.to_vec());
            Ok(())
        }
        fn remove(&self) -> Result<(), Error> {
            if self.fail.load(Ordering::Relaxed) {
                return Err(Error::Vault);
            }
            *self.key.lock().unwrap() = None;
            Ok(())
        }
    }
    fn root() -> PathBuf {
        std::env::var_os("OCTAVE_TEST_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir)
            .join(format!(
                "vault-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ))
    }
    #[test]
    fn sealed_storage_roundtrip_tamper_and_complete_forget() {
        let store = Store {
            root: root(),
            keys: FakeKeys::default(),
        };
        let account = Account::new("SAPISID=synthetic-vault-secret".into(), 2).unwrap();
        let identity = Identity {
            id: "fixture-id".into(),
            name: "Fixture".into(),
            channel_id: None,
            handle: None,
            premium: None,
        };
        store.save(&account, &identity).unwrap();
        let mut bytes = fs::read(store.root.join("session.sealed")).unwrap();
        assert!(!String::from_utf8_lossy(&bytes).contains("synthetic-vault-secret"));
        let (restored, expected) = store.restore().unwrap();
        assert_eq!(expected, "fixture-id");
        assert_eq!(restored.account_index(), 2);
        assert_eq!(
            restored.session_cookie().unwrap().as_str(),
            "SAPISID=synthetic-vault-secret"
        );
        let end = bytes.len() - 1;
        bytes[end] ^= 1;
        fs::write(store.root.join("session.sealed"), bytes).unwrap();
        assert!(store.restore().is_err());
        store.forget().unwrap();
        assert!(!store.exists());
        assert!(store.keys.read().unwrap().is_none());
        fs::remove_dir(&store.root).unwrap();
    }
    #[test]
    fn failed_vault_never_creates_a_plaintext_fallback_and_failed_forget_is_visible() {
        let store = Store {
            root: root(),
            keys: FakeKeys::default(),
        };
        store.keys.fail.store(true, Ordering::Relaxed);
        let account = Account::new("SAPISID=synthetic".into(), 0).unwrap();
        let identity = Identity {
            id: "fixture".into(),
            name: "Fixture".into(),
            channel_id: None,
            handle: None,
            premium: None,
        };
        assert_eq!(store.save(&account, &identity), Err(Error::Vault));
        assert!(!store.exists());
        assert_eq!(store.forget(), Err(Error::Vault));
        fs::remove_dir(&store.root).unwrap();
    }
    #[test]
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    #[ignore = "Explicit synthetic native OS-vault write/read/delete; uses a unique test entry"]
    fn native_vault_roundtrip_and_erasure() {
        let store = Store::new(root());
        assert!(!store.exists());
        assert!(store.keys.read().unwrap().is_none());
        let account = Account::new("SAPISID=synthetic-native-vault-only".into(), 0).unwrap();
        let identity = Identity {
            id: "native-fixture".into(),
            name: "Fixture".into(),
            channel_id: None,
            handle: None,
            premium: None,
        };
        store.save(&account, &identity).unwrap();
        let (restored, expected) = store.restore().unwrap();
        assert_eq!(expected, "native-fixture");
        assert_eq!(
            restored.session_cookie().unwrap().as_str(),
            "SAPISID=synthetic-native-vault-only"
        );
        store.forget().unwrap();
        assert!(!store.exists());
        assert!(store.keys.read().unwrap().is_none());
        fs::remove_dir(&store.root).unwrap();
    }
    #[test]
    fn persistence_refuses_a_source_checkout() {
        let root = root();
        fs::create_dir_all(root.join(".git")).unwrap();
        let store = Store {
            root: root.join("account"),
            keys: FakeKeys::default(),
        };
        assert_eq!(store.prepare(), Err(Error::UnsafeProfile));
        assert!(!store.root.exists());
        fs::remove_dir(root.join(".git")).unwrap();
        fs::remove_dir(root).unwrap();
    }
}
