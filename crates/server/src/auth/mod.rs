pub mod jwt;
pub mod password;

use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Admin,
}

impl Role {
    pub fn as_str(&self) -> &'static str {
        match self {
            Role::User => "user",
            Role::Admin => "admin",
        }
    }
}

impl TryFrom<&str> for Role {
    type Error = ();

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "user" => Ok(Role::User),
            "admin" => Ok(Role::Admin),
            _ => Err(()),
        }
    }
}

pub trait RequiredRole {
    fn required() -> Role;
}

pub struct UserRole;

impl RequiredRole for UserRole {
    fn required() -> Role {
        Role::User
    }
}

pub struct AdminRole;

impl RequiredRole for AdminRole {
    fn required() -> Role {
        Role::Admin
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Claims {
    pub sub: String, // user id / email
    pub exp: usize,  // expiry (unix)
    pub iat: usize,  // issued at
    pub roles: Vec<Role>,
}
