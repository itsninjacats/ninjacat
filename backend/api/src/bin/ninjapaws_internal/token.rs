use std::sync::Arc;

use jsonwebtoken::errors::{Error, ErrorKind};
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode};
use serde::Deserialize;
use tako::middleware::jwt_auth::JwtVerifier;

/// What the panel's token says (frontend/src/lib/server/ninjacat.ts): the one tenant every query
/// is for. A token naming several, as an array, does not deserialize.
#[derive(Clone, Deserialize)]
pub struct Claims {
    pub tenant: String,
}

/// The panel signs an HS256 token for each call, valid 60 seconds; `JwtAuth` checks it here.
#[derive(Clone)]
pub struct PanelToken {
    key: Arc<DecodingKey>,
    validation: Arc<Validation>,
}

impl PanelToken {
    pub fn new(secret: &[u8]) -> Self {
        let mut validation = Validation::new(Algorithm::HS256);
        validation.set_issuer(&["ninjacat-panel"]);
        validation.set_audience(&["ninjacat-query"]);
        // Otherwise a token without `iss` or `aud` passes: they are checked only when present.
        validation.set_required_spec_claims(&["exp", "iss", "aud"]);
        // The default minute of leeway would let a 60-second token live two.
        validation.leeway = 5;
        validation.validate_nbf = true;
        Self { key: Arc::new(DecodingKey::from_secret(secret)), validation: Arc::new(validation) }
    }
}

impl JwtVerifier for PanelToken {
    type Claims = Claims;
    type Error = Error;

    fn verify(&self, token: &str) -> Result<Claims, Error> {
        let claims = decode::<Claims>(token, &self.key, &self.validation)?.claims;
        if claims.tenant.is_empty() {
            return Err(ErrorKind::InvalidToken.into());
        }
        Ok(claims)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jsonwebtoken::{EncodingKey, Header, encode, get_current_timestamp};
    use serde_json::{Value, json};

    const SECRET: &[u8] = b"test-secret-test-secret-test-secret!";

    fn token(claims: Value, secret: &[u8]) -> String {
        encode(&Header::new(Algorithm::HS256), &claims, &EncodingKey::from_secret(secret)).unwrap()
    }

    fn panel(tenant: Value) -> Value {
        json!({ "tenant": tenant, "iss": "ninjacat-panel", "aud": "ninjacat-query", "exp": get_current_timestamp() + 60 })
    }

    fn verify(token: &str) -> Option<String> {
        PanelToken::new(SECRET).verify(token).ok().map(|c| c.tenant)
    }

    #[test]
    fn accepts_the_panels_token() {
        assert_eq!(verify(&token(panel(json!("t1")), SECRET)), Some("t1".into()));
    }

    #[test]
    fn refuses_another_secret_audience_issuer_or_an_expired_token() {
        assert_eq!(verify(&token(panel(json!("t1")), b"another-secret-another-secret-another")), None);
        let mut wrong_aud = panel(json!("t1"));
        wrong_aud["aud"] = json!("someone-else");
        assert_eq!(verify(&token(wrong_aud, SECRET)), None);
        let mut no_iss = panel(json!("t1"));
        no_iss.as_object_mut().unwrap().remove("iss");
        assert_eq!(verify(&token(no_iss, SECRET)), None);
        let mut no_aud = panel(json!("t1"));
        no_aud.as_object_mut().unwrap().remove("aud");
        assert_eq!(verify(&token(no_aud, SECRET)), None);
        let mut expired = panel(json!("t1"));
        expired["exp"] = json!(get_current_timestamp() - 10);
        assert_eq!(verify(&token(expired, SECRET)), None);
        let mut no_exp = panel(json!("t1"));
        no_exp.as_object_mut().unwrap().remove("exp");
        assert_eq!(verify(&token(no_exp, SECRET)), None);
    }

    #[test]
    fn refuses_no_tenant_or_several() {
        assert_eq!(verify(&token(panel(json!("")), SECRET)), None);
        assert_eq!(verify(&token(panel(json!(["t1", "t2"])), SECRET)), None);
    }
}
