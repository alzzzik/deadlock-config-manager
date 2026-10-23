//! Optional "Sign in with GitHub" (OAuth device flow) for people who link many repos: signed-in API calls
//! get 5,000 requests an hour instead of 60. No secret is needed, only the OAuth app's public client ID.
//! The token has no scopes (public data only) and is stored in Windows Credential Manager.
//!
//! Hidden until `CLIENT_ID` is set: register an OAuth app on GitHub with "Enable Device Flow" ticked.

use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Public client ID of the Deadlock Config Manager OAuth app. Empty = sign-in not offered.
pub const CLIENT_ID: &str = "";
const KEYRING_SERVICE: &str = "DeadlockConfigManager";
const KEYRING_USER: &str = "github";
const TIMEOUT: Duration = Duration::from_secs(15);
pub const VERIFY_URL: &str = "https://github.com/login/device";

pub fn available() -> bool {
    !CLIENT_ID.is_empty()
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new().timeout(TIMEOUT).user_agent(concat!("DeadlockConfigManager/", env!("CARGO_PKG_VERSION"))).build()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DeviceCode {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub interval: u64,
    pub expires_in: u64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", tag = "state")]
pub enum PollResult {
    /// Still waiting for the user to enter the code; poll again after `interval` seconds.
    Pending { interval: u64 },
    Done { login: String },
    Failed { message: String },
}

/// GitHub's device-code answer (it uses snake_case).
pub fn parse_device_code(json: &serde_json::Value) -> Result<DeviceCode, String> {
    let s = |k: &str| json[k].as_str().map(str::to_string).ok_or_else(|| format!("GitHub didn't send {k}."));
    let uri = s("verification_uri")?;
    if uri != VERIFY_URL {
        return Err("GitHub sent an unexpected sign-in address.".into());
    }
    Ok(DeviceCode {
        device_code: s("device_code")?,
        user_code: s("user_code")?,
        verification_uri: uri,
        interval: json["interval"].as_u64().unwrap_or(5),
        expires_in: json["expires_in"].as_u64().unwrap_or(900),
    })
}

pub fn start() -> Result<DeviceCode, String> {
    if !available() {
        return Err("Sign-in isn't set up in this build.".into());
    }
    let json: serde_json::Value = agent()
        .post("https://github.com/login/device/code")
        .set("Accept", "application/json")
        .send_form(&[("client_id", CLIENT_ID), ("scope", "")])
        .map_err(|e| format!("Couldn't reach GitHub: {e}"))?
        .into_json()
        .map_err(|e| e.to_string())?;
    parse_device_code(&json)
}

/// Interprets one access-token poll answer. Ok(token) when signed in.
pub fn parse_poll(json: &serde_json::Value, interval: u64) -> Result<String, PollResult> {
    if let Some(token) = json["access_token"].as_str().filter(|t| !t.is_empty()) {
        return Ok(token.to_string());
    }
    Err(match json["error"].as_str() {
        Some("authorization_pending") => PollResult::Pending { interval },
        Some("slow_down") => PollResult::Pending { interval: json["interval"].as_u64().unwrap_or(interval + 5) },
        Some("expired_token") => PollResult::Failed { message: "The code expired. Start again.".into() },
        Some("access_denied") => PollResult::Failed { message: "Sign-in was cancelled.".into() },
        _ => PollResult::Failed { message: "GitHub refused the sign-in.".into() },
    })
}

fn login_for(token: &str) -> Result<String, String> {
    let json: serde_json::Value = agent()
        .get("https://api.github.com/user")
        .set("Authorization", &format!("Bearer {token}"))
        .call()
        .map_err(|e| format!("GitHub didn't accept the sign-in: {e}"))?
        .into_json()
        .map_err(|e| e.to_string())?;
    json["login"].as_str().map(str::to_string).ok_or_else(|| "GitHub didn't return a user name.".into())
}

pub fn poll(device_code: &str, interval: u64) -> PollResult {
    let answer = agent()
        .post("https://github.com/login/oauth/access_token")
        .set("Accept", "application/json")
        .send_form(&[("client_id", CLIENT_ID), ("device_code", device_code), ("grant_type", "urn:ietf:params:oauth:grant-type:device_code")]);
    let json: serde_json::Value = match answer.map(|r| r.into_json()) {
        Ok(Ok(j)) => j,
        Ok(Err(e)) => return PollResult::Failed { message: e.to_string() },
        Err(e) => return PollResult::Failed { message: format!("Couldn't reach GitHub: {e}") },
    };
    let token = match parse_poll(&json, interval) {
        Ok(t) => t,
        Err(state) => return state,
    };
    match login_for(&token).and_then(|login| save_token(&token).map(|_| login)) {
        Ok(login) => {
            crate::github::set_token(Some(token));
            PollResult::Done { login }
        }
        Err(message) => PollResult::Failed { message },
    }
}

fn entry() -> Result<keyring::Entry, String> {
    keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER).map_err(|e| e.to_string())
}

fn save_token(token: &str) -> Result<(), String> {
    entry()?.set_password(token).map_err(|e| format!("Couldn't save the sign-in in Windows Credential Manager: {e}"))
}

pub fn load_token() -> Option<String> {
    entry().ok()?.get_password().ok().filter(|t| !t.is_empty())
}

pub fn sign_out() -> Result<(), String> {
    crate::github::set_token(None);
    match entry()?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}

/// Signed-in user name, checked against GitHub (None when signed out or the token stopped working).
pub fn current_login() -> Option<String> {
    load_token().and_then(|t| login_for(&t).ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_device_code_and_rejects_odd_urls() {
        let ok = serde_json::json!({ "device_code": "d", "user_code": "ABCD-1234", "verification_uri": VERIFY_URL, "interval": 5, "expires_in": 899 });
        assert_eq!(parse_device_code(&ok).unwrap().user_code, "ABCD-1234");
        let odd = serde_json::json!({ "device_code": "d", "user_code": "x", "verification_uri": "https://evil.example" });
        assert!(parse_device_code(&odd).is_err());
    }

    #[test]
    fn interprets_poll_answers() {
        assert_eq!(parse_poll(&serde_json::json!({ "access_token": "t" }), 5), Ok("t".into()));
        assert_eq!(parse_poll(&serde_json::json!({ "error": "authorization_pending" }), 5), Err(PollResult::Pending { interval: 5 }));
        assert_eq!(parse_poll(&serde_json::json!({ "error": "slow_down", "interval": 10 }), 5), Err(PollResult::Pending { interval: 10 }));
        assert!(matches!(parse_poll(&serde_json::json!({ "error": "access_denied" }), 5), Err(PollResult::Failed { .. })));
    }
}
