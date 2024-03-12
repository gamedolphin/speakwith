use std::{
    convert::Infallible,
    sync::{atomic::Ordering, Arc},
};

use anyhow::Result;
use axum::{
    debug_handler,
    extract::{Multipart, Path, Query, State},
    response::{
        sse::{Event, KeepAlive},
        Html, IntoResponse, Sse,
    },
    routing::{get, post},
    Form, Router,
};
use axum_extra::extract::{
    cookie::{Cookie, SameSite},
    CookieJar,
};
use axum_htmx::HxRedirect;
use convert_case::{Case, Casing};
use database::{users::UserCombined, Database};
use futures::TryStreamExt;
use minijinja::context;
use rand::{distributions::Alphanumeric, thread_rng, Rng};
use time::{Duration, OffsetDateTime};
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt as _;
use users::LoginForm;

use crate::{FrontendError, FrontendState};

pub fn setup_api(state: Arc<FrontendState>) -> Router {
    Router::new()
        .route("/register", post(handle_registration))
        .route("/login", post(handle_login))
        .route("/create-room", post(handle_create_room))
        .route("/reset-register-link", post(handle_reset_register_link))
        .route("/search-user", get(handle_search_users))
        .route("/create-user-room", post(handle_create_user_room))
        .route("/user/update/password", post(handle_update_user_password))
        .route("/user/update/profile", post(handle_update_user_profile))
        .route("/user/update/image", post(handle_update_user_image))
        .route("/user/update/image-none", post(handle_delete_user_image))
        .route("/users/:userid/enabled", post(handle_enable_user))
        .route("/users/:userid/admin", post(handle_user_admin))
        .route("/room/:roomid", get(handle_join_room))
        .route("/room/:roomid/upload", post(handle_upload_to_room))
        .route("/room/:roomid/send", post(handle_send_message))
        .route("/room/:roomid/more", get(handle_pagination))
        .route("/room/:roomid/add/:userid", post(handle_add_user_to_room))
        .route(
            "/room/:roomid/remove/:userid",
            post(handle_remove_user_from_room),
        )
        .with_state(state)
}

#[debug_handler]
async fn handle_reset_register_link(
    jar: CookieJar,
    State(state): State<Arc<FrontendState>>,
) -> Result<impl IntoResponse, FrontendError> {
    let Some(user) = extract_user(jar, &state.db, &state.secret).await else {
        return Err(FrontendError::Unauthorized);
    };

    if !user.is_admin {
        return Err(FrontendError::NoPermission);
    }

    let mut regid = state.register_id.write();
    *regid = get_random_alphanumeric();

    let regid = regid.to_string();

    let output = state.templates.render_template(
        "components/invite.jinja2",
        context! {
            register_id => regid,
        },
    )?;

    Ok(Html(output).into_response())
}

#[derive(serde::Deserialize)]
struct Allow {
    value: bool,
}

#[debug_handler]
async fn handle_user_admin(
    jar: CookieJar,
    Path(userid): Path<String>,
    allow: Query<Allow>,
    State(state): State<Arc<FrontendState>>,
) -> Result<impl IntoResponse, FrontendError> {
    let Some(user) = extract_user(jar, &state.db, &state.secret).await else {
        return Err(FrontendError::Unauthorized);
    };

    if !user.is_admin {
        return Err(FrontendError::NoPermission);
    }

    if user.id == userid {
        // not allowed to enable/disable self
        return Err(FrontendError::NoPermission);
    }

    database::users::make_admin_user(&state.db, &userid, allow.value)
        .await
        .map_err(FrontendError::InternalError)?;

    let template = "components/user-buttons-admin.jinja2";

    let output = state.templates.render_template(
        template,
        context! {
            is_admin => allow.value,
            userid => userid,
        },
    )?;

    Ok(Html(output).into_response())
}

#[derive(serde::Deserialize, Debug)]
#[serde(untagged)]
enum UserList {
    Single(String),
    Many(Vec<String>),
}

#[derive(serde::Deserialize, Debug)]
struct UserRoomForm {
    user: UserList,
}

#[debug_handler]
async fn handle_create_user_room(
    jar: CookieJar,
    State(state): State<Arc<FrontendState>>,
    axum_extra::extract::Form(form): axum_extra::extract::Form<UserRoomForm>, // extra::form to read array values
) -> Result<impl IntoResponse, FrontendError> {
    let Some(user) = extract_user(jar, &state.db, &state.secret).await else {
        return Err(FrontendError::Unauthorized);
    };

    let mut users = match form.user {
        UserList::Single(item) => vec![item],
        UserList::Many(items) => items,
    };

    let userids = users
        .iter()
        .map(|u| format!(r#"'{}'"#, u))
        .collect::<Vec<String>>()
        .join(",");

    let usernames = database::users::get_usernames_by_id(&state.db, &userids)
        .await
        .map_err(FrontendError::InternalError)?
        .join(", ");

    // add self to it
    users.push(user.id);
    users.sort_unstable();
    users.dedup();

    if users.is_empty() {
        return Err(FrontendError::InvalidForm(
            "needs at least one user!".into(),
        ));
    }

    let room_id = users.join("-");

    let room_id =
        database::rooms::create_room(&state.db, &room_id, &usernames, "", true, true, &users)
            .await
            .map_err(FrontendError::InternalError)?;

    Ok((
        HxRedirect(format!("/chatroom/{}", room_id).parse().unwrap()),
        "",
    )
        .into_response())
}

#[debug_handler]
async fn handle_enable_user(
    jar: CookieJar,
    Path(userid): Path<String>,
    allow: Query<Allow>,
    State(state): State<Arc<FrontendState>>,
) -> Result<impl IntoResponse, FrontendError> {
    let Some(user) = extract_user(jar, &state.db, &state.secret).await else {
        return Err(FrontendError::Unauthorized);
    };

    if !user.is_admin {
        return Err(FrontendError::NoPermission);
    }

    if user.id == userid {
        // not allowed to enable/disable self
        return Err(FrontendError::NoPermission);
    }

    let otheruser = database::users::get_user_with_profile(&state.db, &userid)
        .await
        .map_err(FrontendError::InternalError)?;

    database::users::enable_user(&state.db, &userid, allow.value)
        .await
        .map_err(FrontendError::InternalError)?;

    let template = if allow.value {
        "components/user-buttons-enabled.jinja2"
    } else {
        "components/user-buttons-disabled.jinja2"
    };

    let output = state.templates.render_template(
        template,
        context! {
            is_admin => otheruser.is_admin,
            userid => userid,
        },
    )?;

    Ok(Html(output).into_response())
}

#[derive(serde::Deserialize)]
struct Pagination {
    page: i32,
}

#[debug_handler]
async fn handle_pagination(
    jar: CookieJar,
    Path(roomid): Path<String>,
    pagination: Query<Pagination>,
    State(state): State<Arc<FrontendState>>,
) -> Result<impl IntoResponse, FrontendError> {
    let Some(user) = extract_user(jar, &state.db, &state.secret).await else {
        return Err(FrontendError::Unauthorized);
    };

    let room = database::rooms::get_room(&state.db, &roomid, &user.id)
        .await
        .map_err(|e| FrontendError::NotFound(e.to_string()))?;

    let page = pagination.page;

    let messages = state
        .room_manager
        .get_room_messages(&roomid, page, &user.id)
        .await
        .map_err(FrontendError::InternalError)?;

    let next_page = if messages.len() < database::messages::MAX_FETCH as usize {
        0
    } else {
        page + 1
    };

    let output = state.templates.render_template(
        "components/message-list.jinja2",
        context! {
            roomid => roomid, currentRoom => room, messages => messages, page => next_page
        },
    )?;

    Ok(Html(output).into_response())
}

#[debug_handler]
async fn handle_join_room(
    jar: CookieJar,
    Path(roomid): Path<String>,
    State(state): State<Arc<FrontendState>>,
) -> Result<impl IntoResponse, FrontendError> {
    let Some(user) = extract_user(jar, &state.db, &state.secret).await else {
        return Err(FrontendError::Unauthorized);
    };

    tracing::info!("connected to {} as {}", roomid, user.username);

    let rcv = state
        .room_manager
        .join_room(roomid, &user.id)
        .await
        .map_err(FrontendError::InternalError)?;

    let ss = BroadcastStream::new(rcv)
        .filter_map(|c| c.ok())
        .map(move |c| {
            let rendered = state
                .templates
                .render_template("components/message.jinja2", context! { message => c })
                .unwrap();
            Event::default().event("IncomingMessage").data(rendered)
        })
        .map(Ok::<Event, Infallible>);

    Ok(Sse::new(ss)
        .keep_alive(KeepAlive::default())
        .into_response())
}

#[derive(serde::Deserialize, Default)]
pub struct MessageForm {
    pub msg: String,
    pub uploads: Option<Vec<String>>,
}

#[debug_handler]
async fn handle_send_message(
    jar: CookieJar,
    Path(roomid): Path<String>,
    State(state): State<Arc<FrontendState>>,
    axum_extra::extract::Form(form): axum_extra::extract::Form<MessageForm>,
) -> Result<impl IntoResponse, FrontendError> {
    let Some(user) = extract_user(jar, &state.db, &state.secret).await else {
        return Err(FrontendError::Unauthorized);
    };

    tracing::info!("uploads: {:?}", form.uploads);

    state
        .room_manager
        .send_message(
            &roomid,
            &user.id,
            &user.username,
            user.image,
            &form.msg,
            form.uploads.unwrap_or_default(),
        )
        .await
        .map_err(FrontendError::InternalError)?;

    // Ok(user_id)
    Ok("".into_response())
}

#[debug_handler]
async fn handle_add_user_to_room(
    jar: CookieJar,
    Path((roomid, userid)): Path<(String, String)>,
    State(state): State<Arc<FrontendState>>,
) -> Result<impl IntoResponse, FrontendError> {
    let Some(user) = extract_user(jar, &state.db, &state.secret).await else {
        return Err(FrontendError::Unauthorized);
    };

    if !user.is_admin && !database::rooms::is_member_of_room(&state.db, &roomid, &user.id).await {
        return Err(FrontendError::NoPermission);
    }

    database::rooms::add_user_to_room(&state.db, &roomid, &userid)
        .await
        .map_err(FrontendError::InternalError)?;

    let room_users = database::rooms::get_room_users(&state.db, &roomid)
        .await
        .map_err(FrontendError::InternalError)?;

    let output = state.templates.render_template(
        "components/room-user.jinja2",
        context! { roomUsers => room_users, roomid => roomid },
    )?;

    Ok(Html(output))
}

#[debug_handler]
async fn handle_remove_user_from_room(
    jar: CookieJar,
    Path((roomid, userid)): Path<(String, String)>,
    State(state): State<Arc<FrontendState>>,
) -> Result<impl IntoResponse, FrontendError> {
    let Some(user) = extract_user(jar, &state.db, &state.secret).await else {
        return Err(FrontendError::Unauthorized);
    };

    if !user.is_admin && !database::rooms::is_member_of_room(&state.db, &roomid, &user.id).await {
        return Err(FrontendError::NoPermission);
    }

    database::rooms::remove_user_from_room(&state.db, &roomid, &userid)
        .await
        .map_err(FrontendError::InternalError)?;

    let room_users = database::rooms::get_room_users(&state.db, &roomid)
        .await
        .map_err(FrontendError::InternalError)?;

    let output = state.templates.render_template(
        "components/room-user.jinja2",
        context! { roomUsers => room_users, roomid => roomid },
    )?;

    Ok(Html(output))
}

#[debug_handler]
async fn handle_login(
    mut jar: CookieJar,
    State(state): State<Arc<FrontendState>>,
    Form(form): Form<LoginForm>,
) -> Result<impl IntoResponse, FrontendError> {
    let user_id = database::users::verify_userpassword(&state.db, &form.email, &form.password)
        .await
        .map_err(|e| match e {
            database::users::DBUserErrors::UserNotEnabled => FrontendError::UserNotEnabled,
            database::users::DBUserErrors::InternalError(e) => FrontendError::InternalError(e),
            _ => FrontendError::InvalidCredentials,
        })?;
    let user_token =
        users::generate_token(&state.secret, &user_id).map_err(FrontendError::InternalError)?;

    jar = set_token(jar, user_token);

    // Ok(user_id)
    Ok((HxRedirect("/".parse().unwrap()), jar, "").into_response())
}

#[derive(serde::Deserialize)]
struct PasswordUpdate {
    pub current: String,
    pub update: String,
}

#[debug_handler]
async fn handle_update_user_password(
    jar: CookieJar,
    State(state): State<Arc<FrontendState>>,
    Form(update): Form<PasswordUpdate>,
) -> Result<impl IntoResponse, FrontendError> {
    let Some(user) = extract_user(jar, &state.db, &state.secret).await else {
        return Err(FrontendError::Unauthorized);
    };

    database::users::update_user_password(&state.db, &user.email, &update.current, &update.update)
        .await
        .map_err(FrontendError::InternalError)?;

    Ok("")
}

#[derive(serde::Deserialize)]
struct ProfileUpdate {
    pub name: String,
    pub bio: String,
}

#[debug_handler]
async fn handle_update_user_profile(
    jar: CookieJar,
    State(state): State<Arc<FrontendState>>,
    Form(update): Form<ProfileUpdate>,
) -> Result<impl IntoResponse, FrontendError> {
    let Some(user) = extract_user(jar, &state.db, &state.secret).await else {
        return Err(FrontendError::Unauthorized);
    };

    let user = database::users::update_user_profile(&state.db, &user.id, &update.name, &update.bio)
        .await
        .map_err(FrontendError::InternalError)?;

    let output = state.templates.render_template(
        "components/user-profile-edit.jinja2",
        context! { username => user.username, email => user.email, bio => user.bio },
    )?;

    Ok(Html(output))
}

#[debug_handler]
async fn handle_delete_user_image(
    jar: CookieJar,
    State(state): State<Arc<FrontendState>>,
) -> Result<impl IntoResponse, FrontendError> {
    let Some(user) = extract_user(jar, &state.db, &state.secret).await else {
        return Err(FrontendError::Unauthorized);
    };

    database::users::unset_user_image(&state.db, &user.id)
        .await
        .map_err(FrontendError::InternalError)?;

    let output = state.templates.render_template(
        "components/user-profile-image-edit.jinja2",
        context! { image => None::<String>, username => user.username, oob => true },
    )?;

    Ok(Html(output))
}

#[debug_handler]
async fn handle_upload_to_room(
    jar: CookieJar,
    Path(roomid): Path<String>,
    State(state): State<Arc<FrontendState>>,
    mut multipart: Multipart,
) -> Result<impl IntoResponse, FrontendError> {
    let Some(user) = extract_user(jar, &state.db, &state.secret).await else {
        return Err(FrontendError::Unauthorized);
    };

    let Ok(Some(field)) = multipart.next_field().await else {
        return Err(FrontendError::InvalidForm(
            "missing field name: {:?}".into(),
        ));
    };

    let name = field
        .name()
        .ok_or_else(|| FrontendError::InvalidForm(format!("missing field name: {:?}", field)))?;

    if name != "file" {
        return Err(FrontendError::InvalidForm(format!(
            "missing field name: {:?}",
            field
        )));
    }

    let file_name = field
        .file_name()
        .ok_or_else(|| {
            FrontendError::InvalidForm(format!("missing file name for file: {:?}", field))
        })?
        .to_string();

    let body_with_err = field.map_err(|err| std::io::Error::new(std::io::ErrorKind::Other, err));

    let (file_url, file_type) = uploads::upload_file(
        body_with_err,
        &state.uploads_path,
        None, // unique id per file upload
        &file_name,
    )
    .await
    .map_err(FrontendError::InternalError)?;

    let (upload_id, trx) = database::uploads::add_upload_and_continue(
        &state.db,
        &user.id,
        Some(roomid),
        Some(file_name.clone()),
        Some(file_url.clone()),
    )
    .await
    .map_err(FrontendError::InternalError)?;

    trx.commit()
        .await
        .map_err(|e| FrontendError::InternalError(e.into()))?;

    println!("file_type: {}", file_type);

    let output = state.templates.render_template(
        "components/uploaded-file.jinja2",
        context! { path => file_url, file_type => file_type, upload_id => upload_id },
    )?;

    Ok(Html(output))
}

#[debug_handler]
async fn handle_update_user_image(
    jar: CookieJar,
    State(state): State<Arc<FrontendState>>,
    mut multipart: Multipart,
) -> Result<impl IntoResponse, FrontendError> {
    let Some(user) = extract_user(jar, &state.db, &state.secret).await else {
        return Err(FrontendError::Unauthorized);
    };

    let Ok(Some(field)) = multipart.next_field().await else {
        return Err(FrontendError::InvalidForm(
            "missing field name: {:?}".into(),
        ));
    };

    let name = field
        .name()
        .ok_or_else(|| FrontendError::InvalidForm(format!("missing field name: {:?}", field)))?;

    if name != "image" {
        return Err(FrontendError::InvalidForm(format!(
            "missing field name: {:?}",
            field
        )));
    }

    let file_name = field
        .file_name()
        .ok_or_else(|| {
            FrontendError::InvalidForm(format!("missing file name for file: {:?}", field))
        })?
        .to_string();

    let body_with_err = field.map_err(|err| std::io::Error::new(std::io::ErrorKind::Other, err));

    let (file_url, _) = uploads::upload_file(
        body_with_err,
        &state.uploads_path,
        Some(user.id.clone()),
        &file_name,
    )
    .await
    .map_err(FrontendError::InternalError)?;

    let (_, mut trx) = database::uploads::add_upload_and_continue(
        &state.db,
        &user.id,
        None,
        Some(file_name.clone()),
        Some(file_url.clone()),
    )
    .await
    .map_err(FrontendError::InternalError)?;

    database::users::set_user_image(&mut trx, &user.id, Some(file_url.as_ref()))
        .await
        .map_err(FrontendError::InternalError)?;

    let output = state.templates.render_template(
        "components/user-profile-image-edit.jinja2",
        context! { image => file_url, username => user.username, oob => true },
    )?;

    trx.commit()
        .await
        .map_err(|e| FrontendError::InternalError(e.into()))?;

    Ok(Html(output))
}

#[derive(Default, Debug)]
struct RegistrationForm {
    pub name: String,
    pub email: String,
    pub password: String,
    pub filename: String,
    pub image: Option<String>,
}

#[debug_handler]
async fn handle_registration(
    mut jar: CookieJar,
    State(state): State<Arc<FrontendState>>,
    mut multipart: Multipart,
) -> Result<impl IntoResponse, FrontendError> {
    let user_id = xid::new().to_string();

    let mut form = RegistrationForm::default();

    while let Ok(Some(field)) = multipart.next_field().await {
        let name = field.name().ok_or_else(|| {
            FrontendError::InvalidForm(format!("missing field name: {:?}", field))
        })?;

        match name {
            "image" => {
                let file_name = field
                    .file_name()
                    .ok_or_else(|| {
                        FrontendError::InvalidForm(format!(
                            "missing file name for file: {:?}",
                            field
                        ))
                    })?
                    .to_string();

                let body_with_err =
                    field.map_err(|err| std::io::Error::new(std::io::ErrorKind::Other, err));

                let (path, _) = uploads::upload_file(
                    body_with_err,
                    &state.uploads_path,
                    Some(user_id.clone()),
                    &file_name,
                )
                .await
                .map_err(FrontendError::InternalError)?;

                form.image = Some(path);
                form.filename = file_name;
            }
            "name" => {
                form.name = field.text().await.map_err(|e| {
                    FrontendError::InvalidForm(format!("failed to read name: {}", e))
                })?;
            }
            "email" => {
                form.email = field.text().await.map_err(|e| {
                    FrontendError::InvalidForm(format!("failed to read email: {}", e))
                })?;
            }
            "password" => {
                form.password = field.text().await.map_err(|e| {
                    FrontendError::InvalidForm(format!("failed to read email: {}", e))
                })?;
            }
            _ => {}
        }
    }

    if form.name.is_empty() {
        return Err(FrontendError::InvalidForm("missing name field".to_string()));
    }

    if form.email.is_empty() {
        return Err(FrontendError::InvalidForm(
            "missing email field".to_string(),
        ));
    }

    if form.password.is_empty() {
        return Err(FrontendError::InvalidForm(
            "missing password field".to_string(),
        ));
    }

    let (_, trx) = database::uploads::add_upload_and_continue(
        &state.db,
        &user_id,
        None,
        Some(form.filename.clone()),
        form.image.clone(),
    )
    .await
    .map_err(FrontendError::InternalError)?;

    let has_admin = state.has_admin.load(Ordering::Relaxed);

    let user = database::users::User {
        email: form.email,
        password: form.password,
        is_admin: !has_admin,
        is_enabled: !has_admin,
        ..Default::default()
    };

    let profile = database::users::UserProfile {
        username: form.name,
        image: form.image,
        ..Default::default()
    };

    let (user_id, trx) = database::users::create_user(&user_id, trx, user, profile)
        .await
        .map_err(FrontendError::InternalError)?;

    let user_token =
        users::generate_token(&user_id, &state.secret).map_err(FrontendError::InternalError)?;

    jar = set_token(jar, user_token);

    if !has_admin {
        let mut register_id = state.register_id.write();
        *register_id = get_random_alphanumeric(); // just created a new admin, set registration_id
        state.has_admin.store(true, Ordering::Relaxed);
    }

    trx.commit()
        .await
        .map_err(|e| FrontendError::InternalError(e.into()))?;

    // Ok(user_id)
    Ok((HxRedirect("/".parse().unwrap()), jar, "").into_response())
}

pub fn get_random_alphanumeric() -> String {
    thread_rng()
        .sample_iter(&Alphanumeric)
        .take(10)
        .map(char::from)
        .collect()
}

#[derive(serde::Deserialize, Default, Debug)]
pub struct SearchUser {
    name: String,
    local: bool,
    roomid: Option<String>,
}

#[debug_handler]
async fn handle_search_users(
    jar: CookieJar,
    State(state): State<Arc<FrontendState>>,
    search: Query<SearchUser>,
) -> Result<impl IntoResponse, FrontendError> {
    let Some(user) = extract_user(jar, &state.db, &state.secret).await else {
        return Err(FrontendError::Unauthorized);
    };

    let users = database::users::search_users(&state.db, &search.name, &user.id)
        .await
        .map_err(FrontendError::InternalError)?;

    println!("searched :{:?}", search);

    let output = state.templates.render_template(
        "components/user-search-results.jinja2",
        context! { results => users, local => search.local, roomid => search.roomid },
    )?;

    Ok(Html(output))
}

#[derive(serde::Deserialize)]
pub struct NewRoom {
    name: String,
    description: String,
    is_private: Option<bool>,
}

#[debug_handler]
async fn handle_create_room(
    jar: CookieJar,
    State(state): State<Arc<FrontendState>>,
    Form(form): Form<NewRoom>,
) -> Result<impl IntoResponse, FrontendError> {
    let Some(user) = extract_user(jar, &state.db, &state.secret).await else {
        return Err(FrontendError::Unauthorized);
    };

    let room_id = form.name.to_case(Case::Kebab);

    let room_id = database::rooms::create_room(
        &state.db,
        &room_id,
        &form.name,
        &form.description,
        form.is_private.unwrap_or_default(),
        false,
        &[user.id],
    )
    .await
    .map_err(FrontendError::InternalError)?;

    // Ok(user_id)
    Ok((
        HxRedirect(format!("/chatroom/{}", room_id).parse().unwrap()),
        "",
    )
        .into_response())
}

fn set_token(jar: CookieJar, user_token: String) -> CookieJar {
    let mut cookie = Cookie::new("token", user_token);
    cookie.set_same_site(SameSite::Strict);
    cookie.set_path("/");
    cookie.set_expires(OffsetDateTime::now_utc() + Duration::weeks(52));
    jar.add(cookie)
}

pub(crate) fn validate_token(jar: CookieJar, secret: &str) -> Option<String> {
    let token = jar.get("token")?.value();

    let user_id = users::extract_sub(secret, token).ok()??;

    Some(user_id)
}

pub(crate) async fn extract_user(
    jar: CookieJar,
    db: &Database,
    secret: &str,
) -> Option<UserCombined> {
    let user_id = validate_token(jar, secret)?;

    let user = database::users::get_user_with_profile(db, &user_id)
        .await
        .ok()?;

    Some(user)
}
