use std::{collections::HashMap, fmt};

use axum::{
    extract::{Path, Query, State},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use chrono::{DateTime, Duration, Utc};
use firestore::{path_camel_case, FirestoreResult};
use futures::{stream::BoxStream, TryStreamExt};
use otp::{
    types::{ObjectId, ObjectType, Patch},
    Object, Operation, ROOT_OBJ_ID, ROOT_PATH, ZERO_REV_ID,
};
use sendgrid::v3::*;
use serde::{Deserialize, Serialize};

use crate::{
    storage::{
        apply_object_updates, lookup_latest_snapshot, save_session, store_patch,
        ACCOUNTS_VIEW_COLLECTION, OBJECTS_COLLECTION,
    },
    types::Account,
    word_list::make_security_code,
    AppError, AppState,
};

pub type SessionId = String;

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Session {
    pub id: SessionId,
    pub obj_id: ObjectId,
    #[serde(alias = "_firestore_created")]
    pub created_at: Option<DateTime<Utc>>,
    pub last_accessed_at: DateTime<Utc>,
}

impl fmt::Display for Session {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Session: {} obj_id={}", self.id, self.obj_id)
    }
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct Passport {
    account_id: ObjectId,
    security_code: String,
    confirmation_token: String,
    validity: PassportValidity,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
enum PassportValidity {
    PVUnconfirmed,
    PVValid,
    PVExpired,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct CreatePassportBody {
    email: String,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct CreatePassportResponse {
    passport_id: String,
    security_code: String,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct ConfirmPassport {
    passport_id: String,
    confirmation_token: String,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct AwaitPassportConfirmation {
    passport_id: String,
}

pub(crate) fn passport_routes() -> Router<AppState> {
    Router::new()
        .route("/{gym}/login", post(create_passport))
        .route("/{gym}/login/confirm", get(confirm_passport))
        .route("/{gym}/login/verify", get(await_passport_confirmation))
}

fn new_id(len: usize) -> String {
    todo!()
}

async fn send_email(
    email: String,
    api_domain: String,
    passport_id: String,
    security_code: String,
    confirmation_token: String,
) -> Result<(), AppError> {
    let confirmation_url = format!(
        "https://apiv2.boulderhalle.app/{api_domain}/login/confirm?passportId={passport_id}&confirmationToken={confirmation_token}",
    );
    let subject = format!("{api_domain} Login Verification (code: \"{security_code}\")",);
    let body = format!(
        "Verify your email to log on to the {api_domain}\n\
        We have received a login attempt with the following code: \n{security_code}\n\
        complete the login process, please click the URL below: \n{confirmation_url}\n\
        copy and paste this URL into your browser."
    );

    let m = Message::new(Email::new(email))
        .set_subject(&subject)
        .add_content(Content::new().set_content_type("text/html").set_value(body));

    // TODO
    // let partToContent :: Part -> Value
    //     partToContent part = object
    //         [ "type" .= ("text/plain" :: Text) -- partType part
    //         , "value" .= LT.decodeUtf8 (partContent part)
    //         ]
    //
    // let toPersonalization addr = object
    //         [ "to" .= [ object [ "email" .= addressEmail addr ] ]
    //         , "subject" .= subject
    //         ]

    let api_key = ::std::env::var("SG_API_KEY").unwrap();
    let sender = Sender::new(api_key, None);
    let code = sender.send(&m).await;
    tracing::debug!("{:?}", code);

    Ok(())
}

async fn create_passport(
    State(state): State<AppState>,
    Path(gym): Path<String>,
    Json(payload): axum::extract::Json<CreatePassportBody>,
) -> Result<Json<CreatePassportResponse>, AppError> {
    let parent_path = state.db.parent_path("gyms", gym.clone())?;

    // 1. Lookup account by email. If no such account exists, create a new one
    let account_stream: BoxStream<FirestoreResult<Account>> = state
        .db
        .fluent()
        .select()
        .from(ACCOUNTS_VIEW_COLLECTION)
        .parent(&parent_path)
        .filter(|q| {
            q.for_all([q
                .field(path_camel_case!(Account::email))
                .eq(payload.email.clone())])
        })
        .limit(1)
        .obj()
        .stream_query_with_errors()
        .await?;

    let accounts: Vec<Account> = account_stream.try_collect().await?;
    let account = match accounts.first() {
        Some(account) => Ok(account.clone()),
        None => {
            // TODO create a new account
            Err(AppError::Query())
        }
    }?;
    let account_id = account.id.expect("object in view has no id");

    // 2. Create a new Passport object.
    let security_code = make_security_code().expect("TODO");
    let confirmation_token = new_id(16);

    // TODO refactor into create_object (with payload)
    let obj = Object::new(ObjectType::Passport, ROOT_OBJ_ID.to_owned());
    let obj: Option<Object> = state
        .db
        .fluent()
        .insert()
        .into(OBJECTS_COLLECTION)
        .generate_document_id()
        .parent(&parent_path)
        .object(&obj)
        .execute()
        .await?;
    let obj = obj.ok_or_else(AppError::Query)?;

    let passport = Passport {
        account_id: account_id.clone(),
        security_code: security_code.clone(),
        confirmation_token: confirmation_token.clone(),
        validity: PassportValidity::PVUnconfirmed,
    };
    let op = Operation::Set {
        path: ROOT_PATH.to_string(),
        value: Some(serde_json::to_value(passport.clone()).expect("serialising passport")),
    };
    let patch = Patch {
        object_id: obj.id(),
        revision_id: ZERO_REV_ID,
        author_id: account_id,
        created_at: None,
        operation: op,
    };
    let patch = store_patch(&state, &gym, &patch).await?;
    let _ = patch.ok_or_else(AppError::Query)?;

    let passport_id = obj.id();

    // 3. Send email
    send_email(
        payload.email,
        gym.clone(),
        passport_id.clone(),
        security_code.clone(),
        confirmation_token.clone(),
    )
    .await?;

    // 4. Send response
    Ok(Json(CreatePassportResponse {
        passport_id,
        security_code,
    }))
}

async fn confirm_passport(
    State(state): State<AppState>,
    Path(gym): Path<String>,
    Query(pport): Query<ConfirmPassport>,
) -> Result<impl IntoResponse, AppError> {
    let snapshot = lookup_latest_snapshot(&state, &gym, &pport.passport_id.clone()).await?;
    let passport: Passport = serde_json::from_value(snapshot.content).or(Err(
        AppError::ParseError("failed to parse object into Passport".to_string()),
    ))?;

    if pport.confirmation_token != passport.confirmation_token {
        return Err(AppError::NotAuthorized());
    } else {
        // mark as valid
        let op = Operation::Set {
            path: "validity".to_string(),
            value: Some(
                serde_json::to_value(&PassportValidity::PVValid).expect("serialising PVExpired"),
            ),
        };
        apply_object_updates(
            &state,
            &gym,
            pport.passport_id,
            snapshot.revision_id,
            ROOT_OBJ_ID.to_string(),
            [op].to_vec(),
            false,
        );

        // -- Apparently this is how you do a 30x redirect in Servant…
        // throwError $ err301
        //     { errHeaders = [("Location", T.encodeUtf8 (_pcAppDomain pc) <> "/email-confirmed")]
        //     }

        Ok(())
    }
}

async fn await_passport_confirmation(
    State(state): State<AppState>,
    Path(gym): Path<String>,
    Query(pport): Query<AwaitPassportConfirmation>,
    // jar: CookieJar,
) -> Result<impl IntoResponse, AppError> {
    let snapshot = lookup_latest_snapshot(&state, &gym, &pport.passport_id.clone()).await?;
    let passport: Passport = serde_json::from_value(snapshot.content).or(Err(
        AppError::ParseError("failed to parse object into Passport".to_string()),
    ))?;

    let (account_id, revision_id) = loop {
        match passport.validity {
            PassportValidity::PVValid => {
                break (passport.account_id, snapshot.revision_id);
            }
            PassportValidity::PVUnconfirmed => {
                // sleep a bit and then retry
                tokio::time::sleep(std::time::Duration::from_secs(500)).await;
            }
            PassportValidity::PVExpired => {
                return Err(AppError::NotAuthorized());
            }
        }
    };

    let op = Operation::Set {
        path: "validity".to_string(),
        value: Some(
            serde_json::to_value(&PassportValidity::PVExpired).expect("serialising PVExpired"),
        ),
    };
    apply_object_updates(
        &state,
        &gym,
        pport.passport_id,
        revision_id,
        ROOT_OBJ_ID.to_string(),
        [op].to_vec(),
        false,
    );

    // The Passport object is valid.
    // Create a new session for the account in the Passport object.
    let now = chrono::offset::Utc::now();
    let session_id = new_id(80);
    save_session(
        &state,
        &gym,
        &Session {
            id: session_id,
            obj_id: account_id,
            created_at: Some(now),
            last_accessed_at: now,
        },
    )
    .await?;

    // setCookie <- mkSetCookie sessId

    // -- 4. Respond with the session cookie and status=200
    // pure $ addHeader setCookie NoContent
    let cookie = Cookie::build(("session", session_id.clone()))
        // .domain("api?")
        .path("/")
        .max_age(Duration::weeks(52))
        .secure(true) // TODO not sure about this
        .http_only(true);

    // FIXME just add to header?
    Ok(jar.add(cookie))
}
