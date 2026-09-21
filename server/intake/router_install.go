package intake

import (
	"log"
	"net/http"

	"github.com/gin-gonic/gin"
)

// Hosts that do not follow the prefix + site scheme — see docs section 1.8.
//
//	install.datadoghq.com / install.datad0g.com   OCI registry, BTF files, HEAD probe
//	eudm-intake.<site>                            /api/v2/aiusage
//	llmobs-intake.<site>                          /api/v2/llmobs (diagnostic probe only)
//
// None of these carry Dd-Api-Key except the two POSTs, so the install host
// cannot sit behind RequireAPIKey — that is a routes.go concern.
//
// Nothing is stored yet.

// routeInstall — install.datadoghq.com / install.datad0g.com.
//
//	config: installer.registry.url (OCI), none for /btfs (hardcoded host)
//
// The fleet installer speaks OCI Distribution: it expects an image index
// under the tag, layers under blob digests and Datadog annotations on the
// manifest. We do not host packages, so instead of a fake 202 — which
// go-containerregistry would reject anyway — we answer the protocol's own
// "not found": 404 with an OCI error body. That is not a network error, so
// the installer fails immediately with a readable message instead of retrying.
func (a *Server) routeInstall(g *gin.RouterGroup) {
	g.HEAD("/", a.handleInstallProbe)
	g.GET("/v2/", a.handleOCIVersion)
	g.GET("/v2/:repo/manifests/:tag", a.handleOCIManifest)
	g.GET("/v2/:repo/blobs/:digest", a.handleOCIBlob)
	g.GET("/btfs/:platform/:ver/:arch/:kernel", a.handleBTF) // <kernel>.btf.tar.xz
}

// handleInstallProbe answers the diagnose sweep: any 2xx is "Success".
func (a *Server) handleInstallProbe(c *gin.Context) {
	log.Printf("[install] HEAD probe from %s", c.GetHeader("User-Agent"))
	c.Status(http.StatusOK)
}

// handleOCIVersion is the API version check every registry client starts with.
func (a *Server) handleOCIVersion(c *gin.Context) {
	log.Printf("[install] OCI version check from %s", c.GetHeader("User-Agent"))
	ociHeaders(c)
	c.JSON(http.StatusOK, gin.H{})
}

func (a *Server) handleOCIManifest(c *gin.Context) {
	repo, tag := c.Param("repo"), c.Param("tag")
	log.Printf("[install] OCI manifest %s:%s (Accept: %s) — not hosted",
		repo, tag, c.GetHeader("Accept"))
	ociError(c, "MANIFEST_UNKNOWN", "ninjacat does not host packages: "+repo+":"+tag)
}

func (a *Server) handleOCIBlob(c *gin.Context) {
	repo, digest := c.Param("repo"), c.Param("digest")
	log.Printf("[install] OCI blob %s@%s — not hosted", repo, digest)
	ociError(c, "BLOB_UNKNOWN", "ninjacat does not host packages: "+repo+"@"+digest)
}

// handleBTF — system-probe fetching kernel BTF archives. The file's SHA256 is
// checked against the BTF_DD remote config catalog, so we could not serve a
// substitute even if we wanted to. A non-200 makes the loader fall back to
// its next BTF source.
func (a *Server) handleBTF(c *gin.Context) {
	log.Printf("[install] BTF %s/%s/%s/%s — not hosted",
		c.Param("platform"), c.Param("ver"), c.Param("arch"), c.Param("kernel"))
	c.JSON(http.StatusNotFound, gin.H{"errors": []string{"BTF archives are not hosted here"}})
}

// ociHeaders marks the response as coming from a Distribution API v2 registry.
func ociHeaders(c *gin.Context) {
	c.Header("Docker-Distribution-API-Version", "registry/2.0")
}

// ociError writes a 404 in the OCI Distribution error format, which
// go-containerregistry parses into a *transport.Error.
func ociError(c *gin.Context, code, message string) {
	ociHeaders(c)
	c.JSON(http.StatusNotFound, gin.H{
		"errors": []gin.H{{"code": code, "message": message}},
	})
}

// routeAIUsage — eudm-intake.<site>.
//
//	config: none — the Rust ai_prompt_logger posts through the local evp_proxy,
//	        which adds the API key. The client only checks for a 2xx.
//
// The producer is not in datadog-agent and publishes no schema, so nothing is
// decoded: the body is kept as the bytes that arrived.
func (a *Server) routeAIUsage(g *gin.RouterGroup) {
	g.POST("/api/v2/aiusage", a.handleAIUsage)
}

func (a *Server) handleAIUsage(c *gin.Context) {
	defer c.JSON(http.StatusAccepted, gin.H{})
	body, err := c.GetRawData()
	if err != nil {
		log.Printf("[aiusage] cannot read body: %v", err)
		return
	}
	describe("aiusage", c.GetHeader("Content-Type"), body)

	// body is the ai_prompt_logger payload byte for byte; the proxy's origin
	// headers (DD-EVP-ORIGIN, DD-EVP-ORIGIN-VERSION) and Content-Type are on c.
	_ = body // TODO(ninjacat): tables. Complete, unconverted, ready to take.
}

// routeLLMObs — llmobs-intake.<site>.
//
// The agent itself never sends LLM Observability data here; the only caller is
// the connectivity diagnose, which POSTs a nil body and wants a 2xx. There is
// no payload to decode, so a body — if one ever shows up — is only described.
func (a *Server) routeLLMObs(g *gin.RouterGroup) {
	g.POST("/api/v2/llmobs", a.handleLLMObs)
}

func (a *Server) handleLLMObs(c *gin.Context) {
	defer c.JSON(http.StatusAccepted, gin.H{})
	body, err := c.GetRawData()
	if err != nil {
		log.Printf("[llmobs] cannot read body: %v", err)
		return
	}
	if len(body) == 0 || isDiagnose(c) {
		log.Printf("[llmobs] diagnose probe from %s", c.GetHeader("User-Agent"))
		return
	}
	log.Printf("[llmobs] unexpected payload — the agent only probes this host")
	describe("llmobs", c.GetHeader("Content-Type"), body)

	// Not expected, but not dropped either: body is whatever was sent, byte
	// for byte.
	_ = body // TODO(ninjacat): tables. Complete, unconverted, ready to take.
}
