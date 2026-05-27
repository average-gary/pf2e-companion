//! OS-keychain wrapper for storing the user's LLM API keys.
//!
//! macOS Keychain, Windows Credential Manager, libsecret on Linux.
//! Mobile platforms (iOS / Android) currently no-op behind an
//! `MockCredential` backend at compile time — a Stage D-or-later
//! enhancement is to wire `tauri-plugin-store` for those targets so
//! mobile users can configure keys in-app. v1 mobile users are
//! expected to configure on desktop.
//!
//! All entries live under the `pf2e-companion` service. Keys per provider:
//!   - `anthropic_api_key`

use anyhow::Result;
use keyring::Entry;

const SERVICE: &str = "pf2e-companion";

pub fn key_name(provider: &str) -> String {
    format!("{provider}_api_key")
}

fn entry(provider: &str) -> Result<Entry> {
    Ok(Entry::new(SERVICE, &key_name(provider))?)
}

pub fn set_key(provider: &str, secret: &str) -> Result<()> {
    let e = entry(provider)?;
    e.set_password(secret)?;
    Ok(())
}

pub fn get_key(provider: &str) -> Result<Option<String>> {
    let e = entry(provider)?;
    match e.get_password() {
        Ok(s) => Ok(Some(s)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

pub fn clear_key(provider: &str) -> Result<()> {
    let e = entry(provider)?;
    match e.delete_credential() {
        Ok(_) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(e.into()),
    }
}

pub fn has_key(provider: &str) -> bool {
    matches!(get_key(provider), Ok(Some(_)))
}
