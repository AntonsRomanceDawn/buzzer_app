use jsonwebtoken::{DecodingKey, EncodingKey, Header, Validation, errors::Error};
use serde::{Deserialize, Serialize};

use core::game::PlayerId;

use crate::dtos::Role;
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
    ) -> Result<String, Error> {
        let now = now_seconds();
        let claims = Claims {
            room_id: room_id.to_string(),
            player_id,
            name: name.to_string(),
            role,
            iat: now,
            exp: now + self.ttl_seconds,
        };
        jsonwebtoken::encode(&Header::default(), &claims, &self.encoding)
    }

    pub fn verify(&self, token: &str) -> Result<Claims, Error> {
        let data = jsonwebtoken::decode::<Claims>(token, &self.decoding, &self.validation)?;
        Ok(data.claims)
    }
}
