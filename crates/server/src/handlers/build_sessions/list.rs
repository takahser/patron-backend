use std::sync::Arc;

use axum::{
    extract::{Query, State},
    Extension, Json,
};
use axum_derive_error::ErrorResponse;
use db::{
    build_session, ColumnTrait, DatabaseConnection, DbErr, EntityTrait, PrimitiveDateTime,
    QueryFilter, QueryOrder, QuerySelect,
};
use derive_more::{Display, Error, From};
use futures_util::TryStreamExt;
use serde::Serialize;

use crate::{auth::AuthenticatedUserId, pagination::Pagination};

#[derive(Serialize)]
pub struct BuildSessionData {
    pub id: i64,
    pub source_code_id: i64,
    pub status: build_session::Status,
    pub code_hash: Option<String>,
    pub timestamp: i64,
}

#[derive(ErrorResponse, Display, From, Error)]
pub(super) enum BuildSessionListError {
    DatabaseError(DbErr),
}

pub(super) async fn list(
    Extension(current_user): Extension<AuthenticatedUserId>,
    State(db): State<Arc<DatabaseConnection>>,
    Query(pagination): Query<Pagination>,
) -> Result<Json<Vec<BuildSessionData>>, BuildSessionListError> {
    build_session::Entity::find()
        .select_only()
        .columns([
            build_session::Column::Id,
            build_session::Column::SourceCodeId,
            build_session::Column::Status,
            build_session::Column::CodeHash,
            build_session::Column::CreatedAt,
        ])
        .filter(build_session::Column::UserId.eq(current_user.id()))
        .limit(pagination.limit())
        .offset(pagination.offset())
        .order_by_desc(build_session::Column::Id)
        .into_tuple::<(
            i64,
            i64,
            build_session::Status,
            Option<Vec<u8>>,
            PrimitiveDateTime,
        )>()
        .stream(&*db)
        .await?
        .err_into()
        .and_then(
            |(id, source_code_id, status, code_hash, timestamp)| async move {
                Ok(BuildSessionData {
                    id,
                    source_code_id,
                    status,
                    code_hash: code_hash.map(hex::encode),
                    timestamp: timestamp.assume_utc().unix_timestamp(),
                })
            },
        )
        .try_collect()
        .await
        .map(Json)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::testing::{create_database, ResponseBodyExt};

    use assert_json::{assert_json, validators};
    use axum::{body::Body, http::Request};
    use common::config::Config;
    use db::{
        build_session, public_key, source_code, token, user, ActiveValue, DatabaseConnection,
        EntityTrait, PrimitiveDateTime,
    };
    use tower::ServiceExt;

    async fn create_test_env(
        db: &DatabaseConnection,
    ) -> (String, i64, PrimitiveDateTime, PrimitiveDateTime) {
        let user = user::Entity::insert(user::ActiveModel::default())
            .exec_with_returning(db)
            .await
            .expect("unable to create user");

        let (model, token) = token::generate_token(user.id);

        token::Entity::insert(model)
            .exec_without_returning(db)
            .await
            .expect("unable to insert token");

        public_key::Entity::insert(public_key::ActiveModel {
            user_id: ActiveValue::Set(user.id),
            address: ActiveValue::Set(Vec::new()),
            ..Default::default()
        })
        .exec_without_returning(db)
        .await
        .expect("unable to create public key");

        let source_code_id = source_code::Entity::insert(source_code::ActiveModel {
            user_id: ActiveValue::Set(Some(user.id)),
            archive_hash: ActiveValue::Set(vec![0; 32]),
            ..Default::default()
        })
        .exec_with_returning(db)
        .await
        .expect("unable to create source code")
        .id;

        let first_ts = build_session::Entity::insert(build_session::ActiveModel {
            user_id: ActiveValue::Set(Some(user.id)),
            source_code_id: ActiveValue::Set(source_code_id),
            status: ActiveValue::Set(build_session::Status::Completed),
            cargo_contract_version: ActiveValue::Set(String::from("3.0.0")),
            rustc_version: ActiveValue::Set(String::from("1.69.0")),
            code_hash: ActiveValue::Set(Some(vec![0; 32])),
            ..Default::default()
        })
        .exec_with_returning(db)
        .await
        .expect("unable to insert build session")
        .created_at;

        let second_ts = build_session::Entity::insert(build_session::ActiveModel {
            user_id: ActiveValue::Set(Some(user.id)),
            source_code_id: ActiveValue::Set(source_code_id),
            status: ActiveValue::Set(build_session::Status::New),
            cargo_contract_version: ActiveValue::Set(String::from("3.0.0")),
            rustc_version: ActiveValue::Set(String::from("1.69.0")),
            ..Default::default()
        })
        .exec_with_returning(db)
        .await
        .expect("unable to insert build session")
        .created_at;

        (token, source_code_id, first_ts, second_ts)
    }

    #[tokio::test]
    async fn successful() {
        let db = create_database().await;

        let (token, source_code_id, first_ts, second_ts) = create_test_env(&db).await;

        let response = crate::app_router(Arc::new(db), Arc::new(Config::new().unwrap()))
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/buildSessions")
                    .header("Authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        let first_unix = first_ts.assume_utc().unix_timestamp();
        let second_unix = second_ts.assume_utc().unix_timestamp();

        assert_json!(response.json().await, [
            {
                "id": 2,
                "source_code_id": source_code_id,
                "status": "new",
                "code_hash": validators::null(),
                "timestamp": second_unix,
            },
            {
                "id": 1,
                "source_code_id": source_code_id,
                "status": "completed",
                "code_hash": hex::encode([0; 32]),
                "timestamp": first_unix
            }
        ]);
    }
}
