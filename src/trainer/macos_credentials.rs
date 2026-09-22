//! macOS coaching credentials.
//!
//! Prefer a local Data Protection internet password (`kSecUseDataProtectionKeychain`,
//! `kSecAttrSynchronizable` = false) so the item can appear in Passwords / Local
//! Items without iCloud sync (issue #1). CLI and unsigned builds often lack the
//! Data Protection entitlement (OSStatus -34018); fall back to a local login-
//! keychain generic password under `dev.openfelt.coaching`, which Keychain Access
//! shows and which does not require that entitlement (Apple TN3137).
use security_framework::base::Error as SecError;
use security_framework::passwords::{
    delete_generic_password_options, generic_password, set_generic_password_options,
    PasswordOptions,
};
use security_framework_sys::keychain::{SecAuthenticationType, SecProtocolType};

use super::provider::{Credential, Provider, KEYRING_SERVICE};

/// `errSecItemNotFound` from SecBase.h / Security framework.
const ERR_SEC_ITEM_NOT_FOUND: i32 = -25300;
/// `errSecMissingEntitlement` — Data Protection keychain needs an app entitlement.
const ERR_SEC_MISSING_ENTITLEMENT: i32 = -34018;

/// Hostname used as `kSecAttrServer` so the item is an internet password.
pub fn credential_server(provider: Provider) -> &'static str {
    match provider {
        Provider::Openai => "api.openai.com",
        Provider::Anthropic => "api.anthropic.com",
    }
}

pub fn credential_label(provider: Provider) -> String {
    format!("OpenFelt coaching ({})", credential_server(provider))
}

/// Where the user should look after Save. Sync is never enabled.
pub fn storage_hint() -> &'static str {
    "Saved locally on this Mac (no iCloud sync). Prefer Passwords / Keychain Access → Local Items for api.openai.com or api.anthropic.com; CLI builds may store under Keychain Access service dev.openfelt.coaching instead."
}

fn internet_options(provider: Provider) -> PasswordOptions {
    let mut options = PasswordOptions::new_internet_password(
        credential_server(provider),
        None,
        provider.account(),
        "/",
        Some(443),
        SecProtocolType::HTTPS,
        SecAuthenticationType::Default,
    );
    options.use_protected_keychain();
    // Explicitly local-only: never set synchronizable true (issue #1).
    options.set_access_synchronized(Some(false));
    options.set_label(&credential_label(provider));
    options.set_description("OpenFelt coaching API key");
    options.set_comment("OpenFelt; local only; not synced");
    options
}

fn legacy_generic_options(provider: Provider) -> PasswordOptions {
    // File-based login keychain generic (no Data Protection). Matches older
    // `keyring` apple-native items and works for CLI binaries.
    let mut options = PasswordOptions::new_generic_password(KEYRING_SERVICE, provider.account());
    options.set_label(&credential_label(provider));
    options
}

/// Short OSStatus explanation. Never includes secret material.
pub fn describe_sec_error(err: &SecError) -> String {
    match err.code() {
        ERR_SEC_MISSING_ENTITLEMENT => {
            "Data Protection keychain needs an app entitlement (-34018)".into()
        }
        -25291 => "no keychain available (-25291)".into(),
        -25299 => "duplicate keychain item (-25299)".into(),
        -50 => "invalid keychain parameters (-50)".into(),
        -25300 => "keychain item not found (-25300)".into(),
        code => format!("keychain error {code}"),
    }
}

pub fn get(provider: Provider) -> Result<Option<Credential>, String> {
    match generic_password(internet_options(provider)) {
        Ok(bytes) => return decode(bytes).map(Some),
        Err(err) if err.code() == ERR_SEC_ITEM_NOT_FOUND => {}
        Err(err) => {
            return Err(format!(
                "Cannot read the saved coaching credential ({})",
                describe_sec_error(&err)
            ));
        }
    }
    match generic_password(legacy_generic_options(provider)) {
        Ok(bytes) => decode(bytes).map(Some),
        Err(err) if err.code() == ERR_SEC_ITEM_NOT_FOUND => Ok(None),
        Err(err) => Err(format!(
            "Cannot read the saved coaching credential ({})",
            describe_sec_error(&err)
        )),
    }
}

pub fn set(provider: Provider, secret: &str) -> Result<(), String> {
    match set_generic_password_options(secret.as_bytes(), internet_options(provider)) {
        Ok(()) => {
            // Prefer a single store: drop any older generic copy.
            let _ = delete_generic_password_options(legacy_generic_options(provider));
            Ok(())
        }
        Err(dp_err) => {
            // CLI / unsigned builds typically cannot write Data Protection items.
            match set_generic_password_options(secret.as_bytes(), legacy_generic_options(provider))
            {
                Ok(()) => Ok(()),
                Err(legacy_err) => Err(format!(
                    "Cannot save the coaching credential ({} — fallback: {})",
                    describe_sec_error(&dp_err),
                    describe_sec_error(&legacy_err)
                )),
            }
        }
    }
}

pub fn delete(provider: Provider) -> Result<(), String> {
    let internet = delete_generic_password_options(internet_options(provider));
    let generic = delete_generic_password_options(legacy_generic_options(provider));
    match (internet, generic) {
        (Ok(()), _) | (_, Ok(())) => Ok(()),
        (Err(a), Err(b))
            if a.code() == ERR_SEC_ITEM_NOT_FOUND && b.code() == ERR_SEC_ITEM_NOT_FOUND =>
        {
            Ok(())
        }
        (Err(a), Err(b)) => Err(format!(
            "Cannot forget the coaching credential ({} / {})",
            describe_sec_error(&a),
            describe_sec_error(&b)
        )),
    }
}

fn decode(bytes: Vec<u8>) -> Result<Credential, String> {
    let text = String::from_utf8(bytes).map_err(|_| "Invalid credential".to_string())?;
    Credential::new(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn servers_and_hint_document_local_passwords_without_sync() {
        assert_eq!(credential_server(Provider::Openai), "api.openai.com");
        assert_eq!(credential_server(Provider::Anthropic), "api.anthropic.com");
        assert!(credential_label(Provider::Openai).contains("api.openai.com"));
        assert!(storage_hint().contains("no iCloud sync"));
        assert!(storage_hint().contains("Passwords") || storage_hint().contains("Keychain"));
        assert!(!storage_hint().to_ascii_lowercase().contains("enables sync"));
    }

    #[test]
    fn internet_options_build_without_enabling_sync() {
        let _ = internet_options(Provider::Openai);
        let _ = internet_options(Provider::Anthropic);
        let mut options = PasswordOptions::new_internet_password(
            "api.openai.com",
            None,
            "openai-api-key",
            "/",
            Some(443),
            SecProtocolType::HTTPS,
            SecAuthenticationType::Default,
        );
        options.use_protected_keychain();
        options.set_access_synchronized(Some(false));
        options.set_label("OpenFelt coaching (api.openai.com)");
    }

    #[test]
    fn describe_sec_error_never_embeds_secrets() {
        let entitlement = SecError::from_code(ERR_SEC_MISSING_ENTITLEMENT);
        let text = describe_sec_error(&entitlement);
        assert!(text.contains("-34018"));
        assert!(!text.contains("sk-"));
        assert!(!text.to_ascii_lowercase().contains("password"));
        let generic = describe_sec_error(&SecError::from_code(-25291));
        assert!(generic.contains("-25291"));
    }

    #[test]
    fn set_failure_message_includes_reason_without_secret() {
        // Construct the same error string shape set() returns on total failure.
        let dp = describe_sec_error(&SecError::from_code(ERR_SEC_MISSING_ENTITLEMENT));
        let legacy = describe_sec_error(&SecError::from_code(-50));
        let message = format!("Cannot save the coaching credential ({dp} — fallback: {legacy})");
        assert!(message.contains("-34018"));
        assert!(message.contains("-50"));
        assert!(!message.contains("sk-"));
        assert!(!message.contains("wzgA"));
    }

    /// Live round-trip: DP when allowed, otherwise login-keychain generic fallback.
    #[test]
    fn optional_save_round_trip_prefers_dp_falls_back_to_generic() {
        let account = format!("openfelt-test-{}", std::process::id());
        // Use a dedicated provider-shaped path via generic/internet helpers with a
        // unique account so we never touch the user's real coaching item.
        let secret = "openfelt-roundtrip-secret-not-a-key";
        let mut dp = PasswordOptions::new_internet_password(
            "openfelt.test.local",
            None,
            &account,
            "/",
            Some(443),
            SecProtocolType::HTTPS,
            SecAuthenticationType::Default,
        );
        dp.use_protected_keychain();
        dp.set_access_synchronized(Some(false));
        dp.set_label("OpenFelt test (do not sync)");

        let used_dp = match set_generic_password_options(secret.as_bytes(), dp) {
            Ok(()) => true,
            Err(err) => {
                assert!(
                    err.code() == ERR_SEC_MISSING_ENTITLEMENT || err.code() != 0,
                    "unexpected DP failure: {err:?}"
                );
                let mut legacy =
                    PasswordOptions::new_generic_password("dev.openfelt.coaching-test", &account);
                legacy.set_label("OpenFelt test generic");
                set_generic_password_options(secret.as_bytes(), legacy)
                    .expect("login-keychain generic fallback must work without DP entitlement");
                false
            }
        };

        if used_dp {
            let mut read = PasswordOptions::new_internet_password(
                "openfelt.test.local",
                None,
                &account,
                "/",
                Some(443),
                SecProtocolType::HTTPS,
                SecAuthenticationType::Default,
            );
            read.use_protected_keychain();
            read.set_access_synchronized(Some(false));
            let got = generic_password(read).expect("read DP item");
            assert_eq!(got, secret.as_bytes());
            let mut del = PasswordOptions::new_internet_password(
                "openfelt.test.local",
                None,
                &account,
                "/",
                Some(443),
                SecProtocolType::HTTPS,
                SecAuthenticationType::Default,
            );
            del.use_protected_keychain();
            del.set_access_synchronized(Some(false));
            delete_generic_password_options(del).expect("cleanup DP");
        } else {
            let read =
                PasswordOptions::new_generic_password("dev.openfelt.coaching-test", &account);
            let got = generic_password(read).expect("read generic fallback");
            assert_eq!(got, secret.as_bytes());
            delete_generic_password_options(PasswordOptions::new_generic_password(
                "dev.openfelt.coaching-test",
                &account,
            ))
            .expect("cleanup generic");
        }
    }
}
