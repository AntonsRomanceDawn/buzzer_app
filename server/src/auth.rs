use jsonwebtoken::errors::ErrorKind;
use jsonwebtoken::{DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};

use core::game::PlayerId;

use crate::dtos::Role;
use crate::errors::AppError;
use crate::utils::time::now_seconds;

#[derive(Serialize, Deserialize)]
pub struct Claims {
    pub room_id: String,
    pub player_id: PlayerId,
    pub name: String,
    pub role: Role,
    pub iat: u64,
    pub exp: u64,
}

pub struct JwtAuth {
    encoding: EncodingKey,
    decoding: DecodingKey,
    validation: Validation,
    ttl_seconds: u64,
}

impl JwtAuth {
    pub fn new(secret: &[u8], ttl_seconds: u64) -> Self {
        Self {
            encoding: EncodingKey::from_secret(secret),
            decoding: DecodingKey::from_secret(secret),
            validation: Validation::default(),
            ttl_seconds,
        }
    }

    pub fn issue(
        &self,
        room_id: &str,
        player_id: PlayerId,
        name: &str,
        role: Role,
    ) -> Result<(String, u64), AppError> {
        let now = now_seconds();
        let exp = now + self.ttl_seconds;

        let claims = Claims {
            room_id: room_id.to_string(),
            player_id,
            name: name.to_string(),
            role,
            iat: now,
            exp,
        };
        let token = jsonwebtoken::encode(&Header::default(), &claims, &self.encoding)
            .map_err(|_| AppError::Internal)?;

        Ok((token, exp))
    }

    pub fn verify(&self, token: &str) -> Result<Claims, AppError> {
        let data = jsonwebtoken::decode::<Claims>(token, &self.decoding, &self.validation)
            .map_err(|err| match err.kind() {
                ErrorKind::ExpiredSignature => AppError::SessionExpired,
                _ => AppError::InvalidToken,
            })?;
        Ok(data.claims)
    }
}
