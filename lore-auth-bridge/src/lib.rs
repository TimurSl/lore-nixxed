// SPDX-FileCopyrightText: 2026 Epic Games, Inc.
// SPDX-License-Identifier: MIT
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use axum::Json;
use axum::Router;
use axum::extract::Query;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::get;
use config::Config;
use jsonwebtoken::Algorithm;
use jsonwebtoken::DecodingKey;
use jsonwebtoken::EncodingKey;
use jsonwebtoken::Validation;
use serde::Deserialize;
use serde::Serialize;
use thiserror::Error;
use tokio::sync::Mutex;
use tonic::Request;
use tonic::Response;
use tonic::Status;
use uuid::Uuid;

pub mod proto {
    #[rustfmt::skip]
    pub mod auth {
        include!(concat!(env!("OUT_DIR"), "/epic_urc.rs"));
    }

    #[rustfmt::skip]
    pub mod rebac {
        include!(concat!(env!("OUT_DIR"), "/ucs.auth.rs"));
    }
}

#[derive(Debug, Error)]
pub enum BridgeError {
    #[error("auth session not found")]
    SessionNotFound,
    #[error("client_state does not match auth session")]
    ClientStateMismatch,
    #[error("jwt error: {0}")]
    Jwt(#[from] jsonwebtoken::errors::Error),
    #[error("invalid jwks json")]
    InvalidJwks,
    #[error("configuration error: {0}")]
    Config(#[from] config::ConfigError),
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("url error: {0}")]
    Url(#[from] url::ParseError),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FlowMode {
    Device,
    Callback,
    Both,
}

impl Default for FlowMode {
    fn default() -> Self {
        Self::Device
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ResourceMode {
    Wildcard,
    Requested,
}

impl Default for ResourceMode {
    fn default() -> Self {
        Self::Wildcard
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct AuthentikSettings {
    pub issuer: String,
    pub client_id: String,
    pub client_secret: Option<String>,
    #[serde(default = "default_scopes")]
    pub scopes: Vec<String>,
    #[serde(default)]
    pub flow: FlowMode,
}

#[derive(Clone, Debug, Deserialize)]
pub struct JwtSettings {
    pub issuer: String,
    pub audience: Vec<String>,
    pub private_key_pem: Option<String>,
    pub private_key_pem_file: Option<PathBuf>,
    pub public_jwks_json: Option<String>,
    pub public_jwks_json_file: Option<PathBuf>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct PolicySettings {
    #[serde(default)]
    pub default_resource_mode: ResourceMode,
    pub registry_path: Option<PathBuf>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct BridgeConfig {
    #[serde(default = "default_bind")]
    pub bind: SocketAddr,
    pub public_base_url: String,
    pub authentik: AuthentikSettings,
    pub jwt: JwtSettings,
    #[serde(default)]
    pub policy: PolicySettings,
}

impl BridgeConfig {
    pub fn load(path: Option<&str>) -> Result<Self, BridgeError> {
        let mut builder = Config::builder();
        if let Some(path) = path {
            builder = builder.add_source(config::File::with_name(path).required(true));
        }
        Ok(builder
            .add_source(
                config::Environment::with_prefix("LORE_AUTH_BRIDGE")
                    .separator("__")
                    .try_parsing(true)
                    .list_separator(",")
                    .with_list_parse_key("authentik.scopes")
                    .with_list_parse_key("jwt.audience"),
            )
            .build()?
            .try_deserialize()?)
    }
}

fn default_bind() -> SocketAddr {
    "127.0.0.1:4180".parse().expect("valid default bind")
}

fn default_scopes() -> Vec<String> {
    vec!["openid".into(), "profile".into(), "email".into()]
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UserProfile {
    pub user_id: String,
    pub name: String,
    pub preferred_username: String,
}

#[derive(Clone, Debug)]
pub struct DeviceSession {
    pub client_state: String,
    pub device_code: String,
    pub expires_at: i64,
    pub interval_seconds: u64,
    pub user: Option<UserProfile>,
    pub flow: SessionFlow,
    pub pkce_verifier: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SessionFlow {
    Device,
    Callback,
}

#[derive(Default)]
pub struct SessionStore {
    sessions: HashMap<String, DeviceSession>,
}

impl SessionStore {
    pub fn insert_device_session(&mut self, session: DeviceSession) -> String {
        let session_code = Uuid::new_v4().to_string();
        self.sessions.insert(session_code.clone(), session);
        session_code
    }

    pub fn get_device_session(
        &self,
        session_code: &str,
        client_state: &str,
    ) -> Result<&DeviceSession, BridgeError> {
        let session = self
            .sessions
            .get(session_code)
            .ok_or(BridgeError::SessionNotFound)?;
        if session.client_state != client_state {
            return Err(BridgeError::ClientStateMismatch);
        }
        Ok(session)
    }

    pub fn get_device_session_mut(
        &mut self,
        session_code: &str,
        client_state: &str,
    ) -> Result<&mut DeviceSession, BridgeError> {
        let session = self
            .sessions
            .get_mut(session_code)
            .ok_or(BridgeError::SessionNotFound)?;
        if session.client_state != client_state {
            return Err(BridgeError::ClientStateMismatch);
        }
        Ok(session)
    }

    pub fn complete_callback(
        &mut self,
        session_code: &str,
        user: UserProfile,
    ) -> Result<(), BridgeError> {
        let session = self
            .sessions
            .get_mut(session_code)
            .ok_or(BridgeError::SessionNotFound)?;
        session.user = Some(user);
        Ok(())
    }
}

pub enum DevicePoll {
    Pending,
    Complete(UserProfile),
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct LoreClaims {
    #[serde(rename = "sub")]
    pub user_id: String,
    #[serde(rename = "iss")]
    pub issuer: String,
    #[serde(rename = "iat")]
    pub issued_at: u64,
    #[serde(rename = "exp")]
    pub expires: u64,
    #[serde(rename = "aud")]
    pub audience: Vec<String>,
    pub env: String,
    pub name: String,
    pub preferred_username: String,
    pub is_service_account: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resources: Option<Vec<ResourcePermission>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub groups: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub idp: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ResourcePermission {
    pub resource_id: String,
    pub permission: Vec<String>,
}

pub struct JwtIssuer {
    issuer: String,
    audience: Vec<String>,
    key_id: String,
    key: EncodingKey,
    decoding_key: DecodingKey,
}

impl JwtIssuer {
    pub fn from_pem(
        issuer: impl Into<String>,
        audience: Vec<String>,
        private_key_pem: &str,
        public_jwks_json: &str,
    ) -> Result<Self, BridgeError> {
        let key_id = jwks_key_id(public_jwks_json)?;
        Ok(Self {
            issuer: issuer.into(),
            audience,
            key_id,
            key: EncodingKey::from_rsa_pem(private_key_pem.as_bytes())?,
            decoding_key: jwks_decoding_key(public_jwks_json)?,
        })
    }

    pub fn mint_authn(&self, user: &UserProfile) -> Result<String, BridgeError> {
        let claims = LoreClaims {
            user_id: user.user_id.clone(),
            issuer: self.issuer.clone(),
            issued_at: now_secs(),
            expires: now_secs() + 3600,
            audience: self.audience.clone(),
            env: "prod".to_string(),
            name: user.name.clone(),
            preferred_username: user.preferred_username.clone(),
            is_service_account: Some(false),
            resources: None,
            groups: None,
            idp: None,
        };
        self.encode(&claims)
    }

    pub fn mint_authz(
        &self,
        user: &UserProfile,
        resource_ids: &[String],
    ) -> Result<String, BridgeError> {
        let resources = resource_ids
            .iter()
            .map(|resource_id| ResourcePermission {
                resource_id: resource_id.clone(),
                permission: vec!["read".into(), "write".into(), "admin".into()],
            })
            .collect();
        let claims = LoreClaims {
            user_id: user.user_id.clone(),
            issuer: self.issuer.clone(),
            issued_at: now_secs(),
            expires: now_secs() + 900,
            audience: self.audience.clone(),
            env: "prod".to_string(),
            name: user.name.clone(),
            preferred_username: user.preferred_username.clone(),
            is_service_account: Some(false),
            resources: Some(resources),
            groups: Some(Vec::new()),
            idp: Some("authentik".to_string()),
        };
        self.encode(&claims)
    }

    pub fn verify(&self, token: &str) -> Result<LoreClaims, BridgeError> {
        let mut validation = Validation::new(Algorithm::RS256);
        validation.set_audience(&self.audience);
        validation.set_issuer(&[self.issuer.as_str()]);
        Ok(jsonwebtoken::decode::<LoreClaims>(token, &self.decoding_key, &validation)?.claims)
    }

    fn encode(&self, claims: &LoreClaims) -> Result<String, BridgeError> {
        let mut header = jsonwebtoken::Header::new(Algorithm::RS256);
        header.kid = Some(self.key_id.clone());
        Ok(jsonwebtoken::encode(&header, claims, &self.key)?)
    }
}

#[derive(Clone)]
pub struct AuthentikClient {
    issuer: url::Url,
    client_id: String,
    client_secret: Option<String>,
    scopes: Vec<String>,
    http: reqwest::Client,
}

impl AuthentikClient {
    pub fn new(settings: &AuthentikSettings) -> Result<Self, BridgeError> {
        Ok(Self {
            issuer: url::Url::parse(&settings.issuer)?,
            client_id: settings.client_id.clone(),
            client_secret: settings.client_secret.clone(),
            scopes: settings.scopes.clone(),
            http: reqwest::Client::new(),
        })
    }

    pub async fn start_device(&self) -> Result<DeviceAuthorization, BridgeError> {
        let mut form = vec![
            ("client_id", self.client_id.clone()),
            ("scope", self.scopes.join(" ")),
        ];
        if let Some(secret) = &self.client_secret {
            form.push(("client_secret", secret.clone()));
        }
        Ok(self
            .http
            .post(self.endpoint("application/o/device/")?)
            .form(&form)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?)
    }

    pub async fn poll_device(&self, device_code: &str) -> Result<DevicePollResult, BridgeError> {
        let mut form = vec![
            (
                "grant_type",
                "urn:ietf:params:oauth:grant-type:device_code".to_string(),
            ),
            ("client_id", self.client_id.clone()),
            ("device_code", device_code.to_string()),
        ];
        if let Some(secret) = &self.client_secret {
            form.push(("client_secret", secret.clone()));
        }

        let response = self
            .http
            .post(self.endpoint("application/o/token/")?)
            .form(&form)
            .send()
            .await?;
        if response.status().is_success() {
            let token: TokenResponse = response.json().await?;
            let access_token = token
                .access_token
                .or(token.id_token)
                .unwrap_or_default();
            let user = self.userinfo(&access_token).await?;
            return Ok(DevicePollResult::Complete(user));
        }

        let error: OAuthError = response.json().await?;
        if error.error == "authorization_pending" || error.error == "slow_down" {
            return Ok(DevicePollResult::Pending);
        }
        Ok(DevicePollResult::Rejected(error.error))
    }

    pub async fn exchange_code(
        &self,
        code: &str,
        redirect_uri: &str,
        verifier: Option<&str>,
    ) -> Result<UserProfile, BridgeError> {
        let mut form = vec![
            ("grant_type", "authorization_code".to_string()),
            ("client_id", self.client_id.clone()),
            ("code", code.to_string()),
            ("redirect_uri", redirect_uri.to_string()),
        ];
        if let Some(secret) = &self.client_secret {
            form.push(("client_secret", secret.clone()));
        }
        if let Some(verifier) = verifier {
            form.push(("code_verifier", verifier.to_string()));
        }
        let token: TokenResponse = self
            .http
            .post(self.endpoint("application/o/token/")?)
            .form(&form)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let access_token = token
            .access_token
            .or(token.id_token)
            .unwrap_or_default();
        self.userinfo(&access_token).await
    }

    pub fn authorize_url(
        &self,
        public_base_url: &str,
        state: &str,
        verifier: &str,
    ) -> Result<String, BridgeError> {
        let redirect_uri = callback_url(public_base_url);
        let challenge = pkce_challenge(verifier);
        let mut url = self.endpoint("application/o/authorize/")?;
        url.query_pairs_mut()
            .append_pair("response_type", "code")
            .append_pair("client_id", &self.client_id)
            .append_pair("redirect_uri", &redirect_uri)
            .append_pair("scope", &self.scopes.join(" "))
            .append_pair("state", state)
            .append_pair("code_challenge", &challenge)
            .append_pair("code_challenge_method", "S256");
        Ok(url.to_string())
    }

    async fn userinfo(&self, bearer: &str) -> Result<UserProfile, BridgeError> {
        let info: UserInfoResponse = self
            .http
            .get(self.endpoint("application/o/userinfo/")?)
            .bearer_auth(bearer)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        Ok(UserProfile {
            user_id: info.sub,
            name: info.name.unwrap_or_else(|| info.preferred_username.clone()),
            preferred_username: info.preferred_username,
        })
    }

    fn endpoint(&self, path: &str) -> Result<url::Url, BridgeError> {
        Ok(self.issuer.join(path)?)
    }
}

#[derive(Debug, Deserialize)]
pub struct DeviceAuthorization {
    pub device_code: String,
    pub verification_uri: Option<String>,
    pub verification_uri_complete: Option<String>,
    pub expires_in: Option<i64>,
    pub interval: Option<u64>,
}

#[derive(Debug)]
pub enum DevicePollResult {
    Pending,
    Complete(UserProfile),
    Rejected(String),
}

#[derive(Debug, Deserialize)]
struct OAuthError {
    error: String,
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: Option<String>,
    id_token: Option<String>,
}

#[derive(Debug, Deserialize)]
struct UserInfoResponse {
    sub: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    preferred_username: String,
}

#[derive(Default)]
pub struct UserCache {
    by_id: HashMap<String, UserProfile>,
    by_name: HashMap<String, String>,
}

impl UserCache {
    pub fn insert(&mut self, user: UserProfile) {
        self.by_name
            .insert(user.preferred_username.clone(), user.user_id.clone());
        self.by_name.insert(user.name.clone(), user.user_id.clone());
        self.by_id.insert(user.user_id.clone(), user);
    }

    pub fn user_info(&self, user_ids: &[String]) -> Vec<UserProfile> {
        user_ids
            .iter()
            .filter_map(|user_id| self.by_id.get(user_id).cloned())
            .collect()
    }

    pub fn user_id(&self, display_name: &str) -> Option<UserProfile> {
        self.by_name
            .get(display_name)
            .and_then(|user_id| self.by_id.get(user_id))
            .cloned()
    }
}

pub fn map_device_poll(poll: DevicePoll, issuer: &JwtIssuer) -> Result<Option<String>, BridgeError> {
    match poll {
        DevicePoll::Pending => Ok(None),
        DevicePoll::Complete(user) => issuer.mint_authn(&user).map(Some),
    }
}

pub fn lookup_user_permissions(
    resources: &[ResourcePermission],
    resource_filter: &str,
) -> Vec<ResourcePermission> {
    resources
        .iter()
        .filter(|resource| resource.resource_id.starts_with(resource_filter))
        .cloned()
        .collect()
}

#[derive(Clone)]
pub struct AppState {
    config: BridgeConfig,
    authentik: AuthentikClient,
    jwt: Arc<JwtIssuer>,
    sessions: Arc<Mutex<SessionStore>>,
    users: Arc<Mutex<UserCache>>,
    registry: Arc<Mutex<ResourceRegistry>>,
}

impl AppState {
    pub fn new(config: BridgeConfig) -> Result<Self, BridgeError> {
        let private_key_pem = read_inline_or_file(
            config.jwt.private_key_pem.as_deref(),
            config.jwt.private_key_pem_file.as_ref(),
        )?;
        let public_jwks_json = read_inline_or_file(
            config.jwt.public_jwks_json.as_deref(),
            config.jwt.public_jwks_json_file.as_ref(),
        )?;
        Ok(Self {
            authentik: AuthentikClient::new(&config.authentik)?,
            jwt: Arc::new(JwtIssuer::from_pem(
                config.jwt.issuer.clone(),
                config.jwt.audience.clone(),
                &private_key_pem,
                &public_jwks_json,
            )?),
            sessions: Arc::new(Mutex::new(SessionStore::default())),
            users: Arc::new(Mutex::new(UserCache::default())),
            registry: Arc::new(Mutex::new(ResourceRegistry::new(
                config.policy.registry_path.clone(),
            ))),
            config,
        })
    }

    pub async fn serve(self) -> Result<(), BridgeError> {
        let bind = self.config.bind;
        let listener = tokio::net::TcpListener::bind(bind).await?;
        let app = self.router();
        axum::serve(listener, app).await?;
        Ok(())
    }

    pub fn router(self) -> Router {
        let auth = proto::auth::urc_auth_api_server::UrcAuthApiServer::new(self.clone());
        let rebac = proto::rebac::rebac_api_server::RebacApiServer::new(self.clone());
        Router::new()
            .route("/healthz", get(healthz))
            .route("/.well-known/jwks.json", get(jwks))
            .route("/callback", get(callback))
            .route_service("/epic_urc.UrcAuthApi/HealthCheck", auth.clone())
            .route_service("/epic_urc.UrcAuthApi/StartAuthSession", auth.clone())
            .route_service("/epic_urc.UrcAuthApi/GetAuthSession", auth.clone())
            .route_service("/epic_urc.UrcAuthApi/RefreshAuthSession", auth.clone())
            .route_service("/epic_urc.UrcAuthApi/VerifyUser", auth.clone())
            .route_service(
                "/epic_urc.UrcAuthApi/ExchangeExternalTokenForUserToken",
                auth.clone(),
            )
            .route_service(
                "/epic_urc.UrcAuthApi/ExchangeAPIKeyForUserToken",
                auth.clone(),
            )
            .route_service(
                "/epic_urc.UrcAuthApi/ExchangeUserTokenForMultiresourceToken",
                auth.clone(),
            )
            .route_service("/epic_urc.UrcAuthApi/CheckUserPermission", auth.clone())
            .route_service("/epic_urc.UrcAuthApi/LookupUserPermissions", auth.clone())
            .route_service("/epic_urc.UrcAuthApi/GetUserInfo", auth.clone())
            .route_service("/epic_urc.UrcAuthApi/GetUserId", auth.clone())
            .route_service("/epic_urc.UrcAuthApi/GetProviderUserId", auth)
            .route_service("/ucs.auth.RebacApi/CreateResource", rebac.clone())
            .route_service("/ucs.auth.RebacApi/DeleteResource", rebac)
            .with_state(self)
    }
}

async fn healthz() -> &'static str {
    "ok"
}

async fn jwks(State(state): State<AppState>) -> impl IntoResponse {
    match read_inline_or_file(
        state.config.jwt.public_jwks_json.as_deref(),
        state.config.jwt.public_jwks_json_file.as_ref(),
    )
    .and_then(|jwks| serde_json::from_str::<serde_json::Value>(&jwks).map_err(|_| BridgeError::InvalidJwks))
    {
        Ok(value) => (StatusCode::OK, Json(value)).into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

#[derive(Deserialize)]
struct CallbackQuery {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
}

async fn callback(State(state): State<AppState>, Query(query): Query<CallbackQuery>) -> impl IntoResponse {
    if let Some(error) = query.error {
        return (StatusCode::BAD_REQUEST, error).into_response();
    }
    let Some(code) = query.code else {
        return (StatusCode::BAD_REQUEST, "missing code".to_string()).into_response();
    };
    let Some(session_code) = query.state else {
        return (StatusCode::BAD_REQUEST, "missing state".to_string()).into_response();
    };

    let verifier = {
        let sessions = state.sessions.lock().await;
        sessions
            .sessions
            .get(&session_code)
            .and_then(|session| session.pkce_verifier.clone())
    };

    match state
        .authentik
        .exchange_code(&code, &callback_url(&state.config.public_base_url), verifier.as_deref())
        .await
    {
        Ok(user) => {
            state.users.lock().await.insert(user.clone());
            if state
                .sessions
                .lock()
                .await
                .complete_callback(&session_code, user)
                .is_err()
            {
                return StatusCode::NOT_FOUND.into_response();
            }
            (StatusCode::OK, "login complete").into_response()
        }
        Err(err) => (StatusCode::BAD_GATEWAY, err.to_string()).into_response(),
    }
}

#[tonic::async_trait]
impl proto::auth::urc_auth_api_server::UrcAuthApi for AppState {
    async fn health_check(
        &self,
        _request: Request<proto::auth::HealthCheckRequest>,
    ) -> Result<Response<proto::auth::HealthCheckResponse>, Status> {
        Ok(Response::new(proto::auth::HealthCheckResponse {
            status: "ok".into(),
        }))
    }

    async fn start_auth_session(
        &self,
        request: Request<proto::auth::StartAuthSessionRequest>,
    ) -> Result<Response<proto::auth::StartAuthSessionResponse>, Status> {
        let client_state = request.into_inner().client_state;
        match self.config.authentik.flow {
            FlowMode::Device | FlowMode::Both => {
                let started = self.authentik.start_device().await.map_err(internal)?;
                let session = DeviceSession {
                    client_state,
                    device_code: started.device_code,
                    expires_at: now_secs() as i64 + started.expires_in.unwrap_or(600),
                    interval_seconds: started.interval.unwrap_or(5),
                    user: None,
                    flow: SessionFlow::Device,
                    pkce_verifier: None,
                };
                let session_code = self.sessions.lock().await.insert_device_session(session);
                let login_url = started
                    .verification_uri_complete
                    .or(started.verification_uri)
                    .unwrap_or_default();
                Ok(Response::new(proto::auth::StartAuthSessionResponse {
                    session_code,
                    login_url,
                }))
            }
            FlowMode::Callback => {
                let verifier = Uuid::new_v4().to_string();
                let session = DeviceSession {
                    client_state,
                    device_code: String::new(),
                    expires_at: now_secs() as i64 + 600,
                    interval_seconds: 5,
                    user: None,
                    flow: SessionFlow::Callback,
                    pkce_verifier: Some(verifier.clone()),
                };
                let session_code = self.sessions.lock().await.insert_device_session(session);
                let login_url = self
                    .authentik
                    .authorize_url(&self.config.public_base_url, &session_code, &verifier)
                    .map_err(internal)?;
                Ok(Response::new(proto::auth::StartAuthSessionResponse {
                    session_code,
                    login_url,
                }))
            }
        }
    }

    async fn get_auth_session(
        &self,
        request: Request<proto::auth::GetAuthSessionRequest>,
    ) -> Result<Response<proto::auth::GetAuthSessionResponse>, Status> {
        let request = request.into_inner();
        let mut sessions = self.sessions.lock().await;
        let session = sessions
            .get_device_session_mut(&request.session_code, &request.client_state)
            .map_err(status_from_bridge)?;

        if let Some(user) = session.user.clone() {
            let token = self.jwt.mint_authn(&user).map_err(internal)?;
            return Ok(Response::new(proto::auth::GetAuthSessionResponse {
                user_token: Some(proto::auth::UserToken {
                    user_token: token,
                    expires_at: (now_secs() + 3600) as i64,
                    user_id: user.user_id,
                    user_name: user.name,
                }),
            }));
        }

        if session.flow == SessionFlow::Callback {
            return Ok(Response::new(proto::auth::GetAuthSessionResponse { user_token: None }));
        }

        let device_code = session.device_code.clone();
        drop(sessions);

        match self.authentik.poll_device(&device_code).await.map_err(internal)? {
            DevicePollResult::Pending => Ok(Response::new(proto::auth::GetAuthSessionResponse {
                user_token: None,
            })),
            DevicePollResult::Rejected(err) => Err(Status::permission_denied(err)),
            DevicePollResult::Complete(user) => {
                self.users.lock().await.insert(user.clone());
                let token = self.jwt.mint_authn(&user).map_err(internal)?;
                Ok(Response::new(proto::auth::GetAuthSessionResponse {
                    user_token: Some(proto::auth::UserToken {
                        user_token: token,
                        expires_at: (now_secs() + 3600) as i64,
                        user_id: user.user_id,
                        user_name: user.name,
                    }),
                }))
            }
        }
    }

    async fn refresh_auth_session(
        &self,
        _request: Request<proto::auth::RefreshAuthSessionRequest>,
    ) -> Result<Response<proto::auth::RefreshAuthSessionResponse>, Status> {
        Err(Status::unimplemented("refresh is not supported"))
    }

    async fn verify_user(
        &self,
        request: Request<proto::auth::VerifyUserRequest>,
    ) -> Result<Response<proto::auth::VerifyUserResponse>, Status> {
        let token = request
            .into_inner()
            .target_user
            .and_then(|target| target.user)
            .map(|user| match user {
                proto::auth::target_user::User::UserToken(token) => token,
            })
            .ok_or_else(|| Status::unauthenticated("missing target user token"))?;
        let claims = self.jwt.verify(&token).map_err(|_| Status::unauthenticated("invalid token"))?;
        Ok(Response::new(proto::auth::VerifyUserResponse {
            user_info: Some(proto::auth::UserInfo {
                user_id: claims.user_id,
                display_name: claims.name,
            }),
        }))
    }

    async fn exchange_external_token_for_user_token(
        &self,
        _request: Request<proto::auth::ExchangeExternalTokenForUserTokenRequest>,
    ) -> Result<Response<proto::auth::ExchangeExternalTokenForUserTokenResponse>, Status> {
        Err(Status::unimplemented("external token exchange is not configured"))
    }

    async fn exchange_api_key_for_user_token(
        &self,
        _request: Request<proto::auth::ExchangeApiKeyForUserTokenRequest>,
    ) -> Result<Response<proto::auth::ExchangeApiKeyForUserTokenResponse>, Status> {
        Err(Status::unimplemented("api key exchange is not configured"))
    }

    async fn exchange_user_token_for_multiresource_token(
        &self,
        request: Request<proto::auth::ExchangeUserTokenForMultiresourceTokenRequest>,
    ) -> Result<Response<proto::auth::ExchangeUserTokenForMultiresourceTokenResponse>, Status> {
        let bearer = bearer_token(request.metadata())?;
        let claims = self.jwt.verify(&bearer).map_err(|_| Status::unauthenticated("invalid token"))?;
        let requested = request.into_inner().resource_id;
        let resources = match self.config.policy.default_resource_mode {
            ResourceMode::Wildcard => vec!["urc-*".to_string()],
            ResourceMode::Requested => requested,
        };
        let user = UserProfile {
            user_id: claims.user_id,
            name: claims.name,
            preferred_username: claims.preferred_username,
        };
        let token = self.jwt.mint_authz(&user, &resources).map_err(internal)?;
        Ok(Response::new(
            proto::auth::ExchangeUserTokenForMultiresourceTokenResponse {
                token: Some(proto::auth::UserToken {
                    user_token: token,
                    expires_at: (now_secs() + 900) as i64,
                    user_id: user.user_id,
                    user_name: user.name,
                }),
            },
        ))
    }

    async fn check_user_permission(
        &self,
        request: Request<proto::auth::CheckUserPermissionRequest>,
    ) -> Result<Response<proto::auth::CheckUserPermissionResponse>, Status> {
        let bearer = bearer_token(request.metadata())?;
        let claims = self.jwt.verify(&bearer).map_err(|_| Status::unauthenticated("invalid token"))?;
        let requested = request.into_inner().resource_id;
        let resources = claims.resources.unwrap_or_default();
        let allowed = resources
            .into_iter()
            .filter(|resource| {
                resource.resource_id == "urc-*" || requested.iter().any(|id| id == &resource.resource_id)
            })
            .map(proto_resource)
            .collect();
        Ok(Response::new(proto::auth::CheckUserPermissionResponse {
            allowed_resource_permission: allowed,
            denied_resource_permission: Vec::new(),
        }))
    }

    async fn lookup_user_permissions(
        &self,
        request: Request<proto::auth::LookupUserPermissionsRequest>,
    ) -> Result<Response<proto::auth::LookupUserPermissionsResponse>, Status> {
        let bearer = bearer_token(request.metadata())?;
        let claims = self.jwt.verify(&bearer).map_err(|_| Status::unauthenticated("invalid token"))?;
        let filter = request.into_inner().resource_filter;
        let resources = lookup_user_permissions(&claims.resources.unwrap_or_default(), &filter)
            .into_iter()
            .map(proto_resource)
            .collect();
        Ok(Response::new(proto::auth::LookupUserPermissionsResponse {
            resource_permission: resources,
            next_page_token: None,
        }))
    }

    async fn get_user_info(
        &self,
        request: Request<proto::auth::GetUserInfoRequest>,
    ) -> Result<Response<proto::auth::GetUserInfoResponse>, Status> {
        let request = request.into_inner();
        let user_info = self
            .users
            .lock()
            .await
            .user_info(&request.user_id)
            .into_iter()
            .map(|user| proto::auth::UserInfo {
                user_id: user.user_id,
                display_name: user.name,
            })
            .collect();
        Ok(Response::new(proto::auth::GetUserInfoResponse { user_info }))
    }

    async fn get_user_id(
        &self,
        request: Request<proto::auth::GetUserIdRequest>,
    ) -> Result<Response<proto::auth::GetUserIdResponse>, Status> {
        let request = request.into_inner();
        let user_info = self.users.lock().await.user_id(&request.user_display_name);
        Ok(Response::new(proto::auth::GetUserIdResponse {
            user_info: user_info.map(|user| proto::auth::UserInfo {
                user_id: user.user_id,
                display_name: user.name,
            }),
        }))
    }

    async fn get_provider_user_id(
        &self,
        request: Request<proto::auth::GetProviderUserIdRequest>,
    ) -> Result<Response<proto::auth::GetProviderUserIdResponse>, Status> {
        let user_id = request.into_inner().user_id;
        Ok(Response::new(proto::auth::GetProviderUserIdResponse {
            user_id: user_id.clone(),
            provider_user_id: user_id,
        }))
    }
}

#[tonic::async_trait]
impl proto::rebac::rebac_api_server::RebacApi for AppState {
    async fn create_resource(
        &self,
        request: Request<proto::rebac::CreateResourceRequest>,
    ) -> Result<Response<proto::rebac::CreateResourceResponse>, Status> {
        self.registry
            .lock()
            .await
            .create(&request.into_inner().resource_id)
            .await
            .map_err(internal)?;
        Ok(Response::new(proto::rebac::CreateResourceResponse {}))
    }

    async fn delete_resource(
        &self,
        request: Request<proto::rebac::DeleteResourceRequest>,
    ) -> Result<Response<proto::rebac::DeleteResourceResponse>, Status> {
        self.registry
            .lock()
            .await
            .delete(&request.into_inner().resource_id)
            .await
            .map_err(internal)?;
        Ok(Response::new(proto::rebac::DeleteResourceResponse {}))
    }
}

pub struct ResourceRegistry {
    path: Option<PathBuf>,
    resources: Vec<String>,
}

impl ResourceRegistry {
    fn new(path: Option<PathBuf>) -> Self {
        let resources = path
            .as_ref()
            .and_then(|path| std::fs::read_to_string(path).ok())
            .and_then(|contents| serde_json::from_str(&contents).ok())
            .unwrap_or_default();
        Self { path, resources }
    }

    async fn create(&mut self, resource_id: &str) -> Result<(), BridgeError> {
        if !self.resources.iter().any(|id| id == resource_id) {
            self.resources.push(resource_id.to_string());
            self.persist().await?;
        }
        Ok(())
    }

    async fn delete(&mut self, resource_id: &str) -> Result<(), BridgeError> {
        self.resources.retain(|id| id != resource_id);
        self.persist().await
    }

    async fn persist(&self) -> Result<(), BridgeError> {
        if let Some(path) = &self.path {
            if let Some(parent) = path.parent() {
                tokio::fs::create_dir_all(parent).await?;
            }
            tokio::fs::write(path, serde_json::to_vec_pretty(&self.resources).unwrap()).await?;
        }
        Ok(())
    }
}

fn proto_resource(resource: ResourcePermission) -> proto::auth::ResourcePermission {
    proto::auth::ResourcePermission {
        resource_id: resource.resource_id,
        permission: resource.permission,
    }
}

fn bearer_token(metadata: &tonic::metadata::MetadataMap) -> Result<String, Status> {
    let value = metadata
        .get("authorization")
        .ok_or_else(|| Status::unauthenticated("missing authorization"))?
        .to_str()
        .map_err(|_| Status::unauthenticated("invalid authorization metadata"))?;
    Ok(value
        .strip_prefix("Bearer ")
        .unwrap_or(value)
        .to_string())
}

fn status_from_bridge(err: BridgeError) -> Status {
    match err {
        BridgeError::SessionNotFound => Status::not_found(err.to_string()),
        BridgeError::ClientStateMismatch => Status::permission_denied(err.to_string()),
        _ => Status::internal(err.to_string()),
    }
}

fn internal(err: impl std::fmt::Display) -> Status {
    Status::internal(err.to_string())
}

fn jwks_key_id(public_jwks_json: &str) -> Result<String, BridgeError> {
    let value: serde_json::Value =
        serde_json::from_str(public_jwks_json).map_err(|_| BridgeError::InvalidJwks)?;
    value
        .get("keys")
        .and_then(|keys| keys.as_array())
        .and_then(|keys| keys.first())
        .and_then(|key| key.get("kid"))
        .and_then(|kid| kid.as_str())
        .map(str::to_string)
        .ok_or(BridgeError::InvalidJwks)
}

fn jwks_decoding_key(public_jwks_json: &str) -> Result<DecodingKey, BridgeError> {
    let value: serde_json::Value =
        serde_json::from_str(public_jwks_json).map_err(|_| BridgeError::InvalidJwks)?;
    let jwk: jsonwebtoken::jwk::Jwk = serde_json::from_value(
        value
            .get("keys")
            .and_then(|keys| keys.as_array())
            .and_then(|keys| keys.first())
            .cloned()
            .ok_or(BridgeError::InvalidJwks)?,
    )
    .map_err(|_| BridgeError::InvalidJwks)?;
    DecodingKey::from_jwk(&jwk).map_err(BridgeError::Jwt)
}

fn callback_url(public_base_url: &str) -> String {
    format!("{}/callback", public_base_url.trim_end_matches('/'))
}

fn pkce_challenge(verifier: &str) -> String {
    use base64::Engine;
    let digest = ring::digest::digest(&ring::digest::SHA256, verifier.as_bytes());
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(digest.as_ref())
}

fn read_inline_or_file(inline: Option<&str>, file: Option<&PathBuf>) -> Result<String, BridgeError> {
    match (inline, file) {
        (Some(value), _) => Ok(value.to_string()),
        (None, Some(path)) => Ok(std::fs::read_to_string(path)?),
        (None, None) => Err(BridgeError::Config(config::ConfigError::Message(
            "missing jwt key material".to_string(),
        ))),
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before UNIX epoch")
        .as_secs()
}

#[cfg(test)]
mod tests {
    use jsonwebtoken::DecodingKey;
    use jsonwebtoken::Validation;

    use super::*;

    const TEST_PRIVATE_KEY: &str = r#"-----BEGIN PRIVATE KEY-----
MIIEvgIBADANBgkqhkiG9w0BAQEFAASCBKgwggSkAgEAAoIBAQDJETqse41HRBsc
7cfcq3ak4oZWFCoZlcic525A3FfO4qW9BMtRO/iXiyCCHn8JhiL9y8j5JdVP2Q9Z
IpfElcFd3/guS9w+5RqQGgCR+H56IVUyHZWtTJbKPcwWXQdNUX0rBFcsBzCRESJL
eelOEdHIjG7LRkx5l/FUvlqsyHDVJEQsHwegZ8b8C0fz0EgT2MMEdn10t6Ur1rXz
jMB/wvCg8vG8lvciXmedyo9xJ8oMOh0wUEgxziVDMMovmC+aJctcHUAYubwoGN8T
yzcvnGqL7JSh36Pwy28iPzXZ2RLhAyJFU39vLaHdljwthUaupldlNyCfa6Ofy4qN
ctlUPlN1AgMBAAECggEAdESTQjQ70O8QIp1ZSkCYXeZjuhj081CK7jhhp/4ChK7J
GlFQZMwiBze7d6K84TwAtfQGZhQ7km25E1kOm+3hIDCoKdVSKch/oL54f/BK6sKl
qlIzQEAenho4DuKCm3I4yAw9gEc0DV70DuMTR0LEpYyXcNJY3KNBOTjN5EYQAR9s
2MeurpgK2MdJlIuZaIbzSGd+diiz2E6vkmcufJLtmYUT/k/ddWvEtz+1DnO6bRHh
xuuDMeJA/lGB/EYloSLtdyCF6sII6C6slJJtgfb0bPy7l8VtL5iDyz46IKyzdyzW
tKAn394dm7MYR1RlUBEfqFUyNK7C+pVMVoTwCC2V4QKBgQD64syfiQ2oeUlLYDm4
CcKSP3RnES02bcTyEDFSuGyyS1jldI4A8GXHJ/lG5EYgiYa1RUivge4lJrlNfjyf
dV230xgKms7+JiXqag1FI+3mqjAgg4mYiNjaao8N8O3/PD59wMPeWYImsWXNyeHS
55rUKiHERtCcvdzKl4u35ZtTqQKBgQDNKnX2bVqOJ4WSqCgHRhOm386ugPHfy+8j
m6cicmUR46ND6ggBB03bCnEG9OtGisxTo/TuYVRu3WP4KjoJs2LD5fwdwJqpgtHl
yVsk45Y1Hfo+7M6lAuR8rzCi6kHHNb0HyBmZjysHWZsn79ZM+sQnLpgaYgQGRbKV
DZWlbw7g7QKBgQCl1u+98UGXAP1jFutwbPsx40IVszP4y5ypCe0gqgon3UiY/G+1
zTLp79GGe/SjI2VpQ7AlW7TI2A0bXXvDSDi3/5Dfya9ULnFXv9yfvH1QwWToySpW
Kvd1gYSoiX84/WCtjZOr0e0HmLIb0vw0hqZA4szJSqoxQgvF22EfIWaIaQKBgQCf
34+OmMYw8fEvSCPxDxVvOwW2i7pvV14hFEDYIeZKW2W1HWBhVMzBfFB5SE8yaCQy
pRfOzj9aKOCm2FjjiErVNpkQoi6jGtLvScnhZAt/lr2TXTrl8OwVkPrIaN0bG/AS
aUYxmBPCpXu3UjhfQiWqFq/mFyzlqlgvuCc9g95HPQKBgAscKP8mLxdKwOgX8yFW
GcZ0izY/30012ajdHY+/QK5lsMoxTnn0skdS+spLxaS5ZEO4qvPVb8RAoCkWMMal
2pOhmquJQVDPDLuZHdrIiKiDM20dy9sMfHygWcZjQ4WSxf/J7T9canLZIXFhHAZT
3wc9h4G8BBCtWN2TN/LsGZdB
-----END PRIVATE KEY-----"#;

    const TEST_JWKS: &str = r#"{"keys":[{"kty":"RSA","n":"yRE6rHuNR0QbHO3H3Kt2pOKGVhQqGZXInOduQNxXzuKlvQTLUTv4l4sggh5_CYYi_cvI-SXVT9kPWSKXxJXBXd_4LkvcPuUakBoAkfh-eiFVMh2VrUyWyj3MFl0HTVF9KwRXLAcwkREiS3npThHRyIxuy0ZMeZfxVL5arMhw1SRELB8HoGfG_AtH89BIE9jDBHZ9dLelK9a184zAf8LwoPLxvJb3Il5nncqPcSfKDDodMFBIMc4lQzDKL5gvmiXLXB1AGLm8KBjfE8s3L5xqi-yUod-j8MtvIj812dkS4QMiRVN_by2h3ZY8LYVGrqZXZTcgn2ujn8uKjXLZVD5TdQ","e":"AQAB","kid":"rsa01","alg":"RS256","use":"sig"}]}"#;

    fn issuer() -> JwtIssuer {
        JwtIssuer::from_pem(
            "https://auth.lore.example",
            vec!["lore.example".to_string()],
            TEST_PRIVATE_KEY,
            TEST_JWKS,
        )
        .unwrap()
    }

    fn user() -> UserProfile {
        UserProfile {
            user_id: "user-1".into(),
            name: "Ada Lovelace".into(),
            preferred_username: "ada".into(),
        }
    }

    fn decode(token: &str) -> LoreClaims {
        let jwk: jsonwebtoken::jwk::Jwk =
            serde_json::from_value(serde_json::from_str::<serde_json::Value>(TEST_JWKS).unwrap()["keys"][0].clone())
                .unwrap();
        let mut validation = Validation::new(Algorithm::RS256);
        validation.set_audience(&["lore.example"]);
        validation.set_issuer(&["https://auth.lore.example"]);
        jsonwebtoken::decode::<LoreClaims>(token, &DecodingKey::from_jwk(&jwk).unwrap(), &validation)
            .unwrap()
            .claims
    }

    #[test]
    fn auth_session_state_rejects_mismatched_client_state() {
        let mut store = SessionStore::default();
        let session_code = store.insert_device_session(DeviceSession {
            client_state: "client-a".into(),
            device_code: "device-code".into(),
            expires_at: 42,
            interval_seconds: 5,
            user: None,
            flow: SessionFlow::Device,
            pkce_verifier: None,
        });

        let err = store
            .get_device_session(&session_code, "client-b")
            .unwrap_err();

        assert!(matches!(err, BridgeError::ClientStateMismatch));
    }

    #[test]
    fn device_code_pending_maps_to_empty_get_auth_session() {
        let result = map_device_poll(DevicePoll::Pending, &issuer()).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn device_code_success_mints_lore_authn_jwt_claims() {
        let token = map_device_poll(DevicePoll::Complete(user()), &issuer())
            .unwrap()
            .unwrap();
        let header = jsonwebtoken::decode_header(&token).unwrap();
        let claims = decode(&token);

        assert_eq!(header.alg, Algorithm::RS256);
        assert_eq!(header.kid.as_deref(), Some("rsa01"));
        assert_eq!(claims.issuer, "https://auth.lore.example");
        assert_eq!(claims.user_id, "user-1");
        assert_eq!(claims.audience, vec!["lore.example"]);
        assert_eq!(claims.name, "Ada Lovelace");
        assert_eq!(claims.preferred_username, "ada");
        assert_eq!(claims.env, "prod");
        assert!(claims.issued_at > 0);
        assert!(claims.expires > claims.issued_at);
        assert!(claims.resources.is_none());
    }

    #[test]
    fn resource_exchange_mints_authz_jwt_with_requested_resources() {
        let token = issuer()
            .mint_authz(
                &user(),
                &["urc-abc".to_string(), "urc-def".to_string()],
            )
            .unwrap();
        let claims = decode(&token);

        assert_eq!(
            claims.resources.unwrap(),
            vec![
                ResourcePermission {
                    resource_id: "urc-abc".into(),
                    permission: vec!["read".into(), "write".into(), "admin".into()],
                },
                ResourcePermission {
                    resource_id: "urc-def".into(),
                    permission: vec!["read".into(), "write".into(), "admin".into()],
                },
            ]
        );
    }

    #[test]
    fn lookup_user_permissions_returns_only_matching_urc_resources() {
        let resources = vec![
            ResourcePermission {
                resource_id: "urc-abc".into(),
                permission: vec!["read".into()],
            },
            ResourcePermission {
                resource_id: "project-abc".into(),
                permission: vec!["read".into()],
            },
            ResourcePermission {
                resource_id: "urc-def".into(),
                permission: vec!["write".into()],
            },
        ];

        let filtered = lookup_user_permissions(&resources, "urc");

        assert_eq!(filtered.len(), 2);
        assert!(filtered.iter().all(|resource| resource.resource_id.starts_with("urc")));
    }

    #[test]
    fn cached_users_resolve_by_id_and_display_name() {
        let mut cache = UserCache::default();
        cache.insert(user());

        assert_eq!(cache.user_info(&["user-1".into()]), vec![user()]);
        assert_eq!(cache.user_id("ada"), Some(user()));
        assert_eq!(cache.user_id("unknown"), None);
    }
}
