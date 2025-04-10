use std::collections::HashMap;

use axum::{
    extract::{Path, Query, State},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use otp::{types::ObjectId, ROOT_PATH};
use sendgrid::v3::*;
use serde::{Deserialize, Serialize};

use crate::{
    storage::{apply_object_updates, lookup_latest_snapshot},
    word_list::make_security_code,
    AppError, AppState,
};

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

fn generate_email(
    pc_realm: String,
    api_domain: String,
    passport_id: String,
    security_code: String,
    confirmation_token: String,
) {
    let confirmation_url = format!(
        "{api_domain}/login/confirm?passportId={passport_id}&confirmationToken={confirmation_token}",
    );
    let subject = format!("{pc_realm} Login Verification (code: \"{security_code}\")",);
    let body = "Verify your email to log on to the {pc_realm}\n\
        We have received a login attempt with the following code: \n{security_code}\n\
        complete the login process, please click the URL below: \n{confirmation_url}\n\
        copy and paste this URL into your browser.";
}

fn send_email(subject: String) {
    let mut cool_header = HashMap::with_capacity(2);
    cool_header.insert(String::from("text/plain"), String::from("indeed"));

    let p = Personalization::new(Email::new("test@example.com")).add_headers(cool_header);

    let m = Message::new(Email::new("g@gmail.com"))
        .set_subject(&subject)
        .add_content(
            Content::new()
                .set_content_type("text/html")
                .set_value("Test"),
        )
        .add_personalization(p);

    let api_key = ::std::env::var("SG_API_KEY").unwrap();
    let sender = Sender::new(api_key, None);
    let code = sender.blocking_send(&m);
    println!("{:?}", code);
}

async fn create_passport(
    State(state): State<AppState>,
    Path(gym): Path<String>,
    Json(payload): axum::extract::Json<CreatePassportBody>,
) -> Result<Json<CreatePassportResponse>, AppError> {
    return Err(AppError::NotImplemented());

    // 1. Lookup account by email. If no such account exists, create a new one

    // 2. Create a new Passport object.
    let security_code = make_security_code();
    let confirmation_token = new_id(16);

    // passportId <- reqAvers2 aversH $ do
    //     Avers.createObject passportObjectType rootObjId $ Passport
    //         { passportAccountId = accId
    //         , passportSecurityCode = securityCode
    //         , passportConfirmationToken = confirmationToken
    //         , passportValidity = PVUnconfirmed
    //         }

    // 3. Send email
    send();

    // let partToContent :: Part -> Value
    //     partToContent part = object
    //         [ "type" .= ("text/plain" :: Text) -- partType part
    //         , "value" .= LT.decodeUtf8 (partContent part)
    //         ]
    //
    // let subject = fromMaybe "???" $ lookup "Subject" (mailHeaders mail)
    //
    // let toPersonalization addr = object
    //         [ "to" .= [ object [ "email" .= addressEmail addr ] ]
    //         , "subject" .= subject
    //         ]
    //
    // print $ mailParts mail
    // let body = object
    //         [ "personalizations" .= map toPersonalization (mailTo mail)
    //         , "from" .= object
    //             [ "email" .= addressEmail (mailFrom mail)
    //             ]
    //         , "content" .= concatMap (map partToContent) (mailParts mail)
    //         ]
    //
    // let request = setRequestBodyJSON body
    //         $ setRequestHeader "Content-Type" ["application/json"]
    //         $ setRequestHeader "Authorization" ["Bearer " <> T.encodeUtf8 apiKey]
    //         $ "POST https://api.sendgrid.com/v3/mail/send"
    //
    // response <- httpLBS request
    // print response

    // 4. Send response
}

async fn confirm_passport(
    State(state): State<AppState>,
    Path(gym): Path<String>,
    Query(p): Query<ConfirmPassport>,
) -> Result<(), AppError> {
    Err(AppError::NotImplemented())

    //     -- Query params in Servant are always optional (Maybe), but we require them here.
    // passportId <- case mbPassportId of
    //     Nothing -> throwError err400 { errBody = "passportId missing" }
    //     Just pId -> pure $ ObjId pId
    //
    // confirmationToken <- case mbConfirmationToken of
    //     Nothing -> throwError err400 { errBody = "confirmationToken missing" }
    //     Just x -> pure x
    //
    // -- Lookup the latest snapshot of the Passport object.
    // (Snapshot{..}, Passport{..}) <- reqAvers2 aversH $ do
    //     snapshot <- lookupLatestSnapshot (BaseObjectId passportId)
    //     passport <- case parseValueAs passportObjectType (snapshotContent snapshot) of
    //         Left e  -> throwError e
    //         Right x -> pure x
    //
    //     pure (snapshot, passport)
    //
    // -- Check the confirmationToken. Fail if it doesn't match.
    // when (confirmationToken /= passportConfirmationToken) $ do
    //     throwError err400 { errBody = "wrong confirmation token" }
    //
    // -- Patch the "validity" field to mark the Passport as valid.
    // void $ reqAvers2 aversH $ applyObjectUpdates
    //     (BaseObjectId passportId)
    //     snapshotRevisionId
    //     rootObjId
    //     [Set { opPath = "validity", opValue = Just (toJSON PVValid) }]
    //     False
    //
    // -- Apparently this is how you do a 30x redirect in Servant…
    // throwError $ err301
    //     { errHeaders = [("Location", T.encodeUtf8 (_pcAppDomain pc) <> "/email-confirmed")]
    //     }
}

async fn await_passport_confirmation(
    State(state): State<AppState>,
    Path(gym): Path<String>,
    Query(pport): Query<AwaitPassportConfirmation>,
    // jar: CookieJar,
) -> Result<impl IntoResponse, AppError> {
    let snapshot = lookup_latest_snapshot(&state, &gym, &pport.passport_id.clone()).await?;
    let passport: Passport = serde_json::from(snapshot.content);

    loop {
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
    }

    apply_object_updates(
        &state,
        &gym,
        passport_id,
        revision_id,
        root_object_id,
        "[Set { opPath = validity, opValue = Just (toJSON PVExpired) }]",
        False,
    );

    // The Passport object is valid.
    // Create a new session for the account in the Passport object.
    let now = getCurrentTime();
    let sess_id = newId(80);
    save_session(session_id, account_id, now, now)?;
    // reqAvers2 aversH $ saveSession $ Session sessId accId now now

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
