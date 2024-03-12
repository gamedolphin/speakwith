use std::sync::{atomic::AtomicBool, Arc};

use anyhow::Result;
use api::{extract_user, get_random_alphanumeric, setup_api, validate_token};
use assets::setup_asset_handler;
use axum::{
    debug_handler,
    extract::{Path, Request, State},
    http::StatusCode,
    response::{Html, IntoResponse, Redirect},
    Router,
};
use axum_extra::extract::CookieJar;
use axum_htmx::HxRequest;
use database::{rooms::RoomUser, Database};
use minijinja::context;
use parking_lot::RwLock;
use templates::Templates;
use thiserror::Error;
use tower::ServiceExt;
use tower_http::services::ServeDir;

mod api;
mod assets;
mod templates;

#[derive(Error, Debug)]
pub enum FrontendError {
    #[error("internal server error : {0}")]
    InternalError(anyhow::Error),
    #[error("not found : {0}")]
    NotFound(String),
    #[error("unauthorized")]
    Unauthorized,
    #[error("no permission")]
    NoPermission,
    #[error("no permission")]
    UserNotEnabled,
    #[error("needs_admin")]
    NeedsAdmin,
    #[error("already logged in")]
    AlreadyLoggedIn,
    #[error("invalid credentials")]
    InvalidCredentials,
    #[error("invalid file")]
    InvalidForm(String),
}

impl IntoResponse for FrontendError {
    fn into_response(self) -> axum::response::Response {
        match self {
            FrontendError::Unauthorized => Redirect::temporary("/login").into_response(),
            FrontendError::NeedsAdmin => Redirect::temporary("/register/admin").into_response(),
            FrontendError::AlreadyLoggedIn => Redirect::temporary("/").into_response(),
            FrontendError::InvalidForm(e) => (StatusCode::BAD_REQUEST, e).into_response(),
            FrontendError::NoPermission => (StatusCode::UNAUTHORIZED).into_response(),
            FrontendError::InternalError(e) => {
                (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response()
            }
            _ => (StatusCode::INTERNAL_SERVER_ERROR, "").into_response(),
        }
    }
}

#[derive(Clone)]
struct FrontendState {
    has_admin: Arc<AtomicBool>,
    templates: Templates,
    room_manager: Arc<rooms::Manager>,
    db: Database,
    secret: String,
    uploads_path: String,
    register_id: Arc<RwLock<String>>,
}

pub async fn initialize(
    _base_route: &str,
    secret: &str,
    data_path: String,
    db: Database,
) -> Result<Router> {
    let has_admin = database::users::has_admin(&db).await?;
    let register_id = if has_admin {
        get_random_alphanumeric()
    } else {
        String::from("admin")
    };

    let templates = Templates::default();

    let state = Arc::new(FrontendState {
        has_admin: Arc::new(AtomicBool::new(has_admin)),
        templates,
        secret: secret.to_string(),
        room_manager: Arc::new(rooms::Manager::new(db.clone())),
        register_id: Arc::new(RwLock::new(register_id)),
        uploads_path: format!("{}/uploads", &data_path),
        db,
    });

    std::fs::create_dir_all(&state.uploads_path)?;

    let router = Router::new()
        .route("/", axum::routing::get(home_handler))
        .route("/login", axum::routing::get(login_handler))
        .route("/register/:id", axum::routing::get(register_handler))
        .route("/users", axum::routing::get(user_handler))
        .route("/profile", axum::routing::get(profile_handler))
        .route("/chatroom/:roomid", axum::routing::get(room_handler))
        .route("/template/*path", axum::routing::get(template_handler))
        .with_state(state.clone())
        .nest_service(
            "/uploads",
            axum::routing::get(uploads_handler).with_state(state.clone()),
        )
        .nest("/htmx", setup_api(state))
        .nest("/assets", setup_asset_handler());

    Ok(router)
}

#[debug_handler]
async fn uploads_handler(
    jar: CookieJar,
    State(state): State<Arc<FrontendState>>,
    req: Request,
) -> Result<impl IntoResponse, FrontendError> {
    let Some(_) = validate_token(jar, &state.secret) else {
        return Err(FrontendError::Unauthorized);
    };

    let service = ServeDir::new(&state.uploads_path);
    let result = service
        .oneshot(req)
        .await
        .map_err(|e| FrontendError::InternalError(e.into()));

    Ok(result)
}

#[debug_handler]
async fn template_handler(
    Path(path): Path<String>,
    State(state): State<Arc<FrontendState>>,
) -> Result<impl IntoResponse, FrontendError> {
    let templates = &state.templates;

    let output = templates.render_template(&path, context! {})?;

    Ok(Html(output))
}

#[debug_handler]
async fn profile_handler(
    jar: CookieJar,
    HxRequest(is_htmx): HxRequest,
    State(state): State<Arc<FrontendState>>,
) -> Result<impl IntoResponse, FrontendError> {
    let user = redirect_to_register(jar, &state).await?;

    let output = if is_htmx {
        state
            .templates
            .render_template("components/profile.jinja2", context! { user => user })?
    } else {
        let (user_rooms, rooms) = database::rooms::get_rooms(&state.db, &user.id)
            .await
            .map_err(FrontendError::InternalError)?;

        state.templates.render_template(
            "profile.jinja2",
            context! { rooms => rooms, user_rooms => user_rooms , user => user },
        )?
    };

    Ok(Html(output).into_response())
}

#[debug_handler]
async fn home_handler(
    jar: CookieJar,
    State(state): State<Arc<FrontendState>>,
) -> Result<impl IntoResponse, FrontendError> {
    let user = redirect_to_register(jar, &state).await?;

    let (user_rooms, rooms) = database::rooms::get_rooms(&state.db, &user.id)
        .await
        .map_err(FrontendError::InternalError)?;

    let templates = &state.templates;

    let output = templates.render_template(
        "home.jinja2",
        context! { rooms => rooms, user_rooms => user_rooms, user => user },
    )?;

    Ok(Html(output).into_response())
}

#[debug_handler]
async fn room_handler(
    HxRequest(is_htmx): HxRequest,
    jar: CookieJar,
    Path(roomid): Path<String>,
    State(state): State<Arc<FrontendState>>,
) -> Result<impl IntoResponse, FrontendError> {
    let user = redirect_to_register(jar, &state).await?;

    let room = database::rooms::get_room(&state.db, &roomid, &user.id)
        .await
        .map_err(|e| FrontendError::NotFound(e.to_string()))?;

    let messages = state
        .room_manager
        .get_room_messages(&roomid, 0, &user.id)
        .await
        .map_err(FrontendError::InternalError)?;

    dbg!(&messages);

    let page = if messages.len() < database::messages::MAX_FETCH as usize {
        0
    } else {
        1
    };

    let room_users: Vec<RoomUser> = if room.is_private && !room.is_user {
        database::rooms::get_room_users(&state.db, &roomid)
            .await
            .map_err(FrontendError::InternalError)?
    } else {
        vec![]
    };

    let output = if is_htmx {
        state.templates.render_template(
            "components/chatroom.jinja2",
            context! { roomid => roomid, currentRoom => room, messages => messages, page => page, user => user, roomUsers => room_users },
        )?
    } else {
        let (user_rooms, rooms) = database::rooms::get_rooms(&state.db, &user.id)
            .await
            .map_err(FrontendError::InternalError)?;

        state.templates.render_template(
            "room.jinja2",
            context! { rooms => rooms, roomid => roomid, currentRoom => room, user_rooms => user_rooms , messages => messages, page => page, user => user, roomUsers => room_users },
        )?
    };

    Ok(Html(output).into_response())
}

async fn redirect_to_home(jar: CookieJar, state: &Arc<FrontendState>) -> Result<(), FrontendError> {
    let Some(_) = extract_user(jar, &state.db, &state.secret).await else {
        return Ok(());
    };

    Err(FrontendError::AlreadyLoggedIn)
}

async fn redirect_to_register(
    jar: CookieJar,
    state: &Arc<FrontendState>,
) -> Result<database::users::UserCombined, FrontendError> {
    let Some(user) = extract_user(jar, &state.db, &state.secret).await else {
        if state.has_admin.load(std::sync::atomic::Ordering::Relaxed) {
            return Err(FrontendError::Unauthorized);
        } else {
            return Err(FrontendError::NeedsAdmin);
        }
    };

    Ok(user)
}

#[debug_handler]
async fn register_handler(
    jar: CookieJar,
    Path(id): Path<String>,
    State(state): State<Arc<FrontendState>>,
) -> Result<impl IntoResponse, FrontendError> {
    redirect_to_home(jar, &state).await?;

    if id != *state.register_id.read() {
        return Err(FrontendError::NotFound(
            "invalid registration link".to_string(),
        ));
    }

    let templates = &state.templates;

    let output = templates.render_template("register.jinja2", context! {})?;

    Ok(Html(output))
}

#[debug_handler]
async fn user_handler(
    jar: CookieJar,
    HxRequest(is_htmx): HxRequest,
    State(state): State<Arc<FrontendState>>,
) -> Result<impl IntoResponse, FrontendError> {
    let user = redirect_to_register(jar, &state).await?;

    if !user.is_admin {
        return Err(FrontendError::NoPermission);
    }

    let user_list = database::users::get_user_list(&state.db)
        .await
        .map_err(FrontendError::InternalError)?;

    let output = if is_htmx {
        let register_id = state.register_id.read();
        let register_id = register_id.as_str().to_string();

        state.templates.render_template(
            "components/users.jinja2",
            context! { register_id => register_id, user => user, userlist => user_list },
        )?
    } else {
        let (user_rooms, rooms) = database::rooms::get_rooms(&state.db, &user.id)
            .await
            .map_err(FrontendError::InternalError)?;

        let register_id = state.register_id.read();
        let register_id = register_id.as_str().to_string();

        state.templates.render_template(
            "users.jinja2",
            context! { rooms => rooms, user_rooms => user_rooms, register_id => register_id, user => user, userlist => user_list },
        )?
    };

    Ok(Html(output))
}

#[debug_handler]
async fn login_handler(
    jar: CookieJar,
    State(state): State<Arc<FrontendState>>,
) -> Result<impl IntoResponse, FrontendError> {
    redirect_to_home(jar, &state).await?;

    let templates = &state.templates;

    let output = templates.render_template("login.jinja2", context! {})?;

    Ok(Html(output))
}
