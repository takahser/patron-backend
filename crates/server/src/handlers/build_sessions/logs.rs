use std::sync::Arc;

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use axum_derive_error::ErrorResponse;
use db::{
    build_session, log, ColumnTrait, DatabaseConnection, DbErr, EntityTrait, QueryFilter,
    QueryOrder, QuerySelect, QueryTrait, TransactionErrorExt, TransactionTrait,
};
use derive_more::{Display, Error, From};
use futures_util::TryStreamExt;
use serde::{Deserialize, Serialize};

use crate::hex_hash::HexHash;

#[derive(ErrorResponse, Display, From, Error)]
pub(super) enum BuildSessionLogsError {
    DatabaseError(DbErr),

    #[status(StatusCode::BAD_REQUEST)]
    #[display(fmt = "unknown identifier format, use either code hash or numeric id")]
    UnknownIdFormat,

    #[status(StatusCode::NOT_FOUND)]
    #[display(fmt = "code not found")]
    NotFound,
}

#[derive(Deserialize)]
pub(super) struct BuildSessionLogsQuery {
    #[serde(default)]
    position: Option<i64>,
}

#[derive(Serialize)]
pub(super) struct LogEntry {
    id: i64,
    text: String,
}

#[derive(Serialize)]
pub(super) struct BuildSessionLogsResponse {
    logs: Vec<LogEntry>,
}

pub(super) async fn logs(
    Path(id): Path<String>,
    State(db): State<Arc<DatabaseConnection>>,
    Query(query): Query<BuildSessionLogsQuery>,
) -> Result<Json<BuildSessionLogsResponse>, BuildSessionLogsError> {
    db.transaction(|txn| {
        Box::pin(async move {
            let logs = log::Entity::find()
                .select_only()
                .columns([log::Column::Id, log::Column::Text])
                .filter(match serde_plain::from_str::<HexHash>(&id) {
                    Ok(val) => {
                        let id = build_session::Entity::find()
                            .select_only()
                            .column(build_session::Column::Id)
                            .filter(build_session::Column::CodeHash.eq(&val.0[..]))
                            .into_tuple::<i64>()
                            .one(txn)
                            .await?
                            .ok_or(BuildSessionLogsError::NotFound)?;

                        log::Column::BuildSessionId.eq(id)
                    }
                    Err(_) => {
                        let id = id
                            .parse::<i64>()
                            .map_err(|_| BuildSessionLogsError::UnknownIdFormat)?;

                        log::Column::BuildSessionId.eq(id)
                    }
                })
                .apply_if(query.position, |query, position| {
                    query.filter(log::Column::Id.gt(position))
                })
                .order_by_asc(log::Column::Id)
                .into_tuple::<(i64, String)>()
                .stream(txn)
                .await?
                .map_ok(|(id, text)| LogEntry { id, text })
                .try_collect()
                .await?;

            Ok(Json(BuildSessionLogsResponse { logs }))
        })
    })
    .await
    .into_raw_result()
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::testing::{create_database, ResponseBodyExt};

    use assert_json::assert_json;
    use axum::{body::Body, http::Request};
    use common::config::Config;
    use db::{build_session, log, source_code, user, ActiveValue, DatabaseConnection, EntityTrait};
    use tower::ServiceExt;

    async fn create_test_env(db: &DatabaseConnection) -> i64 {
        let user = user::Entity::insert(user::ActiveModel::default())
            .exec_with_returning(db)
            .await
            .expect("unable to create user");

        let source_code_id = source_code::Entity::insert(source_code::ActiveModel {
            user_id: ActiveValue::Set(Some(user.id)),
            archive_hash: ActiveValue::Set(vec![0; 32]),
            ..Default::default()
        })
        .exec_with_returning(db)
        .await
        .expect("unable to create source code")
        .id;

        let build_session_id = build_session::Entity::insert(build_session::ActiveModel {
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
        .id;

        log::Entity::insert_many([
            log::ActiveModel {
                build_session_id: ActiveValue::Set(build_session_id),
                text: ActiveValue::Set(String::from("First log\n")),
                ..Default::default()
            },
            log::ActiveModel {
                build_session_id: ActiveValue::Set(build_session_id),
                text: ActiveValue::Set(String::from("Second log\n")),
                ..Default::default()
            },
            log::ActiveModel {
                build_session_id: ActiveValue::Set(build_session_id),
                text: ActiveValue::Set(String::from("Third log")),
                ..Default::default()
            },
        ])
        .exec_without_returning(db)
        .await
        .expect("unable to insert logs");

        build_session_id
    }

    #[tokio::test]
    async fn successful_by_id() {
        let db = create_database().await;

        let build_session_id = create_test_env(&db).await;

        let response = crate::app_router(Arc::new(db), Arc::new(Config::new().unwrap()))
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!("/buildSessions/logs/{}", build_session_id))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_json!(response.json().await, {
            "logs": [
                {
                    "id": 1,
                    "text": "First log\n"
                },
                {
                    "id": 2,
                    "text": "Second log\n"
                },
                {
                    "id": 3,
                    "text": "Third log"
                }
            ]
        });
    }

    #[tokio::test]
    async fn successful_by_code_hash() {
        let db = create_database().await;

        create_test_env(&db).await;

        let response = crate::app_router(Arc::new(db), Arc::new(Config::new().unwrap()))
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!("/buildSessions/logs/{}", hex::encode([0; 32])))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_json!(response.json().await, {
            "logs": [
                {
                    "id": 1,
                    "text": "First log\n"
                },
                {
                    "id": 2,
                    "text": "Second log\n"
                },
                {
                    "id": 3,
                    "text": "Third log"
                }
            ]
        });
    }

    #[tokio::test]
    async fn position() {
        let db = create_database().await;

        let build_session_id = create_test_env(&db).await;

        let response = crate::app_router(Arc::new(db), Arc::new(Config::new().unwrap()))
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!(
                        "/buildSessions/logs/{}?position=2",
                        build_session_id
                    ))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_json!(response.json().await, {
            "logs": [
                {
                    "id": 3,
                    "text": "Third log"
                }
            ]
        });
    }

    #[tokio::test]
    async fn unknown() {
        let db = create_database().await;

        let response = crate::app_router(Arc::new(db), Arc::new(Config::new().unwrap()))
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/buildSessions/logs/1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_json!(response.json().await, {
            "logs": []
        });
    }
}
