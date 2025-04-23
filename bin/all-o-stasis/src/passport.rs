use std::fmt;

use axum::{
    extract::{Path, Query, State},
    response::{IntoResponse, Redirect},
    routing::{get, post},
    Json, Router,
};
use axum_extra::extract::CookieJar;
use chrono::{DateTime, Utc};
use cookie::{time::Duration, Cookie};
use firestore::{path_camel_case, FirestoreResult};
use futures::{stream::BoxStream, TryStreamExt};
use otp::{
    types::{ObjectId, ObjectType},
    Operation, ROOT_OBJ_ID,
};
use rand::Rng;
use sendgrid::v3::*;
use serde::{Deserialize, Serialize};

use crate::{
    storage::{
        apply_object_updates, create_object, lookup_latest_snapshot, save_session,
        ACCOUNTS_VIEW_COLLECTION, SESSIONS_COLLECTION,
    },
    types::{Account, AccountRole},
    word_list::make_security_code,
    AppError, AppState,
};

pub type SessionId = String;

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Session {
    #[serde(alias = "_firestore_id")]
    pub id: Option<SessionId>,
    pub obj_id: ObjectId,
    #[serde(alias = "_firestore_created")]
    pub created_at: Option<DateTime<Utc>>,
    pub last_accessed_at: DateTime<Utc>,
}

impl fmt::Display for Session {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Session: {} obj_id={}",
            self.id.clone().expect("id cant be missing"),
            self.obj_id
        )
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
    Unconfirmed,
    Valid,
    Expired,
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
    const CHARSET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ\
                             abcdefghijklmnopqrstuvwxyz\
                             0123456789";
    let mut rng = rand::rng();

    (0..len)
        .map(|_| {
            let idx = rng.random_range(0..CHARSET.len());
            CHARSET[idx] as char
        })
        .collect()
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
    let body = [
        format!("Verify your email to log on to the {api_domain}"),
        "<br/>".to_string(),
        "We have received a login attempt with the following code:".to_string(),
        "<br/>".to_string(),
        security_code,
        "<br/>".to_string(),
        "complete the login process, please click the URL below:".to_string(),
        "<br/>".to_string(),
        format!("<a href={confirmation_url}>{confirmation_url}</a>"),
        "<br/>".to_string(),
        "copy and paste this URL into your browser.".to_string(),
    ];

    let p = Personalization::new(Email::new(email.clone()));

    let m = Message::new(Email::new("auth@boulderhalle.app".to_string()))
        .set_subject(&subject)
        .add_content(
            Content::new()
                .set_content_type("text/html")
                .set_value(body.join("<br/>")),
        )
        .add_personalization(p);

    let api_key = ::std::env::var("SG_API_KEY").expect("no sendgrid api key");
    let sender = Sender::new(api_key, None);
    tracing::debug!("sending message to {email}");
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
    let maybe_account_id: Result<ObjectId, AppError> = match accounts.first() {
        Some(account) => Ok(account.clone().id.expect("object has no id")),
        None => {
            let account = Account {
                id: None,
                email: payload.email.clone(),
                role: AccountRole::User,
                login: "aaa".to_string(),
                name: None,
            };
            let value = serde_json::to_value(account.clone()).expect("serialising account");
            let obj = create_object(
                &state,
                &gym,
                ROOT_OBJ_ID.to_owned(),
                ObjectType::Account,
                value,
            )
            .await?;

            Ok(obj.id())
        }
    };
    let account_id = maybe_account_id.or(Err(AppError::Query()))?;

    // 2. Create a new Passport object.
    let security_code = make_security_code().expect("security code creation failed");
    let confirmation_token = new_id(16);

    let passport = Passport {
        account_id: account_id.clone(),
        security_code: security_code.clone(),
        confirmation_token: confirmation_token.clone(),
        validity: PassportValidity::Unconfirmed,
    };
    let value = serde_json::to_value(passport.clone()).expect("serialising passport");
    let obj = create_object(&state, &gym, account_id, ObjectType::Passport, value).await?;

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
    jar: CookieJar,
) -> Result<impl IntoResponse, AppError> {
    let snapshot = lookup_latest_snapshot(&state, &gym, &pport.passport_id.clone()).await?;
    let passport: Passport = serde_json::from_value(snapshot.content).or(Err(
        AppError::ParseError("failed to parse object into Passport".to_string()),
    ))?;

    if pport.confirmation_token != passport.confirmation_token {
        Err(AppError::NotAuthorized())
    } else {
        // create a new session for the account in the Passport object
        let session = save_session(
            &state,
            &gym,
            &Session {
                id: None,
                obj_id: passport.account_id,
                created_at: None,
                last_accessed_at: chrono::offset::Utc::now(),
            },
            &new_id(80),
        )
        .await?
        .ok_or_else(AppError::Query)?; // FIXME error

        // mark as valid
        let op = Operation::Set {
            path: "validity".to_string(),
            value: Some(
                serde_json::to_value(&PassportValidity::Valid).expect("serialising PVValid"),
            ),
        };
        let _ = apply_object_updates(
            &state,
            &gym,
            pport.passport_id,
            snapshot.revision_id,
            ROOT_OBJ_ID.to_string(),
            [op].to_vec(),
            false,
        )
        .await?;

        let cookie = Cookie::build(("session", session.id.expect("session has id")))
            .path("/")
            .max_age(Duration::weeks(52))
            .secure(true) // TODO not sure about this
            .http_only(true);

        // FIXME we need the app url
        Ok((
            jar.add(cookie),
            Redirect::permanent("https://all-o-stasis-oxy.vercel.app/email-confirmed"),
        ))
    }
}

async fn await_passport_confirmation(
    State(state): State<AppState>,
    Path(gym): Path<String>,
    Query(pport): Query<AwaitPassportConfirmation>,
    jar: CookieJar,
) -> Result<impl IntoResponse, AppError> {
    let (account_id, revision_id) = loop {
        let snapshot = lookup_latest_snapshot(&state, &gym, &pport.passport_id.clone()).await?;
        let passport: Passport = serde_json::from_value(snapshot.content).or(Err(
            AppError::ParseError("failed to parse object into Passport".to_string()),
        ))?;

        match passport.validity {
            PassportValidity::Valid => {
                break (passport.account_id, snapshot.revision_id);
            }
            PassportValidity::Unconfirmed => {
                // TODO should we stop trying after some time?
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            }
            PassportValidity::Expired => {
                return Err(AppError::NotAuthorized());
            }
        }
    };

    let op = Operation::Set {
        path: "validity".to_string(),
        value: Some(
            serde_json::to_value(&PassportValidity::Expired).expect("serialising PVExpired"),
        ),
    };
    let _ = apply_object_updates(
        &state,
        &gym,
        pport.passport_id,
        revision_id,
        ROOT_OBJ_ID.to_string(),
        [op].to_vec(),
        false,
    )
    .await?;

    // find session created in confirmation
    let parent_path = state.db.parent_path("gyms", gym)?;
    let sessions_stream: BoxStream<FirestoreResult<Session>> = state
        .db
        .fluent()
        .select()
        .from(SESSIONS_COLLECTION)
        .parent(&parent_path)
        .filter(|q| {
            q.for_all([q
                .field(path_camel_case!(Session::obj_id))
                .eq(account_id.clone())])
        })
        .limit(1)
        .obj()
        .stream_query_with_errors()
        .await?;

    let sessions: Vec<Session> = sessions_stream.try_collect().await?;
    let session_id = match sessions.first() {
        Some(session) => Ok(session.clone().id.expect("session has id")),
        None => Err(AppError::NotAuthorized()),
    }?;

    // respond with the session cookie and status=200
    let cookie = Cookie::build(("session", session_id))
        .path("/")
        .max_age(Duration::weeks(52))
        .secure(true) // TODO not sure about this
        .http_only(true);
    Ok(jar.add(cookie))
}
