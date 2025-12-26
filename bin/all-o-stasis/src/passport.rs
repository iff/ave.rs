use std::fmt;

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    response::{IntoResponse, Redirect},
    routing::{get, post},
};
use axum_extra::extract::CookieJar;
use chrono::{DateTime, Utc};
use cookie::{Cookie, SameSite, time::Duration};
use firestore::{FirestoreResult, path_camel_case};
use futures::{TryStreamExt, stream::BoxStream};
use otp::{ObjectId, Operation};
use rand::Rng;
use serde::{Deserialize, Serialize};

use crate::{
    AppError, AppState,
    storage::{apply_object_updates, create_object},
    types::{Account, AccountRole, AccountsView, ObjectType, Snapshot},
    word_list::make_security_code,
};

mod maileroo {
    use crate::AppError;
    use reqwest::blocking::Client;
    use reqwest::header::{CONTENT_TYPE, HeaderMap, HeaderValue};
    use serde::{Deserialize, Serialize};

    const MAILEROO_API_BASE_URL: &str = "https://smtp.maileroo.com/api/v2";

    #[derive(Serialize, Debug)]
    struct EmailAddress {
        address: String,
        display_name: Option<String>,
    }

    #[derive(Serialize, Debug)]
    struct EmailData {
        to: Vec<EmailAddress>,
        from: EmailAddress,
        subject: String,
        html: String,
    }

    #[derive(Deserialize)]
    struct ResponseData {
        #[allow(dead_code)]
        reference_id: String,
    }

    #[derive(Deserialize)]
    struct Response {
        #[allow(dead_code)]
        data: ResponseData,
        message: String,
        success: bool,
    }

    pub struct Email {
        data: EmailData,
    }

    impl Email {
        pub fn new(to: String, subject: String, body: String) -> Self {
            Self {
                data: EmailData {
                    to: vec![EmailAddress {
                        address: to,
                        display_name: None,
                    }],
                    from: EmailAddress {
                        address: String::from("auth@boulderhalle.app"),
                        display_name: None,
                    },
                    subject,
                    html: body,
                },
            }
        }

        pub fn send(self) -> Result<(), AppError> {
            let api_key = ::std::env::var("MAILEROO_API_KEY")
                .or(Err(AppError::Passport(String::from("no maileroo api key"))))?;
            let request = serde_json::to_string(&self.data).expect("serialisation to json");
            let client = Client::new();
            let mut headers = HeaderMap::new();
            headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
            let response = client
                .post(format!("{MAILEROO_API_BASE_URL}/emails"))
                .headers(headers)
                .bearer_auth(api_key.to_owned())
                .body(request)
                .send();
            match response {
                Ok(response) => {
                    let r = response.json::<Response>().map_err(|e| {
                        AppError::Passport(format!("failed to parse response: {e:?}"))
                    })?;
                    if r.success {
                        Ok(())
                    } else {
                        Err(AppError::Passport(format!(
                            "sending email failed: {}",
                            r.message
                        )))
                    }
                }
                Err(error) => {
                    tracing::error!("{:?}", error);
                    Err(AppError::Passport(format!("{error:?}")))
                }
            }
        }
    }
}

pub type SessionId = String;

#[derive(Serialize, Deserialize)]
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

impl Session {
    const COLLECTION: &str = "sessions";

    pub async fn lookup(
        state: &AppState,
        gym: &String,
        object_id: ObjectId,
    ) -> Result<Self, AppError> {
        let parent_path = state.db.parent_path("gyms", gym)?;
        state
            .db
            .fluent()
            .select()
            .by_id_in(Self::COLLECTION)
            .parent(&parent_path)
            .obj()
            .one(&object_id)
            .await?
            .ok_or(AppError::NoSession())
    }

    pub async fn store(
        &self,
        state: &AppState,
        gym: &String,
        session_id: &str,
    ) -> Result<Session, AppError> {
        let parent_path = state.db.parent_path("gyms", gym)?;
        let p: Option<Session> = state
            .db
            .fluent()
            .update()
            .in_col(Session::COLLECTION)
            .document_id(session_id)
            .parent(&parent_path)
            .object(self)
            .execute()
            .await?;

        match p {
            Some(p) => {
                tracing::debug!("storing session: {p}");
                Ok(p)
            }
            None => {
                tracing::warn!("failed to update session: {self} (no such object exists");
                Err(AppError::NoSession())
            }
        }
    }

    pub async fn delete(state: &AppState, gym: &String, session_id: &str) -> Result<(), AppError> {
        let parent_path = state.db.parent_path("gyms", gym)?;
        state
            .db
            .fluent()
            .delete()
            .from(Self::COLLECTION)
            .parent(&parent_path)
            .document_id(session_id)
            .execute()
            .await?;
        Ok(())
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Passport {
    account_id: ObjectId,
    security_code: String,
    confirmation_token: String,
    validity: PassportValidity,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
enum PassportValidity {
    Unconfirmed,
    Valid,
    Expired,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreatePassportBody {
    email: String,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreatePassportResponse {
    passport_id: String,
    security_code: String,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConfirmPassport {
    passport_id: String,
    confirmation_token: String,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AwaitPassportConfirmation {
    passport_id: String,
}

pub(crate) fn routes() -> Router<AppState> {
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
    api_host: String,
    api_domain: String,
    passport_id: String,
    security_code: String,
    confirmation_token: String,
) -> Result<(), AppError> {
    let confirmation_url = format!(
        "{api_host}/{api_domain}/login/confirm?passportId={passport_id}&confirmationToken={confirmation_token}",
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

    maileroo::Email::new(email.clone(), subject, body.join("\n")).send()
}

async fn create_passport(
    State(state): State<AppState>,
    Path(gym): Path<String>,
    Json(payload): axum::extract::Json<CreatePassportBody>,
) -> Result<Json<CreatePassportResponse>, AppError> {
    // 1. Lookup account by email. If no such account exists, create a new one
    let account = AccountsView::with_email(&state, gym.clone(), payload.email.clone()).await?;
    let maybe_account_id: Result<ObjectId, AppError> = match account {
        Some(account) => Ok(account.id.clone().expect("object has no id")),
        None => {
            let account = Account {
                id: None,
                email: payload.email.clone(),
                role: AccountRole::User,
                login: "to be removed".to_string(),
                name: None,
            };
            let value = serde_json::to_value(account).expect("serialising account");
            let obj = create_object(
                &state,
                &gym,
                String::from(""), // TODO fine?
                ObjectType::Account,
                &value,
            )
            .await?;

            Ok(obj.id.clone())
        }
    };
    let account_id = maybe_account_id.or(Err(AppError::Query(
        "create_passport: failed to create/get account id".to_string(),
    )))?;

    // 2. Create a new Passport object.
    let security_code = make_security_code().expect("security code creation failed");
    let confirmation_token = new_id(16);

    let passport = Passport {
        account_id: account_id.clone(),
        security_code: security_code.clone(),
        confirmation_token: confirmation_token.clone(),
        validity: PassportValidity::Unconfirmed,
    };
    let value = serde_json::to_value(passport).expect("serialising passport");
    let obj = create_object(&state, &gym, account_id, ObjectType::Passport, &value).await?;

    let passport_id = obj.id.clone();

    // 3. Send email
    send_email(
        payload.email,
        state.api_host,
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
    let snapshot = Snapshot::lookup_latest(&state, &gym, &pport.passport_id.clone()).await?;
    let passport: Passport = serde_json::from_value(snapshot.content).or(Err(
        AppError::ParseError("failed to parse object into Passport".to_string()),
    ))?;

    if pport.confirmation_token != passport.confirmation_token {
        Err(AppError::NotAuthorized())
    } else {
        // create a new session for the account in the Passport object
        let session = Session {
            id: None,
            obj_id: passport.account_id,
            created_at: None,
            last_accessed_at: chrono::offset::Utc::now(),
        }
        .store(&state, &gym, &new_id(80))
        .await?;

        // mark as valid
        let op = Operation::try_new_set(
            "validity",
            Some(serde_json::to_value(&PassportValidity::Valid).expect("serialising PVValid")),
        )?;
        let _ = apply_object_updates(
            &state,
            &gym,
            pport.passport_id,
            snapshot.revision_id,
            String::from(""), // TODO fine?
            [op].to_vec(),
        )
        .await?;

        let cookie = Cookie::build(("session", session.id.expect("session has id")))
            .path("/")
            .max_age(Duration::weeks(52))
            .secure(true) // TODO not sure about this
            .same_site(SameSite::None)
            .http_only(true);

        let redirect_host = if state.api_host.contains("dev") {
            String::from("https://dev.boulderhalle.app")
        } else {
            // NOTE for now just map gym => https://gym.boulderhalle.app/email-confirmed
            if gym == "leutsch" {
                format!("https://minimum-{gym}.boulderhalle.app")
            } else {
                format!("https://{gym}.boulderhalle.app")
            }
        };
        Ok((
            jar.add(cookie),
            Redirect::permanent(format!("{redirect_host}/email-confirmed").as_str()),
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
        let snapshot = Snapshot::lookup_latest(&state, &gym, &pport.passport_id.clone()).await?;
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

    let op = Operation::try_new_set(
        "validity",
        Some(serde_json::to_value(&PassportValidity::Expired).expect("serialising PVExpired")),
    )?;
    let _ = apply_object_updates(
        &state,
        &gym,
        pport.passport_id,
        revision_id,
        String::from(""), // TODO fine?
        [op].to_vec(),
    )
    .await?;

    // find session created in confirmation
    let parent_path = state.db.parent_path("gyms", gym)?;
    let sessions_stream: BoxStream<FirestoreResult<Session>> = state
        .db
        .fluent()
        .select()
        .from(Session::COLLECTION)
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
        Some(session) => Ok(session.id.clone().expect("session has id")),
        None => Err(AppError::NotAuthorized()),
    }?;

    // respond with the session cookie and status=200
    let cookie = Cookie::build(("session", session_id))
        .path("/")
        .max_age(Duration::weeks(52))
        .secure(true)
        .same_site(SameSite::None)
        .http_only(true);
    Ok(jar.add(cookie))
}
