/// The region of monitor entities, the manager and the notifier as singletons, and how nodes find each other.
module NinjaCat.Api.Monitors.Cluster

open System
open Akka.Actor
open Akka.Cluster.Hosting
open Akka.Cluster.Hosting.SBR
open Akka.Cluster.Sharding
open Akka.DependencyInjection
open Akka.Discovery.KubernetesApi
open Akka.Hosting
open Akka.Management
open Akka.Management.Cluster.Bootstrap
open Akka.Remote.Hosting
open Akka.Serialization
open NinjaCat.Api.Monitors.Settings
open NinjaCat.Api.Monitors.Definitions
open NinjaCat.Api.Monitors.Messages
open NinjaCat.Api.Monitors.Entity
open NinjaCat.Api.Monitors.Manager
open NinjaCat.Api.Monitors.Notifier

/// A monitor's shard is its key's hash modulo this, so changing it moves
/// every monitor and needs every node stopped first.
let private shards = 100

let private metrics = "metrics"

let private notifierRole = "notifier"

/// A key that does not parse has no id, so the region drops its message with
/// a warning; given to `entityProps`, it would fail the whole shard.
let private entityId (message: obj) : string =
    let monitor =
        match message with
        | :? Ensure as ensure -> ensure.Monitor
        | :? Delete as delete -> delete.Monitor
        | _ -> null

    match Option.ofObj monitor |> Option.bind MonitorKey.Parse with
    | Some key -> key.Text
    | None -> null

let private entityProps (services: Services) (entityId: string) : Props =
    MonitorEntity.Props(Option.get (MonitorKey.Parse entityId), services)

/// `manager` is given the node's registry, where the region is found once it has started.
let configure
    (node: Node)
    (entity: Services)
    (manager: IReadOnlyActorRegistry -> ManagerServices)
    (notifier: NotifierServices)
    (akka: AkkaConfigurationBuilder)
    : AkkaConfigurationBuilder =
    let seeds =
        match node.Formation with
        | Seeds seeds -> Array.ofList seeds
        | Kubernetes _ -> null

    let clustering =
        ClusterOptions(
            // One Deployment carries both roles for now.
            Roles = [| metrics; notifierRole |],
            // Keep-majority, so with three replicas one lost node is downed
            // and the other two carry on.
            SplitBrainResolver = SplitBrainResolverOption.Default,
            SeedNodes = seeds
        )

    let sharding =
        ShardOptions(
            Role = metrics,
            // The default, persistence, keeps the coordinator's state in the
            // in-memory journal, lost when the coordinator moves.
            StateStoreMode = Nullable StateStoreMode.DData,
            RememberEntities = Nullable false,
            // An entity lives on its timer; the manager's Ensure on every
            // resync also keeps it alive, and starts it again after a hand-off.
            ShouldPassivateIdleEntities = Nullable false,
            HandOffStopMessage = Stop
        )

    let configured =
        akka
            .WithRemoting(RemoteOptions(HostName = "0.0.0.0", PublicHostName = node.Host, Port = Nullable node.Port))
            .WithClustering(clustering)
            .WithStrictSerialization()
            .WithCustomSerializer(
                "monitors",
                [ typeof<IMonitorMessage> ],
                fun (system: ExtendedActorSystem) -> MessageSerializer system :> Serializer
            )
            .WithShardRegion<MonitorRegion>(
                "metric-monitor",
                Func<string, Props>(entityProps entity),
                HashCodeMessageExtractor.Create(shards, Func<obj, string> entityId),
                sharding
            )
            .WithSingleton<MonitorManager>(
                "monitor-manager",
                Func<ActorSystem, IActorRegistry, IDependencyResolver, Props>(fun _ registry _ ->
                    MonitorManager.Props(manager registry)),
                ClusterSingletonOptions(Role = metrics),
                createProxyToo = false
            )
            .WithSingleton<MonitorNotifier>(
                "monitor-notifier",
                MonitorNotifier.Props notifier,
                ClusterSingletonOptions(Role = notifierRole),
                createProxyToo = false
            )

    match node.Formation with
    | Seeds _ -> configured
    | Kubernetes selector ->
        // Two, not the replica count: with three, a first formation while
        // only two pods can be scheduled would never complete.
        // The chart names the management container port "management";
        // discovery takes the contact point's port from it.
        configured
            .WithAkkaManagement(hostName = node.Host, port = Nullable node.ManagementPort)
            .WithClusterBootstrap(portName = "management", requiredContactPoints = Nullable 2)
            .WithKubernetesDiscovery(fun (options: KubernetesDiscoveryOptions) ->
                match selector with
                | Some s -> options.PodLabelSelector <- s
                | None -> ())
