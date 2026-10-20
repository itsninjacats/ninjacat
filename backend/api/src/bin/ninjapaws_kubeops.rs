//! orchestrator.<site> and kubeops-intake.<site>: what the cluster agent sends, Kubernetes and
//! ECS collections, manifests and the results of remote actions. It takes the place of those
//! hosts of the F# `ninjacat-api intake`, with the same environment.

use ninjapaws_intake::auth::Scheme;
use ninjapaws_intake::routers::kubeops;
use tako::types::BoxError;

#[tokio::main]
async fn main() -> Result<(), BoxError> {
    ninjapaws_intake::server::run("ninjapaws_kubeops", Some(Scheme::Agent), kubeops::mount).await
}
