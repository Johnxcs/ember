//! Supabase Auth: the server asks Supabase who an access token belongs to.
//! (Works with any Supabase JWT signing setup; no secret keys needed on the server.)

pub struct AuthCfg {
    pub url: String,
    pub key: String, // the public anon/publishable key
    pub http: reqwest::Client,
}

pub struct AuthUser {
    pub id: String,
    pub email: String,
    pub username: Option<String>,
}

pub async fn verify(a: &AuthCfg, token: &str) -> Result<AuthUser, String> {
    let r = a
        .http
        .get(format!("{}/auth/v1/user", a.url))
        .header("apikey", &a.key)
        .bearer_auth(token)
        .send()
        .await
        .map_err(|e| format!("Auth service unreachable: {e}"))?;
    if !r.status().is_success() {
        return Err("Your login expired. Please log in again.".into());
    }
    let v: serde_json::Value = r.json().await.map_err(|e| e.to_string())?;
    Ok(AuthUser {
        id: v["id"].as_str().unwrap_or("").to_string(),
        email: v["email"].as_str().unwrap_or("").to_string(),
        username: v["user_metadata"]["username"].as_str().map(String::from),
    })
}
