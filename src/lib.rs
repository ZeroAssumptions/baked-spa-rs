use std::marker::PhantomData;

use axum::handler::Handler;
use axum::Router;
use axum::body::{HttpBody, Body, boxed, Full};
use axum::http::{StatusCode, header, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::{get_service, get};
use rust_embed::RustEmbed;

static INDEX_PATH: &str = "index.html";

pub struct SpaRouter<A, B = Body, T = ()> where A: RustEmbed {
    _assets: PhantomData<A>,
    _marker: PhantomData<fn() -> (B, T)>,
}

impl<A, B, T> SpaRouter<A, B, T> where
A: RustEmbed + 'static, {
    pub fn new() -> Self {
        Self {
            _assets: Default::default(),
            _marker: Default::default(),
        }
    }
}

impl<A, B, T> From<SpaRouter<A, B, T>> for Router<B>
where
    B: HttpBody + Send + 'static,
    T: 'static,
    A: RustEmbed + 'static,
{
    fn from(spa: SpaRouter<A, B, T>) -> Self {
        Router::new()
            .nest("/assets", get(assets_handler::<A>))
            .fallback(get_service(serve_index::<A>.into_service()))
    }
}
async fn serve_asset<A: RustEmbed>(path: &str) -> Response {
    dbg!(path);
    if let Some(index) = A::get(path) {
        let body = boxed(Full::from(index.data));
        let mime = mime_guess::from_path(path).first_or_octet_stream();
        let etag = base64::encode(index.metadata.sha256_hash());
        Response::builder()
            .header(header::CONTENT_TYPE, mime.as_ref())
            .header(header::ETAG, etag)
            .body(body)
            .unwrap_or_else(|_| not_found())
    } else {
        not_found()
    }
}

async fn assets_handler<A: RustEmbed>(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    let path = format!("assets/{path}");
    serve_asset::<A>(&path).await
}

async fn serve_index<A: RustEmbed>() -> Response {
    serve_asset::<A>(INDEX_PATH).await
}

fn not_found() -> Response {
    (StatusCode::NOT_FOUND, "Not found").into_response()
}


#[cfg(test)]
mod tests {
    use super::*;
    use axum_test_helper::TestClient;

    #[derive(RustEmbed)]
    #[folder = "fixture/"]
    struct TestAssets;

    #[tokio::test]
    async fn rust_embed_as_file_provider() {
        let resp = serve_index::<TestAssets>().await;
        assert_eq!(200, resp.status())
    }

    #[tokio::test]
    async fn basic() {
        let app = Router::new()
            .route("/foo", get(|| async { "GET /foo" }))
            .merge(SpaRouter::new() as SpaRouter<TestAssets>);
        let client = TestClient::new(app);

        let res = client.get("/").send().await;
        dbg!(res.headers());
        assert_eq!(res.status(), StatusCode::OK);
        assert!(res.headers().get(header::ETAG).is_some());
        assert_eq!(res.headers().get(header::CONTENT_TYPE).unwrap().as_bytes(), b"text/html");
        assert_eq!(res.text().await, "<h1>Hello, World!</h1>\n");

        let res = client.get("/some/random/path").send().await;
        assert_eq!(res.status(), StatusCode::OK);
        assert!(res.headers().get(header::ETAG).is_some());
        assert_eq!(res.text().await, "<h1>Hello, World!</h1>\n");

        let res = client.get("/assets/script.js").send().await;
        assert_eq!(res.status(), StatusCode::OK);
        assert!(res.headers().get(header::ETAG).is_some());
        assert_eq!(res.headers().get(header::CONTENT_TYPE).unwrap().as_bytes(), b"application/javascript");
        assert_eq!(res.text().await, "console.log('hi')\n");

        let res = client.get("/foo").send().await;
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(res.text().await, "GET /foo");

        let res = client.get("/assets/doesnt_exist").send().await;
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
    }
}
