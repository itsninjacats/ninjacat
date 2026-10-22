//! install.datadoghq.com and install.datad0g.com: the installer's OCI registry, BTF archives and
//! the HEAD probe; anonymous, as its clients send no key.

use ninjapaws_intake::routers::install;
use ninjapaws_intake::server;
use tako::router::Router;
use tako::types::BoxError;

#[tokio::main]
async fn main() -> Result<(), BoxError> {
    server::init_tracing();
    let settings = server::Settings::from_env();

    let mut router = Router::new();
    router.head("/", install::handle_probe);
    router.get("/v2/", install::handle_version_check);
    router.get("/v2/{repo}/manifests/{tag}", install::handle_manifest);
    router.get("/v2/{repo}/blobs/{digest}", install::handle_blob);
    // The last segment is <kernel>.btf.tar.xz.
    router.get("/btfs/{distribution}/{version}/{architecture}/{kernel}", install::handle_btf);

    // Without a key, on every host.
    router.get("/ping", server::ping);
    router.get("/_health", server::health);
    router.head("/support/flare", server::flare_probe);
    router.head("/support/flare/{case}", server::flare_probe);
    // Anything else: logged, and 404.
    router.middleware(server::log_405);
    router.fallback(server::not_found);

    // Nothing here stores anything or asks for a key.
    router.with_state(settings);
    server::serve("ninjapaws_install", router).await
}
