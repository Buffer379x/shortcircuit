use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Method, Request, StatusCode, header},
    response::Response,
};
use serde_json::{Value, json};
use shortcircuit::{
    AdminConfig, Auth, BootstrapKind, PublicHosts, admin_router, open_database, public_router,
};
use tower::ServiceExt;

const ADMIN: &str = "admin.example";
const ORIGIN: &str = "https://admin.example";

struct Fixture {
    _directory: tempfile::TempDir,
    public: Router,
    admin: Router,
    cookie: String,
    csrf: String,
    auth: Auth,
}

impl Fixture {
    async fn start() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let pool = open_database(&directory.path().join("test.db")).await.unwrap();
        let hosts = PublicHosts::parse("go.example,s.example").unwrap();
        let auth = Auth::new(pool.clone());
        let claim = auth.issue_bootstrap(BootstrapKind::Setup).await.unwrap();
        let session = auth.setup(&claim, "Owner", "synthetic-test-password").await.unwrap();
        let public = public_router(pool.clone(), hosts.clone());
        let admin = admin_router(pool, hosts, AdminConfig::new(ORIGIN,8080).unwrap());
        Self {
            _directory: directory,
            auth,
            admin,
            public,
            cookie: format!("__Host-shortcircuit_session={}", session.token),
            csrf: session.session.csrf,
        }
    }

    async fn admin_request(&self, method: Method, path: &str, body: Option<Value>) -> Response {
        send(&self.admin,method,path,&[
            ("host",ADMIN),
            ("origin",ORIGIN),
            ("cookie",&self.cookie),
            ("x-csrf-token",&self.csrf),
        ],body).await
    }

    async fn create(&self, host: &str, slug: &str, target: &str, tags: &[&str]) -> i64 {
        let response = self.admin_request(Method::POST,"/api/v1/links",Some(json!({
            "host":host,"slug":slug,"target":target,"tags":tags
        }))).await;
        assert_eq!(response.status(),StatusCode::CREATED);
        read_json(response).await["id"].as_i64().unwrap()
    }

    async fn visit(&self, host: &str, method: Method, path: &str) -> Response {
        send(&self.public, method, path, &[("host",host)], None).await
    }
}

async fn send(router: &Router, method: Method, path: &str, headers: &[(&str,&str)], body: Option<Value>) -> Response {
    let mut request = Request::builder().method(method).uri(path);
    for (name,value) in headers {
        request=request.header(*name,*value);
    }
    let body=match body {
        Some(data) => {
            request=request.header(header::CONTENT_TYPE,"application/json");
            Body::from(data.to_string())
        }
        None => Body::empty()
    };
    router.clone().oneshot(request.body(body).unwrap()).await.unwrap()
}

async fn read_json(resp: Response) -> Value {
    let data=to_bytes(resp.into_body(),100_000).await.unwrap();
    serde_json::from_slice(&data).unwrap()
}

#[tokio::test]
async fn managed_domains_preserve_links_and_use_explicit_fallbacks() {
    let app=Fixture::start().await;
    let domain="new.example";
    assert_eq!(app.visit(domain,Method::GET,"/").await.status(),StatusCode::NOT_FOUND);
    let mut settings=json!({
        "host":domain, "origin":"https://new.example:8443", "enabled":true,
        "root_target":"https://example.test/home", "missing_target":"https://example.test/help"
    });
    let set=app.admin_request(Method::POST,"/api/v1/catalog/domains",Some(settings.clone())).await;
    assert_eq!(set.status(),StatusCode::OK);
    assert_eq!(app.visit(domain,Method::GET,"/").await.headers()[header::LOCATION],"https://example.test/home");
    assert_eq!(app.visit(domain,Method::GET,"/not-here").await.headers()[header::LOCATION],"https://example.test/help");
    app.create(domain,"docs","https://example.test/docs?use=as%2fis#start",&[]).await;
    settings["enabled"]=json!(false);
    assert_eq!(app.admin_request(Method::POST,"/api/v1/catalog/domains",Some(settings.clone())).await.status(),StatusCode::OK);
    assert_eq!(app.visit(domain,Method::GET,"/docs").await.status(),StatusCode::NOT_FOUND);
    let listed=read_json(app.admin_request(Method::GET,"/api/v1/links",None).await).await;
    assert_eq!(listed["items"][0]["slug"],"docs");
    settings["enabled"]=json!(true);
    assert_eq!(app.admin_request(Method::POST,"/api/v1/catalog/domains",Some(settings.clone())).await.status(),StatusCode::OK);
    let effective=app.visit(domain,Method::GET,"/docs").await;
    assert_eq!(effective.headers()[header::LOCATION],"https://example.test/docs?use=as%2fis#start");
}

#[tokio::test]
async fn ordered_policy_uses_raw_forwarding_and_durable_limit() {
    let app=Fixture::start().await;
    let id=app.create("go.example","guide","https://example.test/guide?source=stored#introduction",&[]).await;
    let path=format!("/api/v1/links/{id}/policy");
    let policy=json!({
        "status":307,"starts_at":null,"expires_at":null,"max_visits":1,"forward_query":true,"forward_path":true,
        "rules":[{"target":"https://example.test/local?value=A%2Fb#help","conditions":[{"type":"query","key":"mode","value":"local"}]}]
    });
    assert_eq!(app.admin_request(Method::PATCH,&path,Some(policy)).await.status(),StatusCode::OK);
    let peek=app.visit("go.example",Method::HEAD,"/guide/child?mode=local&term=a%2fb").await;
    assert_eq!(peek.status(),StatusCode::TEMPORARY_REDIRECT);
    assert_eq!(peek.headers()[header::LOCATION],"https://example.test/local/child?value=A%2Fb&mode=local&term=a%2fb#help");
    let hit=app.visit("go.example",Method::GET,"/guide/child?mode=local&term=a%2fb").await;
    assert_eq!(hit.status(),StatusCode::TEMPORARY_REDIRECT);
    assert_eq!(app.visit("go.example",Method::GET,"/guide").await.status(),StatusCode::NOT_FOUND);
    let details=read_json(app.admin_request(Method::GET,&path,None).await).await;
    assert_eq!(details["visits_used"],1);
}

#[tokio::test]
async fn tag_merge_and_qr_respect_existing_link_identity() {
    let app=Fixture::start().await;
    let id=app.create("go.example","test/guide","https://example.test/",&["first", "second"]).await;
    let changed=app.admin_request(Method::PATCH,"/api/v1/catalog/tags",Some(json!({"name":"first","new_name":"second"}))).await;
    assert_eq!(changed.status(),StatusCode::NO_CONTENT);
    let items=read_json(app.admin_request(Method::GET,"/api/v1/links",None).await).await;
    assert_eq!(items["items"][0]["tags"],json!(["second"]));

    let png=app.admin_request(Method::GET,&format!("/api/v1/links/{id}/qr?format=png&size=320"),None).await;
    assert_eq!(png.status(),StatusCode::OK);
    assert_eq!(png.headers()[header::CONTENT_TYPE],"image/png");
    assert!(to_bytes(png.into_body(),1_000_000).await.unwrap().starts_with(b"\x89PNG\r\n\x1a\n"));

    let scope=app.auth.create_key("Other domain",vec!["read".into()],Some("s.example".into()),None).await.unwrap();
    let denied=send(&app.admin,Method::GET,&format!("/api/v1/links/{id}/qr"),
        &[("host",ADMIN),("authorization",&format!("Bearer {}",scope.token))],None).await;
    assert_eq!(denied.status(),StatusCode::NOT_FOUND);
}
