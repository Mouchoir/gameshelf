//! Profiles (FR-031).

use crate::{
    Result, Store, StoreError,
    ids::TextId,
    password::{self, dummy_hash},
    repo::now,
};
use gameshelf_core::{LocalePref, ProfileId, Role};
use secrecy::SecretString;
use sqlx::Row;
use time::OffsetDateTime;

/// A profile as exposed to the rest of the application (no password hash).
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Profile {
    pub id: ProfileId,
    pub username: String,
    pub display_name: String,
    pub email: Option<String>,
    pub role: Role,
    pub locale: LocalePref,
    pub disabled: bool,
    pub created_at: OffsetDateTime,
}

/// Input for [`Store::create_profile`].
#[derive(Debug)]
pub struct NewProfile<'a> {
    pub username: &'a str,
    pub display_name: &'a str,
    pub email: Option<&'a str>,
    pub password: &'a SecretString,
    pub role: Role,
}

/// Validates a username: 3 to 32 characters among `a-z`, `0-9`, `.`, `_` and `-`.
pub fn validate_username(username: &str) -> Result<()> {
    let len = username.chars().count();
    let ok_chars = username
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '_' | '-'));
    if !(3..=32).contains(&len) || !ok_chars {
        return Err(StoreError::invalid(
            "the username must have 3 to 32 characters among lowercase letters, digits, '.', '_' and '-'",
        ));
    }
    Ok(())
}

fn validate_display_name(name: &str) -> Result<()> {
    let len = name.trim().chars().count();
    if !(1..=64).contains(&len) {
        return Err(StoreError::invalid(
            "the display name must have 1 to 64 characters",
        ));
    }
    Ok(())
}

const COLUMNS: &str = "id, username, display_name, email, role, locale, disabled_at, created_at";

fn from_row(row: &sqlx::sqlite::SqliteRow) -> Result<Profile> {
    let role: String = row.try_get("role")?;
    let locale: String = row.try_get("locale")?;
    let disabled_at: Option<OffsetDateTime> = row.try_get("disabled_at")?;
    Ok(Profile {
        id: ProfileId::from_txt(&row.try_get::<String, _>("id")?)?,
        username: row.try_get("username")?,
        display_name: row.try_get("display_name")?,
        email: row.try_get("email")?,
        role: Role::parse(&role).ok_or_else(|| StoreError::Corrupt(format!("role {role:?}")))?,
        locale: LocalePref::parse(&locale).unwrap_or_default(),
        disabled: disabled_at.is_some(),
        created_at: row.try_get("created_at")?,
    })
}

impl Store {
    /// Number of profiles, used by the first-run setup.
    pub async fn count_profiles(&self) -> Result<i64> {
        Ok(sqlx::query_scalar("SELECT count(*) FROM profiles")
            .fetch_one(self.pool())
            .await?)
    }

    pub async fn create_profile(&self, new: NewProfile<'_>) -> Result<Profile> {
        validate_username(new.username)?;
        validate_display_name(new.display_name)?;
        let hash = password::hash(new.password).await?;
        let id = ProfileId::new();
        let now = now();
        let email = new
            .email
            .map(str::trim)
            .filter(|e| !e.is_empty())
            .map(str::to_lowercase);
        let result = sqlx::query(
            "INSERT INTO profiles (id, username, display_name, email, password_hash, role, locale, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, 'auto', ?, ?)",
        )
        .bind(id.txt())
        .bind(new.username)
        .bind(new.display_name.trim())
        .bind(&email)
        .bind(hash)
        .bind(new.role.as_str())
        .bind(now)
        .bind(now)
        .execute(self.pool())
        .await;
        match result {
            Ok(_) => {}
            Err(e) if StoreError::is_unique_violation(&e) => return Err(StoreError::ProfileExists),
            Err(e) => return Err(e.into()),
        }
        self.get_profile(id).await?.ok_or(StoreError::NotFound)
    }

    pub async fn get_profile(&self, id: ProfileId) -> Result<Option<Profile>> {
        let row = sqlx::query(sqlx::AssertSqlSafe(format!(
            "SELECT {COLUMNS} FROM profiles WHERE id = ?"
        )))
        .bind(id.txt())
        .fetch_optional(self.pool())
        .await?;
        row.as_ref().map(from_row).transpose()
    }

    /// Finds an active profile by username or email (used by trusted-proxy sign-in).
    pub async fn find_profile_by_login(&self, login: &str) -> Result<Option<Profile>> {
        let login = login.trim().to_lowercase();
        let row = sqlx::query(sqlx::AssertSqlSafe(format!(
            "SELECT {COLUMNS} FROM profiles WHERE (username = ? OR email = ?) AND disabled_at IS NULL"
        )))
        .bind(&login)
        .bind(&login)
        .fetch_optional(self.pool())
        .await?;
        row.as_ref().map(from_row).transpose()
    }

    pub async fn list_profiles(&self) -> Result<Vec<Profile>> {
        let rows = sqlx::query(sqlx::AssertSqlSafe(format!(
            "SELECT {COLUMNS} FROM profiles ORDER BY display_name COLLATE NOCASE"
        )))
        .fetch_all(self.pool())
        .await?;
        rows.iter().map(from_row).collect()
    }

    /// Checks a username and password. Takes about the same time whether or not the username exists.
    pub async fn verify_login(&self, username: &str, password: &SecretString) -> Result<Option<Profile>> {
        let row = sqlx::query(sqlx::AssertSqlSafe(format!(
            "SELECT {COLUMNS}, password_hash FROM profiles WHERE username = ?"
        )))
        .bind(username.trim().to_lowercase())
        .fetch_optional(self.pool())
        .await?;
        let Some(row) = row else {
            let _ = password::verify(password, dummy_hash()).await;
            return Ok(None);
        };
        let hash: String = row.try_get("password_hash")?;
        if !password::verify(password, &hash).await {
            return Ok(None);
        }
        let profile = from_row(&row)?;
        Ok((!profile.disabled).then_some(profile))
    }

    pub async fn set_password(&self, id: ProfileId, password: &SecretString) -> Result<()> {
        let hash = password::hash(password).await?;
        let done = sqlx::query("UPDATE profiles SET password_hash = ?, updated_at = ? WHERE id = ?")
            .bind(hash)
            .bind(now())
            .bind(id.txt())
            .execute(self.pool())
            .await?;
        if done.rows_affected() == 0 {
            return Err(StoreError::NotFound);
        }
        self.delete_sessions_of(id).await
    }

    pub async fn set_locale(&self, id: ProfileId, locale: LocalePref) -> Result<()> {
        sqlx::query("UPDATE profiles SET locale = ?, updated_at = ? WHERE id = ?")
            .bind(locale.as_str())
            .bind(now())
            .bind(id.txt())
            .execute(self.pool())
            .await?;
        Ok(())
    }

    async fn active_admins_other_than(&self, id: ProfileId) -> Result<i64> {
        Ok(sqlx::query_scalar(
            "SELECT count(*) FROM profiles WHERE role = 'admin' AND disabled_at IS NULL AND id <> ?",
        )
        .bind(id.txt())
        .fetch_one(self.pool())
        .await?)
    }

    /// Changes a role, refusing to demote the last active admin.
    pub async fn set_role(&self, id: ProfileId, role: Role) -> Result<()> {
        if role != Role::Admin && self.active_admins_other_than(id).await? == 0 {
            return Err(StoreError::LastAdmin);
        }
        sqlx::query("UPDATE profiles SET role = ?, updated_at = ? WHERE id = ?")
            .bind(role.as_str())
            .bind(now())
            .bind(id.txt())
            .execute(self.pool())
            .await?;
        Ok(())
    }

    /// Disables or enables sign-in, refusing to disable the last active admin.
    pub async fn set_disabled(&self, id: ProfileId, disabled: bool) -> Result<()> {
        if disabled {
            let profile = self.get_profile(id).await?.ok_or(StoreError::NotFound)?;
            if profile.role == Role::Admin && self.active_admins_other_than(id).await? == 0 {
                return Err(StoreError::LastAdmin);
            }
        }
        let at = disabled.then(now);
        sqlx::query("UPDATE profiles SET disabled_at = ?, updated_at = ? WHERE id = ?")
            .bind(at)
            .bind(now())
            .bind(id.txt())
            .execute(self.pool())
            .await?;
        if disabled {
            self.delete_sessions_of(id).await?;
        }
        Ok(())
    }

    /// Deletes a profile and, by cascade, its accounts, credentials, ownerships, agents and sharing rules.
    pub async fn delete_profile(&self, id: ProfileId) -> Result<()> {
        let profile = self.get_profile(id).await?.ok_or(StoreError::NotFound)?;
        if profile.role == Role::Admin && self.active_admins_other_than(id).await? == 0 {
            return Err(StoreError::LastAdmin);
        }
        sqlx::query("DELETE FROM profiles WHERE id = ?")
            .bind(id.txt())
            .execute(self.pool())
            .await?;
        Ok(())
    }

    /// Signs a profile out everywhere.
    pub async fn delete_sessions_of(&self, id: ProfileId) -> Result<()> {
        sqlx::query("DELETE FROM sessions WHERE profile_id = ?")
            .bind(id.txt())
            .execute(self.pool())
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_support::store;

    fn pw() -> SecretString {
        SecretString::from("a long enough password".to_string())
    }

    async fn admin(store: &Store, name: &str) -> Profile {
        let password = pw();
        store
            .create_profile(NewProfile {
                username: name,
                display_name: "Admin",
                email: None,
                password: &password,
                role: Role::Admin,
            })
            .await
            .expect("create")
    }

    #[test]
    fn username_rules() {
        assert!(validate_username("example-user").is_ok());
        assert!(validate_username("ab").is_err());
        assert!(validate_username("Upper").is_err());
        assert!(validate_username("with space").is_err());
        assert!(validate_username(&"x".repeat(33)).is_err());
    }

    #[tokio::test]
    async fn create_and_sign_in() {
        let store = store().await;
        assert_eq!(store.count_profiles().await.expect("count"), 0);
        let p = admin(&store, "example-user").await;
        assert_eq!(store.count_profiles().await.expect("count"), 1);
        let ok = store.verify_login("example-user", &pw()).await.expect("verify");
        assert_eq!(ok.map(|p| p.id), Some(p.id));
        let bad = store
            .verify_login(
                "example-user",
                &SecretString::from("wrong password!!".to_string()),
            )
            .await
            .expect("verify");
        assert!(bad.is_none());
        assert!(
            store
                .verify_login("nobody", &pw())
                .await
                .expect("verify")
                .is_none()
        );
    }

    #[tokio::test]
    async fn duplicate_username_is_refused() {
        let store = store().await;
        admin(&store, "example-user").await;
        let password = pw();
        let err = store
            .create_profile(NewProfile {
                username: "example-user",
                display_name: "Other",
                email: None,
                password: &password,
                role: Role::Member,
            })
            .await
            .expect_err("must fail");
        assert!(matches!(err, StoreError::ProfileExists));
    }

    #[tokio::test]
    async fn last_admin_is_protected() {
        let store = store().await;
        let a = admin(&store, "admin-one").await;
        assert!(matches!(
            store.set_role(a.id, Role::Member).await,
            Err(StoreError::LastAdmin)
        ));
        assert!(matches!(
            store.set_disabled(a.id, true).await,
            Err(StoreError::LastAdmin)
        ));
        assert!(matches!(
            store.delete_profile(a.id).await,
            Err(StoreError::LastAdmin)
        ));
        let b = admin(&store, "admin-two").await;
        store
            .set_role(a.id, Role::Member)
            .await
            .expect("demote with another admin present");
        assert!(matches!(
            store.delete_profile(b.id).await,
            Err(StoreError::LastAdmin)
        ));
    }

    #[tokio::test]
    async fn disabled_profiles_cannot_sign_in() {
        let store = store().await;
        admin(&store, "admin-one").await;
        let password = pw();
        let m = store
            .create_profile(NewProfile {
                username: "member",
                display_name: "Member",
                email: Some("Member@Example.com"),
                password: &password,
                role: Role::Member,
            })
            .await
            .expect("create");
        assert_eq!(m.email.as_deref(), Some("member@example.com"));
        store.set_disabled(m.id, true).await.expect("disable");
        assert!(
            store
                .verify_login("member", &pw())
                .await
                .expect("verify")
                .is_none()
        );
        assert!(
            store
                .find_profile_by_login("member@example.com")
                .await
                .expect("find")
                .is_none()
        );
    }
}
