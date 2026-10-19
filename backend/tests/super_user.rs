use admin::{modules::identity::application::create_super_user, platform::error::AppError};
use sqlx::PgPool;

#[sqlx::test]
async fn is_created_once_in_the_master_tenant(db: PgPool) -> Result<(), AppError> {
    assert!(create_super_user(&db, "Root@Platform.test", "first-password-123".into()).await?);
    assert!(!create_super_user(&db, "root@platform.test", "second-password-456".into()).await?);

    let (email, is_master): (String, bool) = sqlx::query_as(
        "SELECT u.email, t.is_master FROM users u JOIN tenants t ON t.id = u.tenant_id",
    )
    .fetch_one(&db)
    .await?;
    assert_eq!(email, "root@platform.test");
    assert!(is_master);
    Ok(())
}

#[sqlx::test]
async fn an_existing_super_user_keeps_their_password(db: PgPool) -> Result<(), AppError> {
    create_super_user(&db, "root@platform.test", "first-password-123".into()).await?;
    let before: String = sqlx::query_scalar("SELECT password_hash FROM users")
        .fetch_one(&db)
        .await?;

    create_super_user(&db, "root@platform.test", "second-password-456".into()).await?;
    let after: String = sqlx::query_scalar("SELECT password_hash FROM users")
        .fetch_one(&db)
        .await?;

    assert_eq!(before, after);
    Ok(())
}

#[sqlx::test]
async fn short_passwords_are_refused(db: PgPool) -> Result<(), AppError> {
    let result = create_super_user(&db, "root@platform.test", "too-short".into()).await;
    assert!(matches!(result, Err(AppError::Invalid(_))));

    let users: i64 = sqlx::query_scalar("SELECT count(*) FROM users")
        .fetch_one(&db)
        .await?;
    assert_eq!(users, 0);
    Ok(())
}
