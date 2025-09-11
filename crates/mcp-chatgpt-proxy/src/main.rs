use std::{
    collections::{HashMap, HashSet},
    net::SocketAddr,
    sync::Arc,
};

use axum::{
    Router as AxumRouter,
    extract::{Form, Json, Query, State},
    http::{Request, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Redirect},
    routing::{get, post},
};
use axum::{body::Body, http::HeaderMap};
use clap::Parser;
use futures::FutureExt;
use llm::mcp::{McpContext, load_mcp_servers};
use llm::tools::ToolExecutor;
use rand::{Rng, distr::Alphanumeric};
use rmcp::{
    ErrorData,
    handler::server::{
        ServerHandler,
        router::{Router as McpRouter, tool::ToolRoute},
        tool::IntoCallToolResult,
    },
    model::{ServerCapabilities, ServerInfo, Tool},
    transport::streamable_http_server::{
        StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
    },
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::{net::TcpListener, sync::Mutex};

#[derive(Parser)]
struct Args {
    /// Path to MCP configuration JSON
    #[arg(long)]
    mcp: Option<String>,
    /// Address to bind the HTTP server to
    #[arg(long, default_value = "127.0.0.1:8080")]
    addr: String,
    /// The host that the server will be visible as, "https://example.com"
    #[arg(long)]
    host: String,
    /// The header to check during authorize
    #[arg(long)]
    auth_header: String,
    /// The header value to check during authorize
    #[arg(long)]
    auth_value: String,
    /// The redirect_uri to allow during authorize
    #[arg(long)]
    allowed_redirect_uri: String,
}

#[derive(Clone)]
struct ProxyService {
    ctx: McpContext,
}

struct OAuthState {
    clients: Mutex<HashMap<String, RegisteredClient>>,
    codes: Mutex<HashMap<String, AuthCode>>,
    tokens: Mutex<HashSet<String>>,
    args: Args,
}

struct RegisteredClient {
    redirect_uris: Vec<String>,
}

struct AuthCode {
    client_id: String,
    redirect_uri: String,
}

#[derive(Serialize)]
struct AuthorizationMetadata {
    authorization_endpoint: String,
    token_endpoint: String,
    registration_endpoint: String,
    issuer: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    jwks_uri: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    scopes_supported: Option<Vec<String>>,
}

#[derive(Deserialize)]
struct RegisterRequest {
    client_name: String,
    redirect_uris: Vec<String>,
}

#[derive(Serialize)]
struct RegisterResponse {
    client_id: String,
    client_secret: Option<String>,
    client_name: String,
    redirect_uris: Vec<String>,
}

#[derive(Deserialize)]
struct AuthorizeQuery {
    client_id: String,
    redirect_uri: String,
    state: String,
}

#[derive(Deserialize)]
struct TokenForm {
    grant_type: String,
    code: String,
    redirect_uri: String,
}

impl ServerHandler for ProxyService {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            ..ServerInfo::default()
        }
    }
}

async fn well_known(State(state): State<Arc<OAuthState>>) -> Json<AuthorizationMetadata> {
    Json(AuthorizationMetadata {
        authorization_endpoint: format!("https://{}/authorize", state.args.host).to_string(),
        token_endpoint: format!("https://{}/token", state.args.host).to_string(),
        registration_endpoint: format!("https://{}/register", state.args.host).to_string(),
        issuer: format!("https://{}", state.args.host).to_string(),
        jwks_uri: None,
        scopes_supported: None,
    })
}

async fn register(
    State(state): State<Arc<OAuthState>>,
    Json(req): Json<RegisterRequest>,
) -> Json<RegisterResponse> {
    let client_id: String = rand::rng()
        .sample_iter(&Alphanumeric)
        .take(16)
        .map(char::from)
        .collect();
    let resp = RegisterResponse {
        client_id: client_id.clone(),
        client_secret: Some("secret".into()),
        client_name: req.client_name.clone(),
        redirect_uris: req.redirect_uris.clone(),
    };
    let client = RegisteredClient {
        redirect_uris: req.redirect_uris,
    };
    state.clients.lock().await.insert(client_id, client);
    Json(resp)
}

async fn authorize(
    State(state): State<Arc<OAuthState>>,
    Query(params): Query<AuthorizeQuery>,
    headers: HeaderMap,
) -> Result<Redirect, (StatusCode, String)> {
    let valid_client = {
        let clients = state.clients.lock().await;
        clients
            .get(&params.client_id)
            .map(|c| c.redirect_uris.contains(&params.redirect_uri))
            .unwrap_or(false)
    };
    if !valid_client {
        // We're happy to accept just the redirect_uri.
        // return Err((StatusCode::BAD_REQUEST, "invalid client".into()));
    }

    if params.redirect_uri != state.args.allowed_redirect_uri {
        return Err((StatusCode::BAD_REQUEST, "invalid redirect".into()));
    }

    headers
        .get(&state.args.auth_header)
        .and_then(|v| v.to_str().ok())
        .filter(|v| *v == state.args.auth_value)
        .ok_or((StatusCode::UNAUTHORIZED, "not authorized".into()))?;

    let code: String = rand::rng()
        .sample_iter(&Alphanumeric)
        .take(40)
        .map(char::from)
        .collect();
    state.codes.lock().await.insert(
        code.clone(),
        AuthCode {
            client_id: params.client_id.clone(),
            redirect_uri: params.redirect_uri.clone(),
        },
    );
    let redirect_uri = format!(
        "{}?code={}&state={}",
        params.redirect_uri, code, params.state
    );
    Ok(Redirect::to(&redirect_uri))
}

#[derive(Serialize)]
struct TokenResponse {
    access_token: String,
    token_type: String,
    expires_in: u64,
}

async fn token(
    State(state): State<Arc<OAuthState>>,
    Form(form): Form<TokenForm>,
) -> Result<Json<TokenResponse>, (StatusCode, String)> {
    if form.grant_type != "authorization_code" {
        return Err((StatusCode::BAD_REQUEST, "unsupported grant_type".into()));
    }
    let code = { state.codes.lock().await.remove(&form.code) };
    let Some(code) = code else {
        return Err((StatusCode::BAD_REQUEST, "invalid code".into()));
    };
    if code.redirect_uri != form.redirect_uri {
        return Err((StatusCode::BAD_REQUEST, "invalid code".into()));
    }
    let token: String = rand::rng()
        .sample_iter(&Alphanumeric)
        .take(40)
        .map(char::from)
        .collect();
    state.tokens.lock().await.insert(token.clone());
    let resp = TokenResponse {
        access_token: token,
        token_type: "bearer".to_string(),
        expires_in: 3600,
    };
    Ok(Json(resp))
}

async fn auth(
    State(state): State<Arc<OAuthState>>,
    req: Request<Body>,
    next: Next,
) -> Result<impl IntoResponse, StatusCode> {
    if let Some(value) = req.headers().get(axum::http::header::AUTHORIZATION) {
        if let Ok(value) = value.to_str() {
            if let Some(token) = value.strip_prefix("Bearer ") {
                if state.tokens.lock().await.contains(token) {
                    return Ok(next.run(req).await);
                }
            }
        }
    }
    Err(StatusCode::UNAUTHORIZED)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    let ctx = if let Some(path) = &args.mcp {
        load_mcp_servers(path).await.expect("mcp")
    } else {
        McpContext::default()
    };

    let session_manager = Arc::new(LocalSessionManager::default());
    let ctx_clone = ctx.clone();
    let service = StreamableHttpService::new(
        move || Ok(build_router(ctx_clone.clone())),
        session_manager,
        StreamableHttpServerConfig::default(),
    );

    let addr: SocketAddr = args.addr.parse()?;
    let oauth_state = Arc::new(OAuthState {
        args,
        clients: Mutex::new(Default::default()),
        codes: Mutex::new(Default::default()),
        tokens: Mutex::new(Default::default()),
    });
    let oauth_router = AxumRouter::new()
        .route("/.well-known/oauth-authorization-server", get(well_known))
        .route("/register", post(register))
        .route("/authorize", get(authorize))
        .route("/token", post(token))
        .with_state(oauth_state.clone());

    let protected = AxumRouter::new()
        .nest_service("/mcp", service)
        .route_layer(middleware::from_fn_with_state(oauth_state.clone(), auth));

    let app = oauth_router.merge(protected);

    let listener = TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

fn build_router(ctx: McpContext) -> McpRouter<ProxyService> {
    let service = ProxyService { ctx: ctx.clone() };
    let mut routes = Vec::new();
    for info in ctx.tool_infos() {
        let name = info.name.clone();
        let desc = info.description.clone();
        let schema_value = serde_json::to_value(info.parameters)
            .unwrap_or_else(|_| Value::Object(Default::default()));
        let schema_obj = match schema_value {
            Value::Object(obj) => obj,
            _ => Default::default(),
        };
        let tool = Tool::new(name.clone(), desc, schema_obj);
        let ctx_clone = ctx.clone();
        let route = ToolRoute::new_dyn(tool, move |mut tc| {
            let ctx = ctx_clone.clone();
            let name = name.clone();
            async move {
                let args = tc.arguments.take().unwrap_or_default();
                let value = Value::Object(args);
                let text = ctx
                    .call(&name, value)
                    .await
                    .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
                text.into_call_tool_result()
            }
            .boxed()
        });
        routes.push(route);
    }
    McpRouter::new(service).with_tools(routes)
}
