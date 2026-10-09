use axum::{
    Router,
    extract::State,
    http::StatusCode,
    response::Html,
    routing::get
};
use minijinja::{
    Environment,
    context
};
use std::sync::Arc;

fn internal_error(e: minijinja::Error) -> StatusCode {
    eprintln!("template error: {e:#}");
    StatusCode::INTERNAL_SERVER_ERROR
}

struct AppState {
    templates: Environment<'static>
}

#[axum::debug_handler]
async fn index(State(state): State<Arc<AppState>>) -> Result<Html<String>, StatusCode> {
    let template = state.templates.get_template("index.html").map_err(internal_error)?;
    let body = template.render(context! { name => "x" }).map_err(internal_error)?;
    Ok(Html(body))
}

#[tokio::main]
async fn main() {
    let mut env = Environment::new();
    env.set_loader(minijinja::path_loader("templates"));
    let state = AppState{templates: env};

    let app = Router::new()
        .route("/", get(index))
        .with_state(Arc::new(state));

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await.unwrap();
    println!("listening on {}", listener.local_addr().unwrap());
    axum::serve(listener, app).await.unwrap();
}
