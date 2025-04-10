use std::collections::HashMap;

use sendgrid::v3::*;

use crate::word_list::make_security_code;

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

fn create_passport(email: String) {
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
