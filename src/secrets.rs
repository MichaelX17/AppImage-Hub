//! Local storage for the optional GitHub token.
//!
//! The token is stored in the OS keyring (Secret Service on Linux/KDE) when a
//! keyring is available. If it is not, the token is written to an
//! Argon2id-encrypted file in the XDG config directory with mode `0600`.
//!
//! It is never written to `config.toml` and never stored beside a portable
//! AppImage: portable folders are meant to be copied or uploaded, so
//! credentials stay in `$XDG_CONFIG_HOME/appimage-manager` even in portable
//! mode.
//!
//! On-disk format v2: `"AIHT2" | salt[16] | nonce[12] | AES-256-GCM
//! ciphertext+tag`. The 32-byte key is `Argon2id(machine-binding material,
//! salt)`; the machine-binding material (`/etc/machine-id` + user + home) is a
//! *binding*, not a secret, and the random salt means a copied file cannot be
//! decrypted from the machine id alone without 64 MiB of Argon2id work.

use anyhow::{anyhow, Context, Result};
use argon2::{Algorithm, Argon2, Params, Version};
use ring::aead;
use ring::digest;
use ring::rand::{SecureRandom, SystemRandom};
use std::path::{Path, PathBuf};
use zeroize::{Zeroize, Zeroizing};

const MAGIC_V2: &[u8; 5] = b"AIHT2";
const MAGIC_V1: &[u8; 5] = b"AIHT1";
const SALT_LEN: usize = 16;
const NONCE_LEN: usize = 12;
const KEY_LEN: usize = 32;
const ARGON2_M_COST_KIB: u32 = 64 * 1024; // 64 MiB
const ARGON2_T_COST: u32 = 3;
const ARGON2_P_COST: u32 = 1;

const KEYRING_SERVICE: &str = "appimage-hub";
const KEYRING_USER: &str = "github-token";

/// Token found on startup plus non-fatal storage problems worth telling the
/// user about (for example a loosened file mode that was tightened).
#[derive(Debug, Default)]
pub struct TokenLoadReport {
    pub token: Option<String>,
    pub warnings: Vec<String>,
}

/// Abstract keyring access so the fallback path is testable without a live
/// Secret Service (CI containers usually have none).
trait KeyringBackend {
    fn get(&self) -> Result<Option<String>>;
    fn set(&self, token: &str) -> Result<()>;
    fn delete(&self) -> Result<()>;
}

struct OsKeyring;

impl KeyringBackend for OsKeyring {
    fn get(&self) -> Result<Option<String>> {
        let entry = keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER)
            .context("system keyring is unavailable")?;
        match entry.get_password() {
            Ok(token) => Ok(Some(token)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(error).context("failed to read the GitHub token from the keyring"),
        }
    }

    fn set(&self, token: &str) -> Result<()> {
        let entry = keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER)
            .context("system keyring is unavailable")?;
        entry
            .set_password(token)
            .context("failed to write the GitHub token to the keyring")
    }

    fn delete(&self) -> Result<()> {
        let entry = keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER)
            .context("system keyring is unavailable")?;
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(error).context("failed to delete the GitHub token from the keyring"),
        }
    }
}

/// Location of the encrypted token file. Deliberately independent of portable
/// mode (D2: always the XDG config directory).
fn token_path() -> PathBuf {
    crate::config::xdg_config_dir().join("github_token.enc")
}

/// A token file that older portable builds wrote next to the AppImage. It is
/// never read or deleted automatically; the user is warned so they can rotate
/// the token and remove the file themselves.
fn portable_token_path() -> PathBuf {
    crate::config::executable_dir()
        .join("AppImageHub.config")
        .join("github_token.enc")
}

/// Saves the token (empty string deletes it) to the best available backend.
pub fn save_github_token(token: &str) -> Result<()> {
    save_token_with(&OsKeyring, &token_path(), token)
}

/// Loads the token from the keyring or, when unavailable, from the encrypted
/// file. Returns `None` when nothing is stored; never panics on corrupt data.
pub fn load_github_token() -> Option<String> {
    load_github_token_report().token
}

/// Loads the token and collects warnings to show at startup.
pub fn load_github_token_report() -> TokenLoadReport {
    let mut report = TokenLoadReport::default();
    if let Some(token) = load_token_with(&OsKeyring, &token_path(), &mut report.warnings) {
        report.token = Some(token);
    }

    let portable = portable_token_path();
    if portable.exists() && portable != token_path() {
        report.warnings.push(format!(
            "An old GitHub token file exists beside the AppImage ({}). \
             If you shared that AppImage, rotate the token and delete that file.",
            portable.display()
        ));
    }
    report
}

fn load_token_with(
    keyring: &dyn KeyringBackend,
    path: &Path,
    warnings: &mut Vec<String>,
) -> Option<String> {
    // Keyring first. An unavailable keyring is the expected fallback case, so
    // it is not surfaced as an error; a missing entry also falls through so
    // tokens written by older versions are not lost.
    if let Ok(Some(token)) = keyring.get() {
        if !token.trim().is_empty() {
            return Some(token);
        }
    }

    match load_from_file(path, warnings) {
        Ok(token) => token,
        Err(error) => {
            warnings.push(format!(
                "Could not read the stored GitHub token ({}): {error:#}",
                path.display()
            ));
            None
        }
    }
}

fn save_token_with(keyring: &dyn KeyringBackend, path: &Path, token: &str) -> Result<()> {
    let token = token.trim();
    if token.is_empty() {
        if let Err(error) = keyring.delete() {
            // The keyring may be unavailable; deleting the file is still
            // useful and must not be skipped.
            let _ = error;
        }
        return remove_file_if_exists(path);
    }

    match keyring.set(token) {
        Ok(()) => {
            // Never keep a second copy when the keyring accepted the token.
            remove_file_if_exists(path)
        }
        Err(_) => write_encrypted_file(path, token),
    }
}

fn remove_file_if_exists(path: &Path) -> Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).with_context(|| format!("failed to remove {}", path.display())),
    }
}

// ---------------------------------------------------------------------------
// File format v2
// ---------------------------------------------------------------------------

fn write_encrypted_file(path: &Path, token: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        crate::fsutil::ensure_private_dir(parent)?;
    }

    let random = SystemRandom::new();
    let mut salt = [0_u8; SALT_LEN];
    random
        .fill(&mut salt)
        .map_err(|_| anyhow!("failed to generate a token encryption salt"))?;
    let mut nonce = [0_u8; NONCE_LEN];
    random
        .fill(&mut nonce)
        .map_err(|_| anyhow!("failed to generate a token encryption nonce"))?;

    let mut key = derive_key(&salt)?;
    let mut sealed = {
        let unbound = aead::UnboundKey::new(&aead::AES_256_GCM, &key)
            .map_err(|_| anyhow!("failed to initialize token encryption"))?;
        let sealing_key = aead::LessSafeKey::new(unbound);
        // `seal_in_place_append_tag` overwrites the plaintext with ciphertext
        // and appends the tag, so no plaintext copy survives in this buffer.
        let mut in_out = token.as_bytes().to_vec();
        sealing_key
            .seal_in_place_append_tag(
                aead::Nonce::assume_unique_for_key(nonce),
                aead::Aad::empty(),
                &mut in_out,
            )
            .map_err(|_| anyhow!("failed to encrypt the GitHub token"))?;
        in_out
    };
    key.zeroize();

    let mut data = Vec::with_capacity(MAGIC_V2.len() + SALT_LEN + NONCE_LEN + sealed.len());
    data.extend_from_slice(MAGIC_V2);
    data.extend_from_slice(&salt);
    data.extend_from_slice(&nonce);
    data.append(&mut sealed);

    crate::fsutil::write_atomic_private(path, &data)
        .with_context(|| format!("failed to write {}", path.display()))?;
    Ok(())
}

fn load_from_file(path: &Path, warnings: &mut Vec<String>) -> Result<Option<String>> {
    let data = match std::fs::read(path) {
        Ok(data) => data,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(error).with_context(|| format!("failed to read {}", path.display()));
        }
    };

    if let Some(warning) = tighten_token_file_mode(path) {
        warnings.push(warning);
    }

    if data.len() < MAGIC_V1.len() || data[..MAGIC_V2.len()] == MAGIC_V2[..] {
        return match decrypt_v2(&data) {
            Some(plaintext) => Ok(Some(UTF8(plaintext).into_string()?)),
            None => {
                warnings.push(format!(
                    "The stored GitHub token could not be decrypted ({}); the file was left in place.",
                    path.display()
                ));
                Ok(None)
            }
        };
    }

    if data[..MAGIC_V1.len()] == MAGIC_V1[..] {
        return match decrypt_v1(&data) {
            Some(plaintext) => {
                let token = UTF8(plaintext).into_string()?;
                // Replace the legacy bytes with the hardened v2 format.
                match write_encrypted_file(path, &token) {
                    Ok(()) => warnings.push(
                        "Upgraded the stored GitHub token to the hardened Argon2id format.".into(),
                    ),
                    Err(error) => warnings.push(format!(
                        "The stored GitHub token could not be upgraded ({error:#}); \
                         the old file was kept."
                    )),
                }
                Ok(Some(token))
            }
            None => {
                warnings.push(format!(
                    "The stored GitHub token could not be decrypted ({}); \
                     the file was kept so you can retry or remove it manually.",
                    path.display()
                ));
                Ok(None)
            }
        };
    }

    warnings.push(format!(
        "Ignoring unrecognized GitHub token file {} (left untouched).",
        path.display()
    ));
    Ok(None)
}

/// Small helper that keeps the decrypted bytes zeroized while they are
/// converted to a `String`.
struct UTF8(Zeroizing<Vec<u8>>);

impl UTF8 {
    fn into_string(self) -> Result<String> {
        String::from_utf8(self.0.to_vec()).context("stored GitHub token is not valid UTF-8")
    }
}

fn decrypt_v2(data: &[u8]) -> Option<Zeroizing<Vec<u8>>> {
    let header = MAGIC_V2.len() + SALT_LEN + NONCE_LEN;
    if data.len() <= header || data[..MAGIC_V2.len()] != MAGIC_V2[..] {
        return None;
    }
    let salt = &data[MAGIC_V2.len()..MAGIC_V2.len() + SALT_LEN];
    let nonce_bytes = &data[MAGIC_V2.len() + SALT_LEN..header];
    let ciphertext = &data[header..];

    let mut key = derive_key(salt).ok()?;
    let mut nonce = [0_u8; NONCE_LEN];
    nonce.copy_from_slice(nonce_bytes);

    let plaintext = (|| -> Option<Zeroizing<Vec<u8>>> {
        let unbound = aead::UnboundKey::new(&aead::AES_256_GCM, &key).ok()?;
        let opening_key = aead::LessSafeKey::new(unbound);
        let mut in_out = Zeroizing::new(ciphertext.to_vec());
        let plaintext = opening_key
            .open_in_place(
                aead::Nonce::assume_unique_for_key(nonce),
                aead::Aad::empty(),
                &mut in_out,
            )
            .ok()?;
        Some(Zeroizing::new(plaintext.to_vec()))
    })();
    key.zeroize();
    plaintext
}

// ---------------------------------------------------------------------------
// Legacy format v1 (migration only)
// ---------------------------------------------------------------------------

fn decrypt_v1(data: &[u8]) -> Option<Zeroizing<Vec<u8>>> {
    if data.len() <= MAGIC_V1.len() + NONCE_LEN || data[..MAGIC_V1.len()] != MAGIC_V1[..] {
        return None;
    }
    let (nonce_bytes, ciphertext) = data[MAGIC_V1.len()..].split_at(NONCE_LEN);
    let mut nonce = [0_u8; NONCE_LEN];
    nonce.copy_from_slice(nonce_bytes);

    let mut key = derive_legacy_key();
    let plaintext = (|| -> Option<Zeroizing<Vec<u8>>> {
        let unbound = aead::UnboundKey::new(&aead::AES_256_GCM, &key).ok()?;
        let opening_key = aead::LessSafeKey::new(unbound);
        let mut in_out = Zeroizing::new(ciphertext.to_vec());
        let plaintext = opening_key
            .open_in_place(
                aead::Nonce::assume_unique_for_key(nonce),
                aead::Aad::empty(),
                &mut in_out,
            )
            .ok()?;
        Some(Zeroizing::new(plaintext.to_vec()))
    })();
    key.zeroize();
    plaintext
}

/// Key derivation used by v1 files. Kept only to migrate them; v1 was
/// obfuscation, not encryption, so this must never be used for new writes.
fn derive_legacy_key() -> [u8; KEY_LEN] {
    let machine_id = std::fs::read_to_string("/etc/machine-id").unwrap_or_default();
    let user = std::env::var("USER").unwrap_or_default();
    let home = std::env::var("HOME").unwrap_or_default();

    let mut context = digest::Context::new(&digest::SHA256);
    context.update(b"appimage-hub-github-token-v1");
    context.update(machine_id.trim().as_bytes());
    context.update(user.as_bytes());
    context.update(home.as_bytes());
    let hash = context.finish();

    let mut key = [0_u8; KEY_LEN];
    key.copy_from_slice(hash.as_ref());
    key
}

// ---------------------------------------------------------------------------
// Key derivation and permissions
// ---------------------------------------------------------------------------

/// Material mixed into the file key. It binds the ciphertext to this machine
/// and user, but — unlike a hidden key file — it is not itself secret, which
/// is why the random salt and Argon2id work factor carry the protection.
fn machine_binding() -> Zeroizing<Vec<u8>> {
    let machine_id = std::fs::read_to_string("/etc/machine-id").unwrap_or_default();
    let user = std::env::var("USER").unwrap_or_default();
    let home = std::env::var("HOME").unwrap_or_default();

    let mut material = Vec::new();
    material.extend_from_slice(b"appimage-hub-github-token-v2\0");
    material.extend_from_slice(machine_id.trim().as_bytes());
    material.push(0);
    material.extend_from_slice(user.as_bytes());
    material.push(0);
    material.extend_from_slice(home.as_bytes());
    Zeroizing::new(material)
}

fn derive_key(salt: &[u8]) -> Result<[u8; KEY_LEN]> {
    let params = Params::new(
        ARGON2_M_COST_KIB,
        ARGON2_T_COST,
        ARGON2_P_COST,
        Some(KEY_LEN),
    )
    .map_err(|error| anyhow!("invalid Argon2id parameters: {error}"))?;
    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let binding = machine_binding();
    let mut key = [0_u8; KEY_LEN];
    argon2
        .hash_password_into(binding.as_slice(), salt, &mut key)
        .map_err(|error| anyhow!("failed to derive the token key: {error}"))?;
    Ok(key)
}

#[cfg(unix)]
fn tighten_token_file_mode(path: &Path) -> Option<String> {
    use std::os::unix::fs::PermissionsExt as _;

    let metadata = std::fs::metadata(path).ok()?;
    let current = metadata.permissions().mode() & 0o777;
    if current & 0o077 == 0 {
        return None;
    }
    let mut permissions = metadata.permissions();
    permissions.set_mode(0o600);
    match std::fs::set_permissions(path, permissions) {
        Ok(()) => Some(format!(
            "The GitHub token file had mode {current:03o}; it was tightened to 0600."
        )),
        Err(error) => Some(format!(
            "The GitHub token file has insecure mode {current:03o} and could not be tightened: {error}"
        )),
    }
}

#[cfg(not(unix))]
fn tighten_token_file_mode(_path: &Path) -> Option<String> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("aim-secrets-{tag}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    struct FailingKeyring;

    impl KeyringBackend for FailingKeyring {
        fn get(&self) -> Result<Option<String>> {
            Err(anyhow!("no Secret Service in this test"))
        }
        fn set(&self, _token: &str) -> Result<()> {
            Err(anyhow!("no Secret Service in this test"))
        }
        fn delete(&self) -> Result<()> {
            Err(anyhow!("no Secret Service in this test"))
        }
    }

    #[derive(Default)]
    struct MemoryKeyring {
        token: Mutex<Option<String>>,
    }

    impl KeyringBackend for MemoryKeyring {
        fn get(&self) -> Result<Option<String>> {
            Ok(self.token.lock().unwrap().clone())
        }
        fn set(&self, token: &str) -> Result<()> {
            *self.token.lock().unwrap() = Some(token.to_string());
            Ok(())
        }
        fn delete(&self) -> Result<()> {
            *self.token.lock().unwrap() = None;
            Ok(())
        }
    }

    #[test]
    fn token_round_trips_v2() {
        let dir = temp_dir("roundtrip");
        let path = dir.join("github_token.enc");

        write_encrypted_file(&path, "ghp_example_token").unwrap();

        let data = std::fs::read(&path).unwrap();
        assert_eq!(&data[..MAGIC_V2.len()], MAGIC_V2);

        let mut warnings = Vec::new();
        let loaded = load_from_file(&path, &mut warnings).unwrap();
        assert_eq!(loaded.as_deref(), Some("ghp_example_token"));
        assert!(warnings.is_empty(), "unexpected warnings: {warnings:?}");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn wrong_key_fails_to_decrypt() {
        let dir = temp_dir("wrongkey");
        let path = dir.join("github_token.enc");
        write_encrypted_file(&path, "secret").unwrap();

        // Corrupt the salt: the derived key no longer matches.
        let mut data = std::fs::read(&path).unwrap();
        for byte in &mut data[MAGIC_V2.len()..MAGIC_V2.len() + SALT_LEN] {
            *byte ^= 0xFF;
        }
        std::fs::write(&path, &data).unwrap();

        let mut warnings = Vec::new();
        assert!(load_from_file(&path, &mut warnings).unwrap().is_none());
        assert!(path.exists(), "the unreadable file must be preserved");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn tampered_ciphertext_fails() {
        let dir = temp_dir("tampered");
        let path = dir.join("github_token.enc");
        write_encrypted_file(&path, "secret").unwrap();

        let mut data = std::fs::read(&path).unwrap();
        let last = data.len() - 1;
        data[last] ^= 0x01;
        std::fs::write(&path, &data).unwrap();

        let mut warnings = Vec::new();
        assert!(load_from_file(&path, &mut warnings).unwrap().is_none());
        assert!(path.exists());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn v1_file_is_migrated_then_removed() {
        let dir = temp_dir("migrate");
        let path = dir.join("github_token.enc");
        std::fs::write(&path, legacy_v1_file("ghp_legacy")).unwrap();

        let mut warnings = Vec::new();
        let loaded = load_from_file(&path, &mut warnings).unwrap();
        assert_eq!(loaded.as_deref(), Some("ghp_legacy"));

        // The legacy bytes are gone; the file is now v2.
        let data = std::fs::read(&path).unwrap();
        assert_eq!(&data[..MAGIC_V2.len()], MAGIC_V2);
        assert!(!data.windows(MAGIC_V1.len()).any(|w| w == MAGIC_V1));
        assert!(warnings.iter().any(|warning| warning.contains("Upgraded")));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn empty_token_deletes_file() {
        let dir = temp_dir("empty");
        let path = dir.join("github_token.enc");
        write_encrypted_file(&path, "secret").unwrap();
        assert!(path.exists());

        let keyring = MemoryKeyring::default();
        save_token_with(&keyring, &path, "").unwrap();

        assert!(!path.exists());
        assert!(keyring.get().unwrap().is_none());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn token_file_mode_is_0600() {
        use std::os::unix::fs::PermissionsExt as _;

        let dir = temp_dir("mode");
        let path = dir.join("github_token.enc");
        write_encrypted_file(&path, "secret").unwrap();

        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn portable_mode_uses_xdg_token_path() {
        let path = token_path();
        assert!(path.ends_with("appimage-manager/github_token.enc"));
        assert!(!path.to_string_lossy().contains("AppImageHub.config"));
    }

    #[test]
    fn keyring_failure_falls_back_to_file() {
        let dir = temp_dir("fallback");
        let path = dir.join("github_token.enc");

        save_token_with(&FailingKeyring, &path, "ghp_fallback").unwrap();

        assert!(path.exists());
        let mut warnings = Vec::new();
        let loaded = load_token_with(&FailingKeyring, &path, &mut warnings);
        assert_eq!(loaded.as_deref(), Some("ghp_fallback"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn keyring_success_removes_file_copy() {
        let dir = temp_dir("keyring");
        let path = dir.join("github_token.enc");
        write_encrypted_file(&path, "old_file_token").unwrap();
        let keyring = MemoryKeyring::default();

        save_token_with(&keyring, &path, "new_keyring_token").unwrap();

        assert!(!path.exists(), "the file copy must be removed");
        let mut warnings = Vec::new();
        let loaded = load_token_with(&keyring, &path, &mut warnings);
        assert_eq!(loaded.as_deref(), Some("new_keyring_token"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Builds a v1 ciphertext using the legacy derivation, for migration tests.
    fn legacy_v1_file(token: &str) -> Vec<u8> {
        let random = SystemRandom::new();
        let mut nonce = [0_u8; NONCE_LEN];
        random.fill(&mut nonce).unwrap();

        let key = derive_legacy_key();
        let unbound = aead::UnboundKey::new(&aead::AES_256_GCM, &key).unwrap();
        let sealing_key = aead::LessSafeKey::new(unbound);
        let mut in_out = token.as_bytes().to_vec();
        sealing_key
            .seal_in_place_append_tag(
                aead::Nonce::assume_unique_for_key(nonce),
                aead::Aad::empty(),
                &mut in_out,
            )
            .unwrap();

        let mut data = Vec::new();
        data.extend_from_slice(MAGIC_V1);
        data.extend_from_slice(&nonce);
        data.extend_from_slice(&in_out);
        data
    }
}
