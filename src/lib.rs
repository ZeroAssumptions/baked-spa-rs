use std::marker::PhantomData;

use axum::body::{boxed, Body, Full, HttpBody};
use axum::handler::HandlerWithoutStateExt;
use axum::http::{header, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::get_service;
use axum::Router;
use rust_embed::RustEmbed;

static INDEX_PATH: &str = "index.html";

pub struct SpaRouter<A, B = Body, T = (), S = ()>
where
    A: RustEmbed,
{
    path: &'static str,
    _assets: PhantomData<A>,
    _marker: PhantomData<fn() -> (B, T, S)>,
}

impl<A, B, T, S> SpaRouter<A, B, T, S>
where
    A: RustEmbed + 'static,
{
    pub fn new(path: &'static str) -> Self {
        Self {
            path,
            _assets: Default::default(),
            _marker: Default::default(),
        }
    }
}

impl<A, B, T, S> From<SpaRouter<A, B, T, S>> for Router<S, B>
where
    B: HttpBody + Send + 'static,
    T: 'static,
    A: RustEmbed + 'static,
    S: Clone + Send + Sync + 'static,
{
    fn from(spa: SpaRouter<A, B, T, S>) -> Self {
        Router::new()
            .nest_service(spa.path, get_service(assets_handler::<A>.into_service()))
            .fallback_service(get_service(serve_index::<A>.into_service()))
    }
}
async fn serve_asset<A: RustEmbed>(path: &str) -> Response {
    if let Some(index) = A::get(path).or_else(|| A::get(INDEX_PATH)) {
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
    if uri.path() == "/" {
        serve_index::<A>().await
    } else {
        let path = uri.path().trim_start_matches('/');
        serve_asset::<A>(path).await
    }
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
    use axum::response::Redirect;
    use axum::routing::get;
    use axum_test_helper::TestClient;

    #[derive(RustEmbed)]
    #[folder = "fixture/"]
    struct TestAssets;

    #[derive(RustEmbed)]
    #[folder = "fixture-coordinator/"]
    struct TestAssetsCoordinator;

    #[tokio::test]
    async fn rust_embed_as_file_provider() {
        let resp = serve_index::<TestAssets>().await;
        assert_eq!(200, resp.status())
    }

    #[tokio::test]
    async fn basic() {
        let app = Router::new()
            .route("/foo", get(|| async { "GET /foo" }))
            .merge(SpaRouter::new("/") as SpaRouter<TestAssets>);
        let client = TestClient::new(app);

        let res = client.get("/").send().await;
        assert_eq!(res.status(), StatusCode::OK);
        assert!(res.headers().get(header::ETAG).is_some());
        assert_eq!(
            res.headers().get(header::CONTENT_TYPE).unwrap().as_bytes(),
            b"text/html"
        );
        assert_eq!(res.text().await, "<h1>Hello, World!</h1>\n");

        let res = client.get("/some/random/path").send().await;
        assert_eq!(res.status(), StatusCode::OK);
        assert!(res.headers().get(header::ETAG).is_some());
        assert_eq!(res.text().await, "<h1>Hello, World!</h1>\n");

        let res = client.get("/assets/script.js").send().await;
        assert_eq!(res.status(), StatusCode::OK);
        assert!(res.headers().get(header::ETAG).is_some());
        assert_eq!(
            res.headers().get(header::CONTENT_TYPE).unwrap().as_bytes(),
            b"application/javascript"
        );
        assert_eq!(res.text().await, "console.log('hi')\n");

        let res = client.get("/foo").send().await;
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(res.text().await, "GET /foo");

        let res = client.get("/assets/doesnt_exist").send().await;
        assert_eq!(res.status(), StatusCode::OK);
        assert!(res.headers().get(header::ETAG).is_some());
        assert_eq!(res.text().await, "<h1>Hello, World!</h1>\n");
    }

    #[tokio::test]
    async fn coordinator_routing() {
        let app = Router::new()
            .route("/api", get(|| async { "OK" }))
            .route("/", get(|| async { Redirect::permanent("/ui/") }))
            .merge(SpaRouter::new("/ui") as SpaRouter<TestAssetsCoordinator>);

        let client = TestClient::new(app);

        // `GET /` will redirect to `/ui/`
        // `GET /ui` will serve index
        // `GET /ui/` will serve index
        // `GET /ui/script.js` will serve `script.js`
        // `GET /ui/doesnt_exist` will serve index
        // `GET /api/` will serve `OK`

        let res = client.get("/").send().await;
        assert_eq!(res.status(), StatusCode::PERMANENT_REDIRECT);

        let res = client.get("/ui").send().await;
        assert_eq!(res.status(), StatusCode::OK);
        assert!(res.headers().get(header::ETAG).is_some());
        assert_eq!(res.text().await, "<h1>Hello, World!</h1>\n");

        let res = client.get("/ui/").send().await;
        assert_eq!(res.status(), StatusCode::OK);
        assert!(res.headers().get(header::ETAG).is_some());
        assert_eq!(res.text().await, "<h1>Hello, World!</h1>\n");

        let res = client.get("/ui/script.js").send().await;
        assert_eq!(res.status(), StatusCode::OK);
        assert!(res.headers().get(header::ETAG).is_some());
        assert_eq!(
            res.headers().get(header::CONTENT_TYPE).unwrap().as_bytes(),
            b"application/javascript"
        );
        assert_eq!(res.text().await, "console.log('hi')\n");

        let res = client.get("/ui/doesnt_exist").send().await;
        assert_eq!(res.status(), StatusCode::OK);
        assert!(res.headers().get(header::ETAG).is_some());
        assert_eq!(res.text().await, "<h1>Hello, World!</h1>\n");

        let res = client.get("/api").send().await;
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(res.text().await, "OK");
    }
}
